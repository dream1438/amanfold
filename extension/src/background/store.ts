// What the extension keeps in the browser.
//
// storage.local (on disk): the account bundle (keys wrapped under the master
// password + Secret Key), the Secret Key, the vault grant, the encrypted vault
// cache and records not yet uploaded. None of it opens without the master
// password, except with the device key below.
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

export async function getSettings(): Promise<Settings> {
  const { settings } = await chrome.storage.local.get("settings");
  return { ...defaults, ...(settings ?? {}) };
}

export async function setSettings(s: Partial<Settings>): Promise<Settings> {
  const next = { ...(await getSettings()), ...s };
  if (![1, 5, 15, 60, 240, 0].includes(next.autoLockMinutes)) next.autoLockMinutes = 15;
  if (next.lang !== null && next.lang !== "en" && next.lang !== "ar") next.lang = null;
  await chrome.storage.local.set({ settings: next });
  return next;
}

export async function getAccount(): Promise<StoredAccount | undefined> {
  const { account } = await chrome.storage.local.get("account");
  return account as StoredAccount | undefined;
}

export async function putAccount(a: StoredAccount): Promise<void> {
  await chrome.storage.local.set({ account: a });
}

export async function clearAccount(): Promise<void> {
  await chrome.storage.local.remove("account");
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
