// The unlocked account and its vaults, held only in the service worker's
// memory.
//
// Every operation runs through `serial` (the message router wraps each
// request), so a sync never interleaves with an edit or a lock. Functions
// here assume they already run inside it; `sync()` queues a new round. When
// the browser stops the service worker, the objects are gone; the next
// request reopens them from the device key in session storage (no password
// needed until the person locks or the auto-lock fires).
//
// Shared vaults (family and team) follow docs/SERVER.md "Sharing". The
// server is not trusted to say who is who: a vault key is accepted only when
// it was signed by this account or by someone whose safety number the person
// compared, and changes to a shared vault are accepted only when signed by
// one of those people.

import { Api, ApiError, checkServerUrl } from "./api.ts";
import { loadCore, type Account, type LocalVault } from "./core.ts";
import { fromB64, toB64 } from "../shared/b64.ts";
import {
  clearAccount,
  getAccount,
  getQuick,
  getSettings,
  putAccount,
  putQuick,
  type StoredAccount,
  type StoredVault,
} from "./store.ts";
import type {
  FieldInput,
  ItemKind,
  ItemView,
  LoginMatch,
  MembersView,
  Role,
  SharingView,
  Strength,
  VaultKind,
  VaultTag,
  VaultView,
  WaitingView,
} from "../shared/types.ts";

/** Pinned OPAQUE public keys of servers this build trusts without asking. */
const PINNED: Record<string, string> = {};

/** Field that marks the hidden item holding a shared vault's name and kind. */
export const VAULT_INFO = "custom:amanfold.vault";

interface Open {
  account: Account;
  me: Uint8Array;
  personalId: string;
  vaults: Map<string, LocalVault>;
}

let open: Open | undefined;
let kit: { secretKey: string; recoveryWords: string; email: string } | undefined;
let syncError: string | undefined;
/** Shared vaults whose key came from someone not yet verified. */
let waiting: WaitingView[] = [];
let chain: Promise<unknown> = Promise.resolve();

export function serial<T>(fn: () => Promise<T>): Promise<T> {
  const run = chain.then(fn, fn);
  chain = run.catch(() => undefined);
  return run;
}

function fail(name: string, message = name): never {
  throw Object.assign(new Error(message), { name });
}

function deviceName(): string {
  const ua = navigator.userAgent;
  const browser = /Firefox\//.test(ua)
    ? "Firefox"
    : /Edg\//.test(ua)
      ? "Edge"
      : /OPR\//.test(ua)
        ? "Opera"
        : /Vivaldi\//.test(ua)
          ? "Vivaldi"
          : /Safari\//.test(ua) && !/Chrome\//.test(ua)
            ? "Safari"
            : "Chrome";
  return `Amanfold for ${browser}`;
}

export function currentKit() {
  return kit;
}

export function kitSaved() {
  kit = undefined;
}

export function lastSyncError() {
  return syncError;
}

const canWrite = (r: Role) => r !== "reader";
const canManage = (r: Role) => r === "owner" || r === "manager";

// ---------------------------------------------------------------------------
// Locking
// ---------------------------------------------------------------------------

// Last use, kept here too for browsers where quick unlock is off (Safari).
let lastActiveMs = 0;

async function touch() {
  lastActiveMs = Date.now();
  const q = await getQuick();
  if (q) await putQuick({ ...q, lastActiveMs: Date.now() });
}

async function rememberUnlock(account: Account) {
  lastActiveMs = Date.now();
  const q = account.enableQuickUnlock();
  await putQuick({ deviceKey: toB64(q.deviceKey), wrappedKeys: toB64(q.wrappedKeys), lastActiveMs: Date.now() });
  q.free();
}

/**
 * The open vaults, reopening them after a service worker restart. Reading
 * the vaults is not activity: only `active()` and `need()`, which serve
 * requests the person made, push the auto-lock deadline back. Alarms,
 * badges, tab events and pages must never keep the vault open.
 */
export async function unlocked(): Promise<Open | undefined> {
  if (open) {
    const settings = await getSettings();
    if (expired(lastActiveMs, settings.autoLockMinutes)) {
      await lock();
      return undefined;
    }
    return open;
  }
  const [stored, quick, settings] = await Promise.all([getAccount(), getQuick(), getSettings()]);
  if (!stored || !quick) return undefined;
  if (expired(quick.lastActiveMs, settings.autoLockMinutes)) {
    await lock();
    return undefined;
  }
  const core = await loadCore();
  try {
    const account = core.quickUnlock(fromB64(stored.bundle), fromB64(quick.deviceKey), fromB64(quick.wrappedKeys));
    open = openAll(account, stored);
    // Carry the last real use over the restart; reopening is not activity.
    lastActiveMs = quick.lastActiveMs;
  } catch {
    await lock();
    return undefined;
  }
  return open;
}

/** The open vault for a request the person made (popup, shortcut). */
export async function active(): Promise<Open | undefined> {
  const o = await unlocked();
  if (o) await touch();
  return o;
}

function expired(lastActiveMs: number, minutes: number): boolean {
  return minutes > 0 && Date.now() - lastActiveMs > minutes * 60_000;
}

export async function autoLockCheck() {
  const [quick, settings] = await Promise.all([getQuick(), getSettings()]);
  const last = quick?.lastActiveMs ?? (open ? lastActiveMs : undefined);
  if (last !== undefined && expired(last, settings.autoLockMinutes)) await lock();
}

export async function lock() {
  if (open) {
    for (const v of open.vaults.values()) v.free();
    open.account.free();
  }
  open = undefined;
  // The recovery kit opens the account: never show it once locked.
  kit = undefined;
  waiting = [];
  await putQuick(undefined);
}

export async function need(): Promise<Open> {
  return (await active()) ?? fail("Locked", "the vault is locked");
}

/** Public keys of who signed a stored grant ("" = this account). */
function granterOf(o: { me: Uint8Array }, v: StoredVault): Uint8Array {
  return v.granter ? fromB64(v.granter) : o.me;
}

function openAll(account: Account, s: StoredAccount): Open {
  const me = account.publicKeys();
  const o: Open = { account, me, personalId: s.personalId, vaults: new Map() };
  for (const v of Object.values(s.vaults)) {
    try {
      o.vaults.set(v.id, account.openVault(fromB64(v.grant), granterOf(o, v), v.cache ? fromB64(v.cache) : undefined));
    } catch (e) {
      // The personal vault must open; a shared one that fails is skipped
      // and opened again from the server on the next sync.
      if (v.id === s.personalId) throw e;
      delete s.vaults[v.id];
    }
  }
  return o;
}

export async function unlock(password: string) {
  const stored = (await getAccount()) ?? fail("NoAccount");
  const core = await loadCore();
  const account = core.unlockAccount(fromB64(stored.bundle), password, stored.secretKey);
  await lock();
  open = openAll(account, stored);
  await rememberUnlock(account);
  void sync().catch(() => undefined);
}

// ---------------------------------------------------------------------------
// Sign-up and sign-in
// ---------------------------------------------------------------------------

async function pinnedKey(api: Api): Promise<string> {
  const pinned = PINNED[api.base];
  const info = await api.get<{ opaque_public_key: string }>("/v1/server");
  if (pinned && info.opaque_public_key !== pinned) fail("ServerKey", "the server's key is not the one this app trusts");
  return pinned ?? info.opaque_public_key;
}

export async function serverInfo(server: string) {
  const api = new Api(checkServerUrl(server));
  return api.get<{ email_verification: boolean; version: string }>("/v1/server");
}

export async function signupEmail(server: string, email: string) {
  await new Api(checkServerUrl(server)).post("/v1/signup/email", { email });
}

export async function signup(server: string, email: string, password: string, code?: string) {
  if ((await getAccount()) !== undefined) fail("HasAccount");
  const api = new Api(checkServerUrl(server));
  const serverKey = await pinnedKey(api);
  const core = await loadCore();
  const created = core.createAccount(password);
  const account = created.account;
  const reg = created.registration;
  const bundle = created.bundle;
  const start = await api.post<{ response: string }>("/v1/signup/start", {
    email,
    code,
    account_id: account.accountId(),
    request: toB64(reg.request()),
  });
  const upload = reg.finish(fromB64(start.response));
  const fin = await api.post<{ account_id: string; api_token: string }>("/v1/signup/finish", {
    email,
    code,
    upload: toB64(upload),
    bundle: toB64(bundle),
    device_name: deviceName(),
  });
  api.token = fin.api_token;
  const nv = account.createVault();
  const grant = nv.grant;
  await api.post("/v1/vaults", { grant: toB64(grant) });
  const vault = nv.vault;
  const id = vault.id();
  const stored: StoredAccount = {
    server: api.base,
    email,
    accountId: fin.account_id,
    bundle: toB64(bundle),
    secretKey: created.secretKey,
    serverKey,
    token: fin.api_token,
    personalId: id,
    vaults: { [id]: { id, role: "owner", grant: toB64(grant), granter: "", cache: toB64(vault.exportCache()), seq: 0, pending: {} } },
    contacts: {},
  };
  const newKit = { secretKey: created.secretKey, recoveryWords: created.recoveryWords, email };
  created.free();
  await putAccount(stored);
  await lock();
  kit = newKit;
  open = { account, me: account.publicKeys(), personalId: id, vaults: new Map([[id, vault]]) };
  await rememberUnlock(account);
}

export async function signin(server: string, email: string, password: string, secretKeyText: string) {
  if ((await getAccount()) !== undefined) fail("HasAccount");
  const core = await loadCore();
  const secretKey = core.checkSecretKey(secretKeyText);
  const api = new Api(checkServerUrl(server));
  const serverKey = await pinnedKey(api);
  const pre = await api.post<{ prelogin: string }>("/v1/prelogin", { email });
  const session = new core.LoginSession(fromB64(pre.prelogin), password, secretKey);
  const start = await api.post<{ login_id: string; response: string }>("/v1/login/start", {
    email,
    request: toB64(session.start()),
  });
  const out = session.finish(fromB64(start.response), fromB64(serverKey));
  const fin = await api.post<{ account_id: string; bundle: string }>("/v1/login/finish", {
    login_id: start.login_id,
    finalization: toB64(out.finalization),
    device_name: deviceName(),
  });
  const account = session.unlock(fromB64(fin.bundle));
  api.token = toB64(out.apiToken);
  out.free();
  session.free();

  // The personal vault: the first one this account owns. Shared vaults are
  // picked up by the sync below (their owners' safety numbers must be
  // compared again on this device before they open).
  const { vaults } = await api.get<{ vaults: { vault_id: string; role: Role }[] }>("/v1/vaults");
  const mine = vaults.find((v) => v.role === "owner") ?? fail("NoVault", "this account has no vault yet");
  const grant = await newestGrant(api, mine.vault_id);
  const stored: StoredAccount = {
    server: api.base,
    email,
    accountId: fin.account_id,
    bundle: fin.bundle,
    secretKey,
    serverKey,
    token: api.token,
    personalId: mine.vault_id,
    vaults: { [mine.vault_id]: { id: mine.vault_id, role: "owner", grant, granter: "", seq: 0, pending: {} } },
    contacts: {},
  };
  await lock();
  open = openAll(account, stored);
  stored.vaults[mine.vault_id]!.cache = toB64(open.vaults.get(mine.vault_id)!.exportCache());
  await putAccount(stored);
  await rememberUnlock(account);
  await syncNow();
}

async function newestGrant(api: Api, vaultId: string): Promise<string> {
  const v = await api.get<{ grants: { epoch: number; grant: string }[] }>(`/v1/vaults/${vaultId}`);
  const g = v.grants.at(-1) ?? fail("NoGrant");
  return g.grant;
}

export async function signout() {
  const stored = await getAccount();
  if (stored) {
    await new Api(stored.server, stored.token).del("/v1/sessions/current").catch(() => undefined);
  }
  await lock();
  kit = undefined;
  syncError = undefined;
  await clearAccount();
}

// ---------------------------------------------------------------------------
// Items across vaults
// ---------------------------------------------------------------------------

function isInfo(i: ItemView): boolean {
  return i.fields.some((f) => f.name === VAULT_INFO);
}

/** Name and kind of a vault, from its hidden info item. */
function vaultInfo(v: LocalVault, personal: boolean): { name: string; kind: VaultKind } {
  if (personal) return { name: "", kind: "personal" };
  const info = (v.items(false) as ItemView[]).find(isInfo);
  const kind = info?.fields.find((f) => f.name === VAULT_INFO)?.value === "team" ? "team" : "family";
  return { name: info?.fields.find((f) => f.name === "title")?.value ?? "", kind };
}

function tags(o: Open, stored: StoredAccount): Map<string, VaultTag> {
  const out = new Map<string, VaultTag>();
  for (const [id, v] of o.vaults) {
    const personal = id === stored.personalId;
    out.set(id, {
      id,
      name: vaultInfo(v, personal).name,
      shared: !personal,
      readOnly: !canWrite(stored.vaults[id]?.role ?? "reader"),
    });
  }
  return out;
}

export async function items(trashed = false): Promise<ItemView[]> {
  const o = await need();
  const stored = (await getAccount())!;
  const t = tags(o, stored);
  const out: ItemView[] = [];
  for (const [id, v] of o.vaults) {
    for (const i of v.items(trashed) as ItemView[]) if (!isInfo(i)) out.push({ ...i, vault: t.get(id) });
  }
  return out;
}

/** The vault holding an item, and the item. */
async function locate(id: string): Promise<{ vaultId: string; vault: LocalVault; item: ItemView } | undefined> {
  const o = await need();
  for (const [vaultId, vault] of o.vaults) {
    const item = vault.item(id) as ItemView | undefined;
    if (item && !isInfo(item)) return { vaultId, vault, item };
  }
  return undefined;
}

export async function item(id: string): Promise<ItemView | undefined> {
  const found = await locate(id);
  if (!found) return undefined;
  const stored = (await getAccount())!;
  return { ...found.item, vault: tags(await need(), stored).get(found.vaultId) };
}

const ORDER: Record<Strength, number> = { Origin: 3, Host: 2, Site: 1, None: 0 };

/** Logins for a page from every vault, best match first. */
export function loginsFor(o: Open, url: string): LoginMatch[] {
  const all: LoginMatch[] = [];
  for (const [id, v] of o.vaults) {
    const matches = v.loginsForUrl(url) as LoginMatch[];
    if (matches.length === 0) continue;
    const shared = id !== o.personalId;
    const vault: VaultTag = { id, name: shared ? vaultInfo(v, false).name : "", shared, readOnly: false };
    for (const m of matches) all.push({ ...m, item: { ...m.item, vault } });
  }
  return all.sort((a, b) => ORDER[b.strength] - ORDER[a.strength]);
}

/** Saved sites (in any vault) that this page imitates. */
export function lookalikesFor(o: Open, url: string): string[] {
  const all = new Set<string>();
  for (const v of o.vaults.values()) for (const s of v.lookalikeSites(url)) all.add(s);
  return [...all];
}

async function saveLocal(vaultId: string, id: string, record: Uint8Array) {
  const o = await need();
  const stored = (await getAccount())!;
  const sv = stored.vaults[vaultId] ?? fail("NotFound");
  sv.pending[id] = toB64(record);
  sv.cache = toB64(o.vaults.get(vaultId)!.exportCache());
  await putAccount(stored);
}

function writable(stored: StoredAccount, vaultId: string) {
  const sv = stored.vaults[vaultId] ?? fail("NotFound");
  if (!canWrite(sv.role)) fail("ReadOnly", "this shared vault is read-only for you");
}

export async function create(kind: ItemKind, fields: FieldInput[], vaultId?: string): Promise<string> {
  const o = await need();
  const stored = (await getAccount())!;
  const target = vaultId ?? stored.personalId;
  writable(stored, target);
  const vault = o.vaults.get(target) ?? fail("NotFound");
  const saved = vault.createItem(kind, fields);
  const id = saved.id;
  await saveLocal(target, id, saved.record);
  saved.free();
  void sync().catch(() => undefined);
  return id;
}

export async function update(id: string, set: FieldInput[], remove: string[]) {
  const found = (await locate(id)) ?? fail("NotFound");
  writable((await getAccount())!, found.vaultId);
  const saved = found.vault.updateItem(id, set, remove);
  await saveLocal(found.vaultId, id, saved.record);
  saved.free();
  void sync().catch(() => undefined);
}

export async function trash(id: string, trashed: boolean) {
  const found = (await locate(id)) ?? fail("NotFound");
  writable((await getAccount())!, found.vaultId);
  const saved = found.vault.setTrashed(id, trashed);
  await saveLocal(found.vaultId, id, saved.record);
  saved.free();
  void sync().catch(() => undefined);
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

/** Sync in the background, after the current operation. */
export function sync(): Promise<void> {
  return serial(syncNow);
}

function remember(e: unknown) {
  const err = e as Error;
  if (err instanceof ApiError && err.status === 401) syncError = "signedOut";
  else if (err.name === "Rollback" || err.name === "BadSignature") syncError = "rollback";
  else if (err instanceof ApiError && err.kind === "offline") syncError = "offline";
  else syncError = err.name || "error";
}

/**
 * One round of the sync loop in docs/SERVER.md for every vault: pick up
 * vaults shared with this account, sync each (retrying on a conflict), and
 * rotate the key of a vault someone was removed from.
 */
export async function syncNow(): Promise<void> {
  const o = await unlocked();
  if (!o) return;
  let stored = (await getAccount())!;
  const api = new Api(stored.server, stored.token);
  let firstError: unknown;
  try {
    const list = await discover(o, stored, api);
    stored = (await getAccount())!;
    for (const id of [...o.vaults.keys()]) {
      try {
        await syncVault(o, id, api);
        const row = list.find((v) => v.vault_id === id);
        if (row?.needs_rotation && canManage(stored.vaults[id]?.role ?? "reader")) await rotate(o, id, api);
      } catch (e) {
        // A shared vault that fails to sync must not stop the others.
        firstError ??= e;
        if (id === stored.personalId) throw e;
      }
    }
  } catch (e) {
    remember(e);
    throw e;
  }
  if (firstError) {
    remember(firstError);
    throw firstError;
  }
  syncError = undefined;
  stored = (await getAccount())!;
  stored.lastSyncMs = Date.now();
  await putAccount(stored);
}

interface VaultRow {
  vault_id: string;
  owner_id: string;
  role: Role;
  active: boolean;
  needs_rotation: boolean;
}
interface MemberRow {
  account_id: string;
  email: string;
  role: Role;
  active: boolean;
  public_keys: string;
}

/** Is this public key this account, or someone whose safety number was compared? */
function trustedKey(o: Open, stored: StoredAccount, keys: string): boolean {
  return keys === toB64(o.me) || Object.values(stored.contacts).some((c) => c.keys === keys);
}

function writers(o: Open, stored: StoredAccount): Uint8Array[] {
  return [o.me, ...Object.values(stored.contacts).map((c) => fromB64(c.keys))];
}

/** Open vaults newly shared with this account; drop ones it left. */
async function discover(o: Open, stored: StoredAccount, api: Api): Promise<VaultRow[]> {
  const { vaults } = await api.get<{ vaults: VaultRow[] }>("/v1/vaults");
  const nextWaiting: WaitingView[] = [];
  for (const row of vaults) {
    const have = stored.vaults[row.vault_id];
    if (have) {
      have.role = row.role;
      continue;
    }
    if (!row.active) continue;
    const grant = await newestGrant(api, row.vault_id);
    const core = await loadCore();
    const granter = toB64(core.grantGranter(fromB64(grant)));
    if (!trustedKey(o, stored, granter)) {
      // Opened only after the person compares the sharer's safety number.
      const { members } = await api.get<{ members: MemberRow[] }>(`/v1/vaults/${row.vault_id}/members`);
      const who = members.find((m) => m.public_keys === granter);
      if (who) {
        nextWaiting.push({ vaultId: row.vault_id, accountId: who.account_id, email: who.email, fingerprint: core.fingerprint(fromB64(granter)) });
      }
      continue;
    }
    const vault = o.account.openVault(fromB64(grant), fromB64(granter), undefined);
    o.vaults.set(row.vault_id, vault);
    stored.vaults[row.vault_id] = { id: row.vault_id, role: row.role, grant, granter, cache: toB64(vault.exportCache()), seq: 0, pending: {} };
  }
  for (const id of Object.keys(stored.vaults)) {
    if (id !== stored.personalId && !vaults.some((v) => v.vault_id === id)) {
      // Removed from it (or it was deleted).
      o.vaults.get(id)?.free();
      o.vaults.delete(id);
      delete stored.vaults[id];
    }
  }
  waiting = nextWaiting;
  await putAccount(stored);
  return vaults;
}

async function syncVault(o: Open, id: string, api: Api) {
  for (let attempt = 0; attempt < 4; attempt++) {
    try {
      if (await syncOnce(o, id, api)) return;
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) continue;
      throw e;
    }
  }
  fail("Conflict", "another device keeps writing; try again");
}

async function syncOnce(o: Open, id: string, api: Api): Promise<boolean> {
  const stored = (await getAccount())!;
  const sv = stored.vaults[id];
  const vault = o.vaults.get(id);
  if (!sv || !vault) return true;
  const r = await api.get<{ epoch: number; seq: number; manifest: string | null; records: string[] }>(
    `/v1/vaults/${id}/sync?since=${sv.seq}`,
  );
  if (r.epoch > vault.epoch()) {
    // Someone removed a member and moved the vault to a new key.
    const grant = await newestGrant(api, id);
    const granter = toB64((await loadCore()).grantGranter(fromB64(grant)));
    if (!trustedKey(o, stored, granter)) fail("BadSignature", "new vault key from someone not verified");
    o.account.acceptRotation(vault, fromB64(grant), fromB64(granter));
    sv.grant = grant;
    sv.granter = granter === toB64(o.me) ? "" : granter;
  }
  if (r.manifest) {
    const ups = vault.sync(fromB64(r.manifest), r.records.map(fromB64), writers(o, stored));
    for (const s of ups) {
      if (canWrite(sv.role)) sv.pending[s.id] = toB64(s.record);
      s.free();
    }
  }
  sv.cache = toB64(vault.exportCache());
  const pending = Object.values(sv.pending);
  if (pending.length === 0 || !canWrite(sv.role)) {
    sv.pending = {};
    sv.seq = r.seq;
    await putAccount(stored);
    return true;
  }
  await putAccount(stored);
  const manifest = vault.signManifest(o.account);
  const done = await api.post<{ seq: number }>(`/v1/vaults/${id}/commit`, {
    manifest: toB64(manifest),
    records: pending,
  });
  vault.commitAccepted();
  sv.seq = done.seq;
  sv.pending = {};
  sv.cache = toB64(vault.exportCache());
  await putAccount(stored);
  return true;
}

/**
 * New vault key after a member left or was removed, so they cannot read
 * anything written from now on. Every remaining member must have been
 * verified on this device, since each gets the new key.
 */
async function rotate(o: Open, id: string, api: Api) {
  const stored = (await getAccount())!;
  const sv = stored.vaults[id] ?? fail("NotFound");
  const vault = o.vaults.get(id) ?? fail("NotFound");
  const { members } = await api.get<{ members: MemberRow[] }>(`/v1/vaults/${id}/members`);
  const keys = members
    .filter((m) => m.active)
    .map((m) => (m.account_id === stored.accountId ? toB64(o.me) : stored.contacts[m.account_id]?.keys === m.public_keys ? m.public_keys : undefined));
  if (keys.some((k) => k === undefined)) fail("Unverified", "compare safety numbers with every member first");
  const records = vault.rotateKey();
  try {
    const grants = keys.map((k) => toB64(o.account.grantVault(vault, fromB64(k!))));
    const manifest = vault.signManifest(o.account);
    const r = await api.post<{ epoch: number; seq: number }>(`/v1/vaults/${id}/rotate`, {
      manifest: toB64(manifest),
      records: records.map((s: { record: Uint8Array }) => toB64(s.record)),
      grants,
    });
    vault.commitAccepted();
    sv.grant = grants[keys.indexOf(toB64(o.me))]!;
    sv.granter = "";
    sv.seq = r.seq;
    sv.pending = {};
    sv.cache = toB64(vault.exportCache());
    await putAccount(stored);
  } catch (e) {
    // Not accepted: go back to the old key, which the server still has.
    vault.free();
    o.vaults.delete(id);
    try {
      o.vaults.set(id, o.account.openVault(fromB64(sv.grant), granterOf(o, sv), sv.cache ? fromB64(sv.cache) : undefined));
    } catch {
      // Opened again from the server on the next sync.
      delete stored.vaults[id];
      await putAccount(stored);
    }
    throw e;
  } finally {
    for (const s of records) s.free();
  }
}

// ---------------------------------------------------------------------------
// Sharing
// ---------------------------------------------------------------------------

async function apiFor(): Promise<{ stored: StoredAccount; api: Api; o: Open }> {
  const o = await need();
  const stored = (await getAccount())!;
  return { o, stored, api: new Api(stored.server, stored.token) };
}

export async function sharing(): Promise<SharingView> {
  const { o, stored, api } = await apiFor();
  const core = await loadCore();
  const { invites } = await api.get<{
    invites: { invite_id: string; vault_id: string; role: Role; from: { account_id: string; email: string; public_keys: string } }[];
  }>("/v1/invites");
  const vaults: VaultView[] = [];
  for (const [id, v] of o.vaults) {
    if (id === stored.personalId) continue;
    const info = vaultInfo(v, false);
    vaults.push({ id, name: info.name, kind: info.kind, role: stored.vaults[id]!.role, items: (v.items(false) as ItemView[]).filter((i) => !isInfo(i)).length });
  }
  return {
    vaults,
    invites: invites.map((i) => ({
      inviteId: i.invite_id,
      vaultId: i.vault_id,
      role: i.role,
      fromEmail: i.from.email,
      fromFingerprint: core.fingerprint(fromB64(i.from.public_keys)),
    })),
    waiting,
    myFingerprint: o.account.fingerprint(),
  };
}

export async function createVault(name: string, kind: "family" | "team") {
  const { o, stored, api } = await apiFor();
  const nv = o.account.createVault();
  const grant = toB64(nv.grant);
  await api.post("/v1/vaults", { grant });
  const vault = nv.vault;
  const id = vault.id();
  o.vaults.set(id, vault);
  const info = vault.createItem("SecureNote", [
    { name: "title", value: name.trim() || (kind === "team" ? "Team" : "Family") },
    { name: VAULT_INFO, value: kind },
  ]);
  stored.vaults[id] = { id, role: "owner", grant, granter: "", cache: toB64(vault.exportCache()), seq: 0, pending: { [info.id]: toB64(info.record) } };
  info.free();
  await putAccount(stored);
  void sync().catch(() => undefined);
  return id;
}

export async function members(vaultId: string): Promise<MembersView> {
  const { o, stored, api } = await apiFor();
  const core = await loadCore();
  const sv = stored.vaults[vaultId] ?? fail("NotFound");
  const { members } = await api.get<{ members: MemberRow[] }>(`/v1/vaults/${vaultId}/members`);
  const invites = canManage(sv.role)
    ? (await api.get<{ invites: { invite_id: string; email: string; role: Role }[] }>(`/v1/vaults/${vaultId}/invites`)).invites
    : [];
  return {
    members: members.map((m) => ({
      accountId: m.account_id,
      email: m.email,
      role: m.role,
      active: m.active,
      fingerprint: core.fingerprint(fromB64(m.public_keys)),
      verified: trustedKey(o, stored, m.public_keys),
      me: m.account_id === stored.accountId,
    })),
    invites: invites.map((i) => ({ inviteId: i.invite_id, email: i.email, role: i.role })),
  };
}

export async function invite(vaultId: string, email: string, role: Role) {
  const { api } = await apiFor();
  if (!["manager", "writer", "reader"].includes(role)) fail("BadRole");
  await api.post(`/v1/vaults/${vaultId}/invites`, { email: email.trim(), role });
}

export async function cancelInvite(inviteId: string) {
  const { api } = await apiFor();
  await api.del(`/v1/invites/${inviteId}`);
}

export async function declineInvite(inviteId: string) {
  await cancelInvite(inviteId);
}

/**
 * Join a shared vault. `fingerprint` is the safety number the person saw and
 * compared; the inviter's keys are trusted only if they still match it.
 */
export async function acceptInvite(inviteId: string, fingerprint: string) {
  const { stored, api } = await apiFor();
  const core = await loadCore();
  const { invites } = await api.get<{
    invites: { invite_id: string; from: { account_id: string; email: string; public_keys: string } }[];
  }>("/v1/invites");
  const inv = invites.find((i) => i.invite_id === inviteId) ?? fail("NotFound");
  if (core.fingerprint(fromB64(inv.from.public_keys)) !== fingerprint) fail("Fingerprint", "the safety number changed");
  stored.contacts[inv.from.account_id] = { email: inv.from.email, keys: inv.from.public_keys };
  await putAccount(stored);
  await api.post(`/v1/invites/${inviteId}/accept`, {});
}

/**
 * Mark a member as verified after comparing safety numbers, and give them
 * the vault key if they don't have it yet (owners and managers).
 */
export async function verify(vaultId: string, accountId: string, fingerprint: string) {
  const { o, stored, api } = await apiFor();
  const core = await loadCore();
  const { members } = await api.get<{ members: MemberRow[] }>(`/v1/vaults/${vaultId}/members`);
  const m = members.find((x) => x.account_id === accountId) ?? fail("NotFound");
  if (core.fingerprint(fromB64(m.public_keys)) !== fingerprint) fail("Fingerprint", "the safety number changed");
  stored.contacts[accountId] = { email: m.email, keys: m.public_keys };
  await putAccount(stored);
  const sv = stored.vaults[vaultId];
  const vault = o.vaults.get(vaultId);
  if (sv && vault && !m.active && canManage(sv.role)) {
    const grant = o.account.grantVault(vault, fromB64(m.public_keys));
    await api.put(`/v1/vaults/${vaultId}/members/${accountId}/grant`, { grant: toB64(grant) });
  }
  // A vault waiting for this person's key can open now.
  void sync().catch(() => undefined);
}

export async function setRole(vaultId: string, accountId: string, role: Role) {
  const { api } = await apiFor();
  await api.put(`/v1/vaults/${vaultId}/members/${accountId}/role`, { role });
}

/** Remove someone, then move the vault to a new key at once. */
export async function removeMember(vaultId: string, accountId: string) {
  const { o, api } = await apiFor();
  await api.del(`/v1/vaults/${vaultId}/members/${accountId}`);
  await syncVault(o, vaultId, api);
  await rotate(o, vaultId, api);
}

export async function leaveVault(vaultId: string) {
  const { o, stored, api } = await apiFor();
  if (vaultId === stored.personalId) fail("NotAllowed");
  await api.del(`/v1/vaults/${vaultId}/members/${stored.accountId}`);
  o.vaults.get(vaultId)?.free();
  o.vaults.delete(vaultId);
  delete stored.vaults[vaultId];
  await putAccount(stored);
}

export async function deleteVault(vaultId: string) {
  const { o, stored, api } = await apiFor();
  if (vaultId === stored.personalId) fail("NotAllowed");
  await api.del(`/v1/vaults/${vaultId}`);
  o.vaults.get(vaultId)?.free();
  o.vaults.delete(vaultId);
  delete stored.vaults[vaultId];
  await putAccount(stored);
}
