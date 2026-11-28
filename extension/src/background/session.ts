// The unlocked account and vault, held only in the service worker's memory.
//
// Every operation runs through `serial` (the message router wraps each
// request), so a sync never interleaves with an edit or a lock. Functions
// here assume they already run inside it; `sync()` queues a new round. When the browser stops the service worker, the objects are gone; the
// next request reopens them from the device key in session storage (no
// password needed until the person locks or the auto-lock fires).

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
} from "./store.ts";
import type { FieldInput, ItemKind, ItemView } from "../shared/types.ts";

/** Pinned OPAQUE public keys of servers this build trusts without asking. */
const PINNED: Record<string, string> = {};

interface Open {
  account: Account;
  vault: LocalVault;
  me: Uint8Array;
}

let open: Open | undefined;
let kit: { secretKey: string; recoveryWords: string; email: string } | undefined;
let syncError: string | undefined;
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
  const browser = /Firefox\//.test(ua) ? "Firefox" : /Edg\//.test(ua) ? "Edge" : "Chrome";
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

// ---------------------------------------------------------------------------
// Locking
// ---------------------------------------------------------------------------

async function touch() {
  const q = await getQuick();
  if (q) await putQuick({ ...q, lastActiveMs: Date.now() });
}

async function rememberUnlock(account: Account) {
  const q = account.enableQuickUnlock();
  await putQuick({ deviceKey: toB64(q.deviceKey), wrappedKeys: toB64(q.wrappedKeys), lastActiveMs: Date.now() });
  q.free();
}

/**
 * The open vault, reopening it after a service worker restart. Reading the
 * vault is not activity: only `active()` and `need()`, which serve requests
 * the person made, push the auto-lock deadline back. Alarms, badges, tab
 * events and pages must never keep the vault open.
 */
export async function unlocked(): Promise<Open | undefined> {
  if (open) {
    const [quick, settings] = await Promise.all([getQuick(), getSettings()]);
    if (!quick || expired(quick.lastActiveMs, settings.autoLockMinutes)) {
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
    open = openVault(account, stored);
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
  if (quick && expired(quick.lastActiveMs, settings.autoLockMinutes)) await lock();
}

export async function lock() {
  if (open) {
    open.vault.free();
    open.account.free();
  }
  open = undefined;
  // The recovery kit opens the account: never show it once locked.
  kit = undefined;
  await putQuick(undefined);
}

export async function need(): Promise<Open> {
  return (await active()) ?? fail("Locked", "the vault is locked");
}

function openVault(account: Account, s: StoredAccount): Open {
  const me = account.publicKeys();
  const vault = account.openVault(fromB64(s.grant), me, s.cache ? fromB64(s.cache) : undefined);
  return { account, vault, me };
}

export async function unlock(password: string) {
  const stored = (await getAccount()) ?? fail("NoAccount");
  const core = await loadCore();
  const account = core.unlockAccount(fromB64(stored.bundle), password, stored.secretKey);
  await lock();
  open = openVault(account, stored);
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
  const stored: StoredAccount = {
    server: api.base,
    email,
    accountId: fin.account_id,
    bundle: toB64(bundle),
    secretKey: created.secretKey,
    serverKey,
    token: fin.api_token,
    vaultId: vault.id(),
    grant: toB64(grant),
    cache: toB64(vault.exportCache()),
    seq: 0,
    pending: {},
  };
  const newKit = { secretKey: created.secretKey, recoveryWords: created.recoveryWords, email };
  created.free();
  await putAccount(stored);
  await lock();
  kit = newKit;
  open = { account, vault, me: account.publicKeys() };
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

  const { vaults } = await api.get<{ vaults: { vault_id: string; role: string }[] }>("/v1/vaults");
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
    vaultId: mine.vault_id,
    grant,
    seq: 0,
    pending: {},
  };
  await lock();
  open = openVault(account, stored);
  await putAccount({ ...stored, cache: toB64(open.vault.exportCache()) });
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
// Items and sync
// ---------------------------------------------------------------------------

export async function items(trashed = false): Promise<ItemView[]> {
  return (await need()).vault.items(trashed);
}

async function saveLocal(id: string, record: Uint8Array) {
  const { vault } = await need();
  const stored = (await getAccount())!;
  stored.pending[id] = toB64(record);
  stored.cache = toB64(vault.exportCache());
  await putAccount(stored);
}

export async function create(kind: ItemKind, fields: FieldInput[]): Promise<string> {
  const { vault } = await need();
  const saved = vault.createItem(kind, fields);
  const id = saved.id;
  await saveLocal(id, saved.record);
  saved.free();
  void sync().catch(() => undefined);
  return id;
}

export async function update(id: string, set: FieldInput[], remove: string[]) {
  const { vault } = await need();
  const saved = vault.updateItem(id, set, remove);
  await saveLocal(id, saved.record);
  saved.free();
  void sync().catch(() => undefined);
}

export async function trash(id: string, trashed: boolean) {
  const { vault } = await need();
  const saved = vault.setTrashed(id, trashed);
  await saveLocal(id, saved.record);
  saved.free();
  void sync().catch(() => undefined);
}

/** Sync in the background, after the current operation. */
export function sync(): Promise<void> {
  return serial(syncNow);
}

/** One round of the sync loop in docs/SERVER.md, retried on a conflict. */
export async function syncNow(): Promise<void> {
  {
    for (let attempt = 0; attempt < 4; attempt++) {
      try {
        if (await syncOnce()) {
          syncError = undefined;
          return;
        }
      } catch (e) {
        const err = e as Error;
        if (err instanceof ApiError && err.status === 409) continue;
        if (err instanceof ApiError && err.status === 401) syncError = "signedOut";
        else if (err.name === "Rollback" || err.name === "BadSignature") syncError = "rollback";
        else if (err instanceof ApiError && err.kind === "offline") syncError = "offline";
        else syncError = err.name || "error";
        throw err;
      }
    }
    syncError = "conflict";
  }
}

async function syncOnce(): Promise<boolean> {
  const o = await unlocked();
  if (!o) return true;
  const stored = (await getAccount())!;
  const api = new Api(stored.server, stored.token);
  const r = await api.get<{ epoch: number; seq: number; manifest: string | null; records: string[] }>(
    `/v1/vaults/${stored.vaultId}/sync?since=${stored.seq}`,
  );
  if (r.epoch > o.vault.epoch()) {
    const grant = await newestGrant(api, stored.vaultId);
    o.account.acceptRotation(o.vault, fromB64(grant), o.me);
    stored.grant = grant;
  }
  if (r.manifest) {
    const ups = o.vault.sync(fromB64(r.manifest), r.records.map(fromB64), [o.me]);
    for (const s of ups) {
      stored.pending[s.id] = toB64(s.record);
      s.free();
    }
  }
  stored.cache = toB64(o.vault.exportCache());
  const pending = Object.values(stored.pending);
  if (pending.length === 0) {
    stored.seq = r.seq;
    stored.lastSyncMs = Date.now();
    await putAccount(stored);
    return true;
  }
  await putAccount(stored);
  const manifest = o.vault.signManifest(o.account);
  const done = await api.post<{ seq: number }>(`/v1/vaults/${stored.vaultId}/commit`, {
    manifest: toB64(manifest),
    records: pending,
  });
  o.vault.commitAccepted();
  stored.seq = done.seq;
  stored.pending = {};
  stored.cache = toB64(o.vault.exportCache());
  stored.lastSyncMs = Date.now();
  await putAccount(stored);
  return true;
}
