// What the extension keeps in the browser.
//
// IndexedDB of the extension origin (on disk): settings, the account bundle
// (keys wrapped under the master password + Secret Key), the Secret Key, the
// vault grant, the encrypted vault cache and records not yet uploaded. None of
// it opens without the master password, except with the device key below.
// Not storage.local: content scripts (and so a compromised page renderer) can
// read and write storage.local, but not the extension origin's IndexedDB.
//
// storage.session (memory only, cleared when the browser quits, not readable
// by content scripts): the quick-unlock device key, so the vault survives the
// service worker being stopped. Locking deletes it.

import type { Settings } from "../shared/types.ts";

export interface StoredAccount {
  server: string;
  email: string;
  accountId: string;
  bundle: string;
  secretKey: string;
  /** Server OPAQUE public key, pinned at first contact. */
  serverKey: string;
  token: string;
  vaultId: string;
  /** Newest grant for the personal vault. */
  grant: string;
  cache?: string;
  seq: number;
  /** Encrypted records saved here but not yet accepted by the server. */
  pending: Record<string, string>;
  lastSyncMs?: number;
}

export interface QuickUnlockState {
  deviceKey: string;
  wrappedKeys: string;
  lastActiveMs: number;
}

export const DEFAULT_SERVER = __AMANFOLD_SERVER__;

declare global {
  const __AMANFOLD_SERVER__: string;
  const __AMANFOLD_VERSION__: string;
}

const defaults: Settings = { lang: null, autoLockMinutes: 15, server: DEFAULT_SERVER };

// ---------------------------------------------------------------------------
// Key-value store in the extension origin's IndexedDB.
// ---------------------------------------------------------------------------

let dbp: Promise<IDBDatabase> | undefined;

function db(): Promise<IDBDatabase> {
  dbp ??= new Promise<IDBDatabase>((resolve, reject) => {
    const r = indexedDB.open("amanfold", 1);
    r.onupgradeneeded = () => r.result.createObjectStore("kv");
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  }).catch((e) => {
    dbp = undefined;
    throw e;
  });
  return dbp;
}

async function kv<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest): Promise<T> {
  const d = await db();
  return new Promise<T>((resolve, reject) => {
    const tx = d.transaction("kv", mode);
    const req = f(tx.objectStore("kv"));
    tx.oncomplete = () => resolve(req.result as T);
    tx.onerror = tx.onabort = () => reject(tx.error);
  });
}

const kvGet = <T>(key: string) => kv<T | undefined>("readonly", (s) => s.get(key));
const kvSet = (key: string, value: unknown) => kv<void>("readwrite", (s) => s.put(value, key));
const kvDel = (key: string) => kv<void>("readwrite", (s) => s.delete(key));

let migrated: Promise<void> | undefined;

/** Move data written by earlier versions out of storage.local, once. */
function migrate(): Promise<void> {
  migrated ??= (async () => {
    const old = await chrome.storage.local.get(["settings", "account"]);
    if (old.settings !== undefined && (await kvGet("settings")) === undefined) await kvSet("settings", old.settings);
    if (old.account !== undefined && (await kvGet("account")) === undefined) await kvSet("account", old.account);
    await chrome.storage.local.remove(["settings", "account"]);
  })().catch((e) => {
    migrated = undefined;
    throw e;
  });
  return migrated;
}

export async function getSettings(): Promise<Settings> {
  await migrate();
  const settings = await kvGet<Partial<Settings>>("settings");
  return { ...defaults, ...(settings ?? {}) };
}

export async function setSettings(s: Partial<Settings>): Promise<Settings> {
  const next = { ...(await getSettings()), ...s };
  if (![1, 5, 15, 60, 240, 0].includes(next.autoLockMinutes)) next.autoLockMinutes = 15;
  if (next.lang !== null && next.lang !== "en" && next.lang !== "ar") next.lang = null;
  await kvSet("settings", next);
  return next;
}

export async function getAccount(): Promise<StoredAccount | undefined> {
  await migrate();
  return kvGet<StoredAccount>("account");
}

export async function putAccount(a: StoredAccount): Promise<void> {
  await migrate();
  await kvSet("account", a);
}

export async function clearAccount(): Promise<void> {
  await migrate();
  await kvDel("account");
  await chrome.storage.session.clear();
}

export async function getQuick(): Promise<QuickUnlockState | undefined> {
  const { quick } = await chrome.storage.session.get("quick");
  return quick as QuickUnlockState | undefined;
}

export async function putQuick(q: QuickUnlockState | undefined): Promise<void> {
  if (q) await chrome.storage.session.set({ quick: q });
  else await chrome.storage.session.remove("quick");
}
