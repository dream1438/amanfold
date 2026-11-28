// Messages between the popup, the background service worker and the
// content scripts. Only the background holds the vault; content scripts get
// values only for a fill the person started, and only in the frame chosen.

export type ItemKind = "Login" | "Card" | "SecureNote" | "Identity" | "Passkey";
export type Strength = "None" | "Site" | "Host" | "Origin";

export interface FieldView {
  name: string;
  value: string;
  concealed: boolean;
}

export interface ItemView {
  id: string;
  kind: ItemKind;
  fields: FieldView[];
  trashed: boolean;
  createdMs: number;
  modifiedMs: number;
}

export interface LoginMatch {
  item: ItemView;
  strength: Strength;
}

export interface SiteView {
  secure: boolean;
  host: string;
  displayHost: string;
  site: string;
}

export interface FieldInput {
  name: string;
  value: string;
}

export interface TotpCode {
  code: string;
  secondsRemaining: number;
  period: number;
}

export interface PasswordRules {
  length: number;
  lowercase: boolean;
  uppercase: boolean;
  digits: boolean;
  symbols: boolean;
  avoidAmbiguous: boolean;
}

export type Lang = "en" | "ar";

export interface Settings {
  /** null: follow the browser's language. */
  lang: Lang | null;
  autoLockMinutes: number;
  /** Server for the next sign-up or sign-in. */
  server: string;
}

export interface Status {
  state: "new" | "locked" | "unlocked";
  email?: string;
  server?: string;
  settings: Settings;
  /** Shown once, right after sign-up, until the person confirms. */
  kit?: { secretKey: string; recoveryWords: string; email: string };
  lastSyncMs?: number;
  /** "rollback" means the server sent old or incomplete data. */
  syncError?: string;
  version: string;
}

export interface PendingSave {
  site: string;
  displayHost: string;
  username: string;
  /** Existing login to update, when only the password changed. */
  updateId?: string;
  /** Names this capture; a save is refused if the page replaced it since. */
  nonce: string;
}

export interface TabInfo {
  url: string;
  site?: SiteView;
  matches: LoginMatch[];
  lookalikes: string[];
  pendingSave?: PendingSave;
}

export interface FillResult {
  filled: number;
  /** Fields found but not filled because they were hidden or covered. */
  skippedHidden: number;
  /** Frames from other sites that hold card fields and need a yes first. */
  needsConfirm?: { frameId: number; host: string }[];
  /** Fields in frames from another site, never filled for logins. */
  blockedFrames?: string[];
}

export type Req =
  | { type: "status" }
  | { type: "serverInfo"; server: string }
  | { type: "signupEmail"; server: string; email: string }
  | { type: "signup"; server: string; email: string; password: string; code?: string }
  | { type: "kitSaved" }
  | { type: "signin"; server: string; email: string; password: string; secretKey: string }
  | { type: "unlock"; password: string }
  | { type: "lock" }
  | { type: "signout" }
  | { type: "sync" }
  | { type: "items"; trashed?: boolean }
  | { type: "item"; id: string }
  | { type: "tabInfo"; tabId: number }
  | { type: "create"; kind: ItemKind; fields: FieldInput[] }
  | { type: "update"; id: string; set: FieldInput[]; remove: string[] }
  | { type: "trash"; id: string; trashed: boolean }
  | { type: "fill"; tabId: number; itemId: string; confirmFrames?: number[]; anySite?: boolean }
  | { type: "pendingSave"; tabId: number; action: "save" | "dismiss"; nonce: string }
  | { type: "totp"; secret: string }
  | { type: "generate"; rules?: PasswordRules; words?: number }
  | { type: "cardInfo"; number: string }
  | { type: "settings"; set: Partial<Settings> };

export type Reply<T = unknown> = { ok: true; value: T } | { ok: false; error: { name: string; message: string } };

// Background → content script.

export interface ScanResult {
  login: boolean;
  card: boolean;
  otp: boolean;
}

export interface CardValues {
  number?: string;
  name?: string;
  expMonth?: string;
  expYear?: string;
  cvv?: string;
}

export interface FillValues {
  username?: string;
  password?: string;
  totp?: string;
  card?: CardValues;
}

export type ContentReq =
  | { type: "scan" }
  | { type: "fill"; origin: string; values: FillValues };

// Content script → background.

export type ContentEvent = { type: "submitted"; username: string; password: string };
