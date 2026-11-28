// Background service worker: the only place the vault is open.
//
// Who may ask for what:
// * Extension pages (the popup) may do everything. They are recognised by
//   their URL, which the browser sets and pages cannot forge.
// * Content scripts may only report a submitted login form (for the save
//   prompt). They can never ask for a password, a card or a list of items.
// * Web pages cannot reach the extension at all (no externally_connectable).

import { loadCore } from "./core.ts";
import { fillTab, itemUrls } from "./fill.ts";
import * as s from "./session.ts";
import { getAccount, getSettings, setSettings } from "./store.ts";
import type { ContentEvent, ItemView, PendingSave, Reply, Req, Status, TabInfo } from "../shared/types.ts";

const EXT = chrome.runtime.getURL("");


// ---------------------------------------------------------------------------
// Save prompts: kept in memory only, per tab, for a few minutes.
// ---------------------------------------------------------------------------

interface Captured extends PendingSave {
  password: string;
  url: string;
  at: number;
}
const captured = new Map<number, Captured>();
const SAVE_TTL_MS = 5 * 60_000;

function pendingFor(tabId: number): Captured | undefined {
  const c = captured.get(tabId);
  if (c && Date.now() - c.at > SAVE_TTL_MS) {
    captured.delete(tabId);
    return undefined;
  }
  return c;
}

async function onSubmitted(ev: ContentEvent, sender: chrome.runtime.MessageSender) {
  const tabId = sender.tab?.id;
  const url = sender.url;
  if (tabId === undefined || !url || !ev.password || ev.password.length > 1024 || ev.username.length > 512) return;
  const core = await loadCore();
  const page = core.pageSite(url);
  if (!page) return;
  // Only the top page, or a frame on the same site as the top page.
  const topUrl = sender.tab?.url;
  if (sender.frameId !== 0 && (!topUrl || core.pageSite(topUrl)?.site !== page.site)) return;
  const open = await s.unlocked();
  let updateId: string | undefined;
  if (open) {
    const matches = s.loginsFor(open, url).filter((m) => !m.item.vault?.readOnly);
    const same = matches.find(
      (m) => (m.item.fields.find((f) => f.name === "username")?.value ?? "") === ev.username,
    );
    if (same) {
      if (same.item.fields.find((f) => f.name === "password")?.value === ev.password) return;
      updateId = same.item.id;
    }
  }
  captured.set(tabId, {
    site: page.site,
    displayHost: page.displayHost,
    username: ev.username,
    password: ev.password,
    url,
    updateId,
    at: Date.now(),
  });
  await badge(tabId);
}

// ---------------------------------------------------------------------------
// Toolbar badge: how many saved logins match this tab, "!" for a look-alike.
// ---------------------------------------------------------------------------

async function badge(tabId: number) {
  try {
    const tab = await chrome.tabs.get(tabId);
    const open = tab.url ? await s.unlocked() : undefined;
    let text = "";
    let color = "#0b6e4f";
    if (open && tab.url) {
      const n = s.loginsFor(open, tab.url).length;
      if (s.lookalikesFor(open, tab.url).length > 0) {
        text = "!";
        color = "#b3261e";
      } else if (pendingFor(tabId)) {
        text = "+";
      } else if (n > 0) {
        text = String(n);
      }
    }
    await chrome.action.setBadgeText({ tabId, text });
    await chrome.action.setBadgeBackgroundColor({ tabId, color });
  } catch {
    /* tab closed */
  }
}

// Through the queue, so a badge never reads a vault that is being locked.
chrome.tabs.onActivated.addListener(({ tabId }) => void s.serial(() => badge(tabId)));
chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (info.status === "complete" || info.url) void s.serial(() => badge(tabId));
});
chrome.tabs.onRemoved.addListener((tabId) => captured.delete(tabId));

// ---------------------------------------------------------------------------
// Auto-lock and background sync
// ---------------------------------------------------------------------------

chrome.alarms.create("tick", { periodInMinutes: 1 });
chrome.alarms.onAlarm.addListener((a) => {
  if (a.name !== "tick") return;
  void s.serial(async () => {
    await s.autoLockCheck();
    const minute = new Date().getMinutes();
    if (minute % 5 === 0 && (await s.unlocked())) await s.syncNow().catch(() => undefined);
  });
});

// ---------------------------------------------------------------------------
// Keyboard shortcut: fill the one login for this page, else open the popup.
// ---------------------------------------------------------------------------

chrome.commands.onCommand.addListener((command, tab) => {
  if (command !== "fill-login") return;
  void s.serial(async () => {
    const t = tab?.id !== undefined ? tab : (await chrome.tabs.query({ active: true, currentWindow: true }))[0];
    if (t?.id === undefined || !t.url) return;
    const open = await s.unlocked();
    const matches = open ? s.loginsFor(open, t.url) : [];
    const best = matches.filter((m) => m.strength === matches[0]?.strength);
    if (open && best.length === 1 && best[0]) {
      await fillTab(t.id, best[0].item, { lookalikes: (u) => s.lookalikesFor(open, u) });
    } else {
      await chrome.action.openPopup?.().catch(() => undefined);
    }
  });
});

// ---------------------------------------------------------------------------
// Requests from the popup
// ---------------------------------------------------------------------------

async function status(): Promise<Status> {
  const [stored, settings] = await Promise.all([getAccount(), getSettings()]);
  const open = stored ? await s.unlocked() : undefined;
  return {
    state: !stored ? "new" : open ? "unlocked" : "locked",
    email: stored?.email,
    server: stored?.server,
    settings,
    kit: s.currentKit(),
    lastSyncMs: stored?.lastSyncMs,
    syncError: s.lastSyncError(),
    version: __AMANFOLD_VERSION__,
  };
}

async function tabInfo(tabId: number): Promise<TabInfo> {
  const tab = await chrome.tabs.get(tabId);
  const url = tab.url ?? "";
  const core = await loadCore();
  const open = await s.need();
  const p = pendingFor(tabId);
  return {
    url,
    site: core.pageSite(url),
    matches: s.loginsFor(open, url),
    lookalikes: s.lookalikesFor(open, url),
    pendingSave: p && { site: p.site, displayHost: p.displayHost, username: p.username, updateId: p.updateId },
  };
}

async function handle(req: Req): Promise<unknown> {
  switch (req.type) {
    case "status":
      return status();
    case "serverInfo":
      return s.serverInfo(req.server);
    case "signupEmail":
      return s.signupEmail(req.server, req.email);
    case "signup":
      await setSettings({ server: req.server });
      return s.signup(req.server, req.email, req.password, req.code);
    case "kitSaved":
      return s.kitSaved();
    case "signin":
      await setSettings({ server: req.server });
      return s.signin(req.server, req.email, req.password, req.secretKey);
    case "unlock":
      return s.unlock(req.password);
    case "lock":
      return s.lock();
    case "signout":
      captured.clear();
      return s.signout();
    case "sync":
      return s.syncNow();
    case "items":
      return s.items(req.trashed);
    case "item":
      return (await s.item(req.id)) ?? null;
    case "tabInfo":
      return tabInfo(req.tabId);
    case "create":
      return s.create(req.kind, req.fields, req.vaultId);
    case "update":
      return s.update(req.id, req.set, req.remove);
    case "trash":
      return s.trash(req.id, req.trashed);
    case "fill": {
      const open = await s.need();
      const item = (await s.item(req.itemId)) ?? fail("NotFound");
      if (item.trashed) fail("NotFound");
      const r = await fillTab(req.tabId, item, {
        confirmFrames: req.confirmFrames,
        anySite: req.anySite,
        lookalikes: (u) => s.lookalikesFor(open, u),
      });
      return r;
    }
    case "pendingSave": {
      const p = pendingFor(req.tabId);
      captured.delete(req.tabId);
      if (p && req.action === "save") {
        if (p.updateId) {
          await s.update(p.updateId, [{ name: "password", value: p.password }], []);
        } else {
          const fields = [
            { name: "title", value: p.displayHost },
            { name: "url", value: new URL(p.url).origin },
            { name: "password", value: p.password },
          ];
          if (p.username) fields.push({ name: "username", value: p.username });
          await s.create("Login", fields);
        }
      }
      await badge(req.tabId);
      return;
    }
    case "totp":
      return (await loadCore()).totpNow(req.secret);
    case "generate": {
      const core = await loadCore();
      if (req.words) return core.generatePassphrase(req.words, "-");
      return core.generatePassword(req.rules ?? core.defaultPasswordRules());
    }
    case "cardInfo":
      return (await loadCore()).cardInfo(req.number);
    case "settings":
      return setSettings(req.set);
    case "sharing":
      return s.sharing();
    case "createVault":
      return s.createVault(req.name, req.kind);
    case "members":
      return s.members(req.vaultId);
    case "invite":
      return s.invite(req.vaultId, req.email, req.role);
    case "cancelInvite":
      return s.cancelInvite(req.inviteId);
    case "acceptInvite":
      return s.acceptInvite(req.inviteId, req.fingerprint);
    case "declineInvite":
      return s.declineInvite(req.inviteId);
    case "verify":
      return s.verify(req.vaultId, req.accountId, req.fingerprint);
    case "setRole":
      return s.setRole(req.vaultId, req.accountId, req.role);
    case "removeMember":
      return s.removeMember(req.vaultId, req.accountId);
    case "leaveVault":
      return s.leaveVault(req.vaultId);
    case "deleteVault":
      return s.deleteVault(req.vaultId);
  }
}

function fail(name: string): never {
  throw Object.assign(new Error(name), { name });
}

// Every request runs in the session queue, so each sees a settled state.
chrome.runtime.onMessage.addListener((msg: unknown, sender, sendResponse) => {
  if (sender.id !== chrome.runtime.id) return false;
  const fromExtensionPage = typeof sender.url === "string" && sender.url.startsWith(EXT);
  if (!fromExtensionPage) {
    // A content script. The only thing it may say is "a login form was submitted".
    const ev = msg as ContentEvent;
    if (ev && ev.type === "submitted" && typeof ev.username === "string" && typeof ev.password === "string") {
      void s.serial(() => onSubmitted(ev, sender));
    }
    return false;
  }
  const req = msg as Req;
  s.serial(() => handle(req))
    .then((value) => sendResponse({ ok: true, value: value ?? null } satisfies Reply))
    .catch((e: Error) =>
      sendResponse({ ok: false, error: { name: e?.name ?? "Error", message: e?.message ?? String(e) } } satisfies Reply),
    );
  return true;
});

// Refresh the badge when the vault opens or locks elsewhere.
chrome.storage.session.onChanged?.addListener(() => {
  void chrome.tabs
    .query({ active: true })
    .then((tabs) => tabs.forEach((t) => t.id !== undefined && void s.serial(() => badge(t.id!))));
});
