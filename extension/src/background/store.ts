// What the extension keeps in the browser.
//
// storage.local (on disk): the account bundle (keys wrapped under the master
// password + Secret Key), the Secret Key, each vault's grant, the encrypted
// vault caches, records not yet uploaded, and the public keys of people whose
// safety numbers were compared. None of it opens without the master
// password, except with the device key below.
//
// storage.session (memory only, cleared when the browser quits, not readable
// by content scripts): the quick-unlock device key, so the vault survives the
// service worker being stopped. Locking deletes it. Only where the browser
// confirms content scripts can't read it (see `sessionTrusted`); elsewhere,
// Safari included, nothing is put there.

import type { Role, Settings } from "../shared/types.ts";

export interface StoredAccount {
  server: string;
  email: string;
  accountId: string;
  bundle: string;
  secretKey: string;
  /** Server OPAQUE public key, pinned at first contact. */
  serverKey: string;
  token: string;
  /** The personal vault's id. */
  personalId: string;
  /** Every vault this browser has open: personal, family and team. */
  vaults: Record<string, StoredVault>;
  /**
   * People whose safety numbers were compared (by account id). Only they
   * may give this account a vault key or sign changes to a shared vault.
   */
  contacts: Record<string, Contact>;
  lastSyncMs?: number;
}

export interface StoredVault {
  id: string;
  role: Role;
  /** Newest grant for this vault, and the public keys of who signed it. */
  grant: string;
  granter: string;
  cache?: string;
  seq: number;
  /** Encrypted records saved here but not yet accepted by the server. */
  pending: Record<string, string>;
}

export interface Contact {
  email: string;
  /** Public keys whose safety number the person compared. */
  keys: string;
}

/** The shape before shared vaults (one vault). */
interface StoredAccountV1 {
  vaultId: string;
  grant: string;
  cache?: string;
  seq: number;
  pending: Record<string, string>;
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
  const __AMANFOLD_TARGET__: "chrome" | "firefox" | "safari";
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
  if (!account) return undefined;
  const a = account as StoredAccount & Partial<StoredAccountV1>;
  if (!a.vaults && a.vaultId && a.grant) {
    // Written by 0.1.0: only the personal vault, signed by the account itself.
    a.personalId = a.vaultId;
    a.vaults = {
      [a.vaultId]: { id: a.vaultId, role: "owner", grant: a.grant, granter: "", cache: a.cache, seq: a.seq ?? 0, pending: a.pending ?? {} },
    };
    a.contacts = {};
    for (const k of ["vaultId", "grant", "cache", "seq", "pending"] as const) delete a[k];
  }
  return a;
}

export async function putAccount(a: StoredAccount): Promise<void> {
  await chrome.storage.local.set({ account: a });
}

export async function clearAccount(): Promise<void> {
  await chrome.storage.local.remove("account");
  await chrome.storage.session.clear();
}

/**
 * Whether storage.session is out of reach of content scripts. Chrome: only
 * once setAccessLevel(TRUSTED_CONTEXTS) has succeeded (the default, made
 * explicit). Firefox never exposes it. Safari and anything else that can't
 * confirm it: false, and quick unlock is off, so the keys stay in the
 * background's memory only and the master password is asked again whenever
 * the browser unloads the background.
 */
export const sessionTrusted: Promise<boolean> = (async () => {
  if (__AMANFOLD_TARGET__ === "firefox") return true;
  if (__AMANFOLD_TARGET__ !== "chrome" || typeof chrome.storage.session?.setAccessLevel !== "function") return false;
  try {
    await chrome.storage.session.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" });
    return true;
  } catch {
    return false;
  }
})();

export async function getQuick(): Promise<QuickUnlockState | undefined> {
  if (!(await sessionTrusted)) return undefined;
  const { quick } = await chrome.storage.session.get("quick");
  return quick as QuickUnlockState | undefined;
}

export async function putQuick(q: QuickUnlockState | undefined): Promise<void> {
  if (q && !(await sessionTrusted)) return;
  if (q) await chrome.storage.session.set({ quick: q });
  else await chrome.storage.session.remove("quick");
}
