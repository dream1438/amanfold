// The popup: the only interface that shows vault contents. It asks the
// background for everything; it holds no keys.

import { ago, currentLang, dir, setLang, t } from "../shared/i18n.ts";
import type { MessageKey } from "../shared/locales.ts";
import type {
  FieldInput,
  FillResult,
  ItemKind,
  ItemView,
  PasswordRules,
  Reply,
  Req,
  Status,
  TabInfo,
  TotpCode,
} from "../shared/types.ts";

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

class CallError extends Error {}

async function call<T = unknown>(req: Req): Promise<T> {
  const r = (await chrome.runtime.sendMessage(req)) as Reply<T> | undefined;
  if (!r) throw Object.assign(new CallError("no answer"), { name: "NoAnswer" });
  if (r.ok) return r.value;
  throw Object.assign(new CallError(r.error.message), { name: r.error.name });
}

type Child = Node | string | false | null | undefined;
type Attrs = Record<string, string | number | boolean | ((e: Event) => void) | undefined>;

function h(tag: string, attrs: Attrs = {}, ...children: Child[]): HTMLElement {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === false) continue;
    if (typeof v === "function") el.addEventListener(k.replace(/^on/, ""), v);
    else if (k === "class") el.className = String(v);
    else if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, String(v));
  }
  for (const c of children) if (c !== false && c !== null && c !== undefined) el.append(c);
  return el;
}

const field = (i: ItemView, name: string) => i.fields.find((f) => f.name === name)?.value ?? "";

function itemTitle(i: ItemView): string {
  return field(i, "title") || field(i, "username") || hostOf(field(i, "url")) || t("untitled");
}

function hostOf(url: string): string {
  try {
    return new URL(url.includes("://") ? url : `https://${url}`).hostname;
  } catch {
    return url;
  }
}

function errorText(e: unknown): string {
  const err = e as Error;
  const name = err?.name ?? "";
  const map: Record<string, MessageKey> = {
    Decrypt: "errDecrypt",
    InvalidCode: "errSecretKey",
    BadServer: "errBadServer",
    Rollback: "errRollback",
    BadSignature: "errRollback",
    Locked: "errLocked",
    Lookalike: "errLookalike",
    NotThisSite: "notThisSite",
  };
  if (map[name]) return t(map[name]);
  if (name === "Local") return err.message;
  const msg = err?.message ?? "";
  if (name === "Server") {
    if (msg.startsWith("offline")) return t("errOffline");
    if (msg.startsWith("exists")) return t("errExists");
    if (/code/.test(msg)) return t("errCode");
    if (/^(verification_failed|unauthorized)/.test(msg)) return t("errVerification");
    if (msg.startsWith("rate_limited")) return t("errServer", msg);
    return t("errServer", msg);
  }
  return t("errGeneric", msg || name);
}

const app = document.getElementById("app")!;
let status: Status;
let tabId: number | undefined;
let tab: TabInfo | undefined;
let toastTimer: number | undefined;

function toast(text: string, kind: "ok" | "err" = "ok") {
  document.querySelector(".toast")?.remove();
  const el = h("div", { class: `toast ${kind}`, role: "status" }, text);
  document.body.append(el);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.remove(), 4000) as unknown as number;
}

function screen(...children: Child[]) {
  document.documentElement.lang = currentLang();
  document.documentElement.dir = dir();
  app.replaceChildren(...children.filter((c): c is Node | string => !!c));
  const first = app.querySelector<HTMLElement>("[autofocus]");
  first?.focus();
}

function header(title: string, opts: { back?: () => void; actions?: Child[] } = {}) {
  return h(
    "header",
    {},
    opts.back ? h("button", { class: "icon", "aria-label": t("back"), onclick: opts.back }, dir() === "rtl" ? "→" : "←") : h("span", { class: "logo", "aria-hidden": "true" }, "◆"),
    h("h1", {}, title),
    h("span", { class: "badge-beta", title: t("betaNote") }, t("beta")),
    ...(opts.actions ?? []),
  );
}

/** A form whose submit runs `fn` with a busy state and shows errors inline. */
function form(fn: (data: FormData) => Promise<void>, ...children: Child[]) {
  const err = h("p", { class: "error", role: "alert" });
  const f = h("form", {}, ...children, err) as HTMLFormElement;
  f.addEventListener("submit", async (e) => {
    e.preventDefault();
    err.textContent = "";
    const btn = f.querySelector<HTMLButtonElement>("button[type=submit]");
    const label = btn?.textContent;
    const data = new FormData(f); // before disabling: disabled fields are left out
    f.querySelectorAll("button, input").forEach((x) => ((x as HTMLInputElement).disabled = true));
    if (btn) btn.textContent = t("working");
    try {
      await fn(data);
    } catch (e2) {
      err.textContent = errorText(e2);
    } finally {
      if (f.isConnected) {
        f.querySelectorAll("button, input").forEach((x) => ((x as HTMLInputElement).disabled = false));
        if (btn && label) btn.textContent = btn.dataset.label ?? label;
      }
    }
  });
  return f;
}

function input(name: string, label: MessageKey, attrs: Attrs = {}) {
  const id = `f-${name}`;
  return h("label", { for: id }, h("span", {}, t(label)), h("input", { id, name, ...attrs }));
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text);
  toast(t("copied"));
  // Best effort: clear it again if the popup is still open in 30 s.
  setTimeout(() => {
    void navigator.clipboard.readText?.().then(
      (cur) => {
        if (cur === text) void navigator.clipboard.writeText("");
      },
      () => undefined,
    );
  }, 30_000);
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

async function refresh() {
  status = await call<Status>({ type: "status" });
  setLang(status.settings.lang);
  if (status.kit) return showKit();
  if (status.state === "new") return showWelcome();
  if (status.state === "locked") return showUnlock();
  return showHome();
}

async function main() {
  const param = new URLSearchParams(location.search).get("tab");
  if (param) tabId = Number(param);
  else tabId = (await chrome.tabs.query({ active: true, currentWindow: true }))[0]?.id;
  await refresh();
}

// ---------------------------------------------------------------------------
// Welcome, sign-up, recovery kit, sign-in, unlock
// ---------------------------------------------------------------------------

function serverLine(onChange: (s: string) => void) {
  const line = h("p", { class: "muted small" }, `${t("server")}: ${status.settings.server} `);
  line.append(
    h("button", {
      type: "button",
      class: "link",
      onclick: () => {
        const box = h("input", { name: "server", value: status.settings.server, dir: "ltr", "aria-label": t("server") }) as HTMLInputElement;
        line.replaceChildren(
          box,
          h("button", { type: "button", class: "link", onclick: () => onChange(box.value.trim()) }, t("save")),
        );
        box.focus();
      },
    }, t("change")),
  );
  return line;
}

function showWelcome() {
  screen(
    header(t("appName")),
    h(
      "main",
      { class: "center" },
      h("div", { class: "hero", "aria-hidden": "true" }, "◆"),
      h("p", { class: "lead" }, t("tagline")),
      h("button", { class: "primary wide", onclick: () => showSignup() }, t("createAccount")),
      h("button", { class: "wide", onclick: () => showSignin() }, t("signIn")),
      serverLine(async (server) => {
        try {
          await call({ type: "settings", set: { server } });
          await refresh();
        } catch (e) {
          toast(errorText(e), "err");
        }
      }),
      h("p", { class: "muted small" }, t("betaNote")),
    ),
  );
}

async function showSignup() {
  const server = status.settings.server;
  let needsCode = false;
  try {
    needsCode = (await call<{ email_verification: boolean }>({ type: "serverInfo", server })).email_verification;
  } catch (e) {
    toast(errorText(e), "err");
    return;
  }
  let codeSentTo = "";
  const codeBox = h("div");
  const f = form(
    async (d) => {
      const email = String(d.get("email")).trim();
      const pw = String(d.get("password"));
      if (needsCode && !codeSentTo) {
        await call({ type: "signupEmail", server, email });
        codeSentTo = email;
        codeBox.replaceChildren(
          h("p", { class: "muted small" }, t("codeSent", email)),
          input("code", "emailCode", { inputmode: "numeric", autocomplete: "one-time-code", required: true, dir: "ltr" }),
        );
        (f.querySelector("button[type=submit]") as HTMLButtonElement).dataset.label = t("createAccount");
        return;
      }
      if (pw.length < 10) throw Object.assign(new Error(t("passwordShort")), { name: "Local" });
      if (pw !== String(d.get("confirm"))) throw Object.assign(new Error(t("passwordsDiffer")), { name: "Local" });
      const code = needsCode ? String(d.get("code") ?? "").trim() : undefined;
      toast(t("creatingKeys"));
      try {
        await call({ type: "signup", server, email, password: pw, code });
      } catch (e) {
        if (needsCode && (e as Error).message.startsWith("verification_failed")) {
          throw Object.assign(new Error(t("errCode")), { name: "Local" });
        }
        throw e;
      }
      await refresh();
    },
    input("email", "email", { type: "email", autocomplete: "email", required: true, autofocus: true, dir: "ltr" }),
    codeBox,
    input("password", "masterPassword", { type: "password", autocomplete: "new-password", required: true, minlength: 10 }),
    input("confirm", "confirmPassword", { type: "password", autocomplete: "new-password", required: true }),
    h("p", { class: "muted small" }, t("passwordHint")),
    h("label", { class: "check" }, h("input", { type: "checkbox", name: "understand", required: true }), h("span", {}, t("noReset"))),
    h("button", { type: "submit", class: "primary wide" }, needsCode ? t("sendCode") : t("createAccount")),
  );
  screen(header(t("createAccount"), { back: showWelcome }), h("main", {}, f));
}

function kitText(k: NonNullable<Status["kit"]>): string {
  return [
    t("kitFileTitle"),
    "",
    `${t("email")}: ${k.email}`,
    `${t("server")}: ${status.server ?? status.settings.server}`,
    "",
    `${t("secretKey")}:`,
    k.secretKey,
    "",
    `${t("recoveryWords")}:`,
    k.recoveryWords
      .split(" ")
      .map((w, i) => `${String(i + 1).padStart(2, " ")}. ${w}`)
      .join("\n"),
    "",
    t("kitFileWarn"),
    "",
  ].join("\n");
}

function showKit() {
  const k = status.kit!;
  const done = h("button", { class: "primary wide", disabled: true, onclick: async () => {
    await call({ type: "kitSaved" });
    await refresh();
  } }, t("continue")) as HTMLButtonElement;
  screen(
    header(t("kitTitle")),
    h(
      "main",
      {},
      h("p", {}, t("kitIntro")),
      h("h2", {}, t("secretKey")),
      h("p", { class: "secret mono", dir: "ltr", "data-testid": "secret-key" }, k.secretKey),
      h("h2", {}, t("recoveryWords")),
      h(
        "ol",
        { class: "words mono", dir: "ltr", "data-testid": "recovery-words" },
        ...k.recoveryWords.split(" ").map((w) => h("li", {}, w)),
      ),
      h("button", {
        class: "wide",
        onclick: () => {
          const blob = new Blob([kitText(k)], { type: "text/plain;charset=utf-8" });
          const a = h("a", { href: URL.createObjectURL(blob), download: "Amanfold-Recovery-Kit.txt" });
          a.click();
          setTimeout(() => URL.revokeObjectURL((a as HTMLAnchorElement).href), 1000);
        },
      }, t("downloadKit")),
      h("label", { class: "check" }, h("input", { type: "checkbox", name: "saved", onchange: (e: Event) => {
        done.disabled = !(e.target as HTMLInputElement).checked;
      } }), h("span", {}, t("kitSavedCheck"))),
      done,
    ),
  );
}

function showSignin() {
  const server = status.settings.server;
  screen(
    header(t("signIn"), { back: showWelcome }),
    h(
      "main",
      {},
      form(
        async (d) => {
          await call({
            type: "signin",
            server,
            email: String(d.get("email")).trim(),
            password: String(d.get("password")),
            secretKey: String(d.get("secretKey")),
          });
          await refresh();
        },
        input("email", "email", { type: "email", autocomplete: "email", required: true, autofocus: true, dir: "ltr" }),
        input("password", "masterPassword", { type: "password", autocomplete: "current-password", required: true }),
        input("secretKey", "secretKey", { autocomplete: "off", spellcheck: "false", required: true, dir: "ltr", class: "mono" }),
        h("button", { type: "submit", class: "primary wide" }, t("signIn")),
        h("p", { class: "muted small" }, `${t("server")}: ${server}`),
      ),
    ),
  );
}

function showUnlock() {
  screen(
    header(t("locked")),
    h(
      "main",
      {},
      h("p", { class: "muted", dir: "ltr" }, status.email ?? ""),
      form(
        async (d) => {
          await call({ type: "unlock", password: String(d.get("password")) });
          await refresh();
        },
        input("password", "masterPassword", { type: "password", autocomplete: "current-password", required: true, autofocus: true }),
        h("button", { type: "submit", class: "primary wide" }, t("unlock")),
      ),
      h("button", { class: "link", onclick: signOut }, t("signOut")),
    ),
  );
}

async function signOut() {
  if (!confirm(t("signOutConfirm"))) return;
  await call({ type: "signout" });
  await refresh();
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

function syncBanner(): Child {
  if (status.syncError === "rollback") return h("div", { class: "banner danger", role: "alert" }, t("errRollback"));
  if (status.syncError === "signedOut") return h("div", { class: "banner danger", role: "alert" }, t("errSignedOut"));
  if (status.syncError === "offline") return h("div", { class: "banner" }, t("syncOffline"));
  return null;
}

function itemRow(i: ItemView, opts: { fill?: boolean } = {}) {
  const sub = i.kind === "Card" ? field(i, "card_number").replace(/\d(?=\d{4})/g, "•").slice(-9) : field(i, "username");
  return h(
    "li",
    { class: "row" },
    h(
      "button",
      { class: "row-main", onclick: () => showItem(i.id) },
      h("span", { class: `kind kind-${i.kind}`, "aria-hidden": "true" }, i.kind === "Card" ? "▭" : i.kind === "SecureNote" ? "✎" : "●"),
      h("span", { class: "row-text" }, h("span", { class: "row-title" }, itemTitle(i)), sub && h("span", { class: "row-sub", dir: "ltr" }, sub)),
    ),
    opts.fill && h("button", { class: "primary small", onclick: () => fill(i) }, t("fill")),
  );
}

async function fill(i: ItemView, extra: { confirmFrames?: number[]; anySite?: boolean } = {}) {
  if (tabId === undefined) return;
  try {
    const r = await call<FillResult>({ type: "fill", tabId, itemId: i.id, ...extra });
    if (r.needsConfirm?.length) {
      const hosts = r.needsConfirm.map((x) => x.host).join(", ");
      if (confirm(t("confirmFrame", hosts))) {
        return fill(i, { ...extra, confirmFrames: r.needsConfirm.map((x) => x.frameId) });
      }
      return;
    }
    const notes: string[] = [];
    if (r.blockedFrames?.length) notes.push(t("blockedFrames", r.blockedFrames.join(", ")));
    if (r.skippedHidden > 0) notes.push(t("hiddenSkipped"));
    if (r.filled > 0 && notes.length === 0) {
      toast(t("filledN"));
      if (!new URLSearchParams(location.search).has("tab")) setTimeout(() => window.close(), 400);
    } else if (r.filled > 0) {
      toast(`${t("filledN")} ${notes.join(" ")}`);
    } else {
      toast(notes.join(" ") || t("nothingFilled"), "err");
    }
  } catch (e) {
    const err = e as Error;
    if (err.name === "NotThisSite" && !extra.anySite) {
      if (confirm(t("confirmAnySite", tab?.site?.displayHost ?? ""))) return fill(i, { ...extra, anySite: true });
      return;
    }
    toast(errorText(e), "err");
  }
}

async function showHome(query = "") {
  const web = tabId !== undefined;
  tab = undefined;
  if (web) {
    try {
      tab = await call<TabInfo>({ type: "tabInfo", tabId: tabId! });
    } catch {
      tab = undefined;
    }
  }
  if (!tab?.site) tab = undefined;
  const all = await call<ItemView[]>({ type: "items" });
  const site = tab?.site;

  const top: Child[] = [syncBanner()];
  if (site) {
    top.push(
      h(
        "div",
        { class: "site" },
        h("span", { class: site.secure ? "lockicon" : "lockicon open", "aria-hidden": "true" }, site.secure ? "🔒" : "⚠"),
        h("strong", { dir: "ltr" }, site.displayHost),
      ),
    );
    if (site.displayHost !== site.host) top.push(h("p", { class: "banner warn", dir: "auto" }, t("punycode", site.host)));
    if (!site.secure) top.push(h("p", { class: "muted small" }, t("insecure")));
  }
  if (tab?.lookalikes.length) {
    top.push(h("div", { class: "banner danger", role: "alert", "data-testid": "lookalike" }, t("lookalike", tab.lookalikes.join(", "))));
  }
  const p = tab?.pendingSave;
  if (p) {
    top.push(
      h(
        "div",
        { class: "banner save", "data-testid": "save-prompt" },
        h("p", {}, p.updateId ? t("updatePrompt", p.displayHost) : t("savePrompt", p.displayHost)),
        p.username && h("p", { class: "muted small", dir: "ltr" }, p.username),
        h(
          "div",
          { class: "actions" },
          h("button", { class: "primary small", onclick: async () => {
            await call({ type: "pendingSave", tabId: tabId!, action: "save" });
            await showHome();
          } }, p.updateId ? t("update") : t("save")),
          h("button", { class: "small", onclick: async () => {
            await call({ type: "pendingSave", tabId: tabId!, action: "dismiss" });
            await showHome();
          } }, t("notNow")),
        ),
      ),
    );
  }

  const sections: Child[] = [];
  if (!query && site && !tab?.lookalikes.length) {
    sections.push(h("h2", {}, t("forThisSite")));
    sections.push(
      tab!.matches.length
        ? h("ul", { class: "list", "data-testid": "matches" }, ...tab!.matches.map((m) => itemRow(m.item, { fill: true })))
        : h("p", { class: "muted small" }, t("noneForSite")),
    );
    const cards = all.filter((i) => i.kind === "Card");
    if (cards.length) {
      sections.push(h("h2", {}, t("cards")));
      sections.push(h("ul", { class: "list", "data-testid": "cards" }, ...cards.map((c) => itemRow(c, { fill: true }))));
    }
  } else if (!query && !site) {
    sections.push(h("p", { class: "muted small" }, t("noPage")));
  }

  const q = query.toLowerCase();
  const shown = all.filter(
    (i) => !q || i.fields.some((f) => !f.concealed && f.value.toLowerCase().includes(q)),
  );
  sections.push(h("h2", {}, t("allItems")));
  sections.push(
    all.length === 0
      ? h("p", { class: "muted" }, t("empty"))
      : h("ul", { class: "list", "data-testid": "all-items" }, ...shown.map((i) => itemRow(i))),
  );

  const search = h("input", {
    type: "search",
    placeholder: t("search"),
    "aria-label": t("search"),
    value: query,
    oninput: (e: Event) => {
      const v = (e.target as HTMLInputElement).value;
      clearTimeout((search as unknown as { timer?: number }).timer);
      (search as unknown as { timer?: number }).timer = setTimeout(() => void showHome(v).then(() => {
        const s = app.querySelector<HTMLInputElement>("input[type=search]");
        s?.focus();
        s?.setSelectionRange(v.length, v.length);
      }), 150) as unknown as number;
    },
  });

  screen(
    header(t("appName"), {
      actions: [
        h("button", { class: "icon", "aria-label": t("settings"), title: t("settings"), onclick: showSettings }, "⚙"),
        h("button", { class: "icon", "aria-label": t("lock"), title: t("lock"), onclick: async () => {
          await call({ type: "lock" });
          await refresh();
        } }, "🔒"),
      ],
    }),
    h(
      "main",
      {},
      ...top,
      h(
        "div",
        { class: "toolbar" },
        search,
        h("button", { class: "primary small", "aria-haspopup": "menu", onclick: (e: Event) => addMenu(e.currentTarget as HTMLElement) }, `+ ${t("add")}`),
      ),
      ...sections,
    ),
  );
}

function addMenu(anchor: HTMLElement) {
  document.querySelector(".menu")?.remove();
  const pick = (k: ItemKind) => () => {
    menu.remove();
    showEdit(undefined, k);
  };
  const menu = h(
    "div",
    { class: "menu", role: "menu" },
    h("button", { role: "menuitem", onclick: pick("Login") }, t("newLogin")),
    h("button", { role: "menuitem", onclick: pick("Card") }, t("newCard")),
    h("button", { role: "menuitem", onclick: pick("SecureNote") }, t("newNote")),
  );
  anchor.after(menu);
  (menu.firstChild as HTMLElement).focus();
}

// ---------------------------------------------------------------------------
// Item, edit, generator
// ---------------------------------------------------------------------------

const LABELS: Record<string, MessageKey> = {
  title: "title",
  username: "username",
  password: "password",
  url: "website",
  totp: "oneTimeCode",
  notes: "notes",
  cardholder: "cardholder",
  card_number: "cardNumber",
  exp_month: "expMonth",
  exp_year: "expYear",
  cvv: "cvv",
  pin: "pin",
};

const ORDER: Record<ItemKind, string[]> = {
  Login: ["username", "password", "totp", "url", "notes"],
  Card: ["cardholder", "card_number", "exp_month", "exp_year", "cvv", "pin", "notes"],
  SecureNote: ["notes"],
  Identity: ["notes"],
  Passkey: ["notes"],
};

let totpTimer: number | undefined;

async function showItem(id: string) {
  clearInterval(totpTimer);
  const i = await call<ItemView | null>({ type: "item", id });
  if (!i) return showHome();
  const names = [...ORDER[i.kind], ...i.fields.map((f) => f.name).filter((n) => n !== "title" && !ORDER[i.kind].includes(n))];
  const rows: Child[] = [];
  for (const name of names) {
    const f = i.fields.find((x) => x.name === name);
    if (!f) continue;
    const label = LABELS[name] ? t(LABELS[name]!) : name.replace(/^custom:/, "").replace(/^url\.\d+$/, t("website"));
    if (name === "totp") {
      const code = h("span", { class: "mono big", dir: "ltr", "data-testid": "totp-code" }, "······");
      const left = h("span", { class: "muted small" });
      const tick = async () => {
        try {
          const c = await call<TotpCode>({ type: "totp", secret: f.value });
          code.textContent = `${c.code.slice(0, 3)} ${c.code.slice(3)}`;
          code.dataset.code = c.code;
          left.textContent = t("secondsLeft", c.secondsRemaining);
        } catch {
          code.textContent = "—";
        }
      };
      void tick();
      totpTimer = setInterval(tick, 1000) as unknown as number;
      rows.push(
        h("div", { class: "field" }, h("span", { class: "label" }, label), h("div", { class: "value" }, code, left),
          h("button", { class: "small", onclick: () => copy(code.dataset.code ?? "") }, t("copy"))),
      );
      continue;
    }
    const val = h("span", { class: f.concealed ? "mono" : "", dir: "auto" }, f.concealed ? "••••••••" : f.value);
    let shown = !f.concealed;
    rows.push(
      h(
        "div",
        { class: "field" },
        h("span", { class: "label" }, label),
        h("div", { class: "value" }, val),
        f.concealed && h("button", { class: "small", onclick: (e: Event) => {
          shown = !shown;
          val.textContent = shown ? f.value : "••••••••";
          (e.target as HTMLElement).textContent = shown ? t("hide") : t("show");
        } }, t("show")),
        h("button", { class: "small", onclick: () => copy(f.value) }, t("copy")),
      ),
    );
  }
  const canFill = (i.kind === "Login" || i.kind === "Card") && tabId !== undefined;
  screen(
    header(itemTitle(i), {
      back: () => {
        clearInterval(totpTimer);
        void showHome();
      },
    }),
    h(
      "main",
      {},
      ...rows,
      h(
        "div",
        { class: "actions" },
        canFill && h("button", { class: "primary", onclick: () => fill(i) }, t("fill")),
        h("button", { onclick: () => {
          clearInterval(totpTimer);
          showEdit(i, i.kind);
        } }, t("edit")),
        h("button", { class: "danger", onclick: async () => {
          if (!confirm(t("deleteConfirm"))) return;
          clearInterval(totpTimer);
          await call({ type: "trash", id: i.id, trashed: true });
          await showHome();
        } }, t("delete")),
      ),
    ),
  );
}

function showEdit(existing: ItemView | undefined, kind: ItemKind) {
  const val = (n: string) => (existing ? field(existing, n) : "");
  const names = ["title", ...ORDER[kind]];
  const defaults: Record<string, string> = {};
  if (!existing && kind === "Login" && tab?.site) {
    defaults.url = new URL(tab.url).origin;
    defaults.title = tab.site.displayHost.replace(/^www\./, "");
  }
  const cardNote = h("p", { class: "error small", "aria-live": "polite" });
  const inputs = names.map((n) => {
    const v = val(n) || defaults[n] || "";
    const label = LABELS[n] ?? "notes";
    if (n === "notes") {
      return h("label", { for: "f-notes" }, h("span", {}, t("notes")), h("textarea", { id: "f-notes", name: n, rows: 3 }, v));
    }
    const attrs: Attrs = { value: v, autocomplete: "off" };
    if (n === "password" || n === "cvv" || n === "pin") attrs.type = "password";
    if (["url", "username", "card_number", "exp_month", "exp_year", "cvv", "pin", "totp"].includes(n)) attrs.dir = "ltr";
    if (n === "card_number") {
      attrs.inputmode = "numeric";
      attrs.onblur = async (e: Event) => {
        const raw = (e.target as HTMLInputElement).value;
        cardNote.textContent = "";
        if (!raw.trim()) return;
        try {
          const info = await call<{ luhnValid: boolean }>({ type: "cardInfo", number: raw });
          if (!info.luhnValid) cardNote.textContent = t("cardInvalid");
        } catch {
          cardNote.textContent = t("cardInvalid");
        }
      };
    }
    if (n === "exp_month") Object.assign(attrs, { inputmode: "numeric", maxlength: 2, placeholder: "MM" });
    if (n === "exp_year") Object.assign(attrs, { inputmode: "numeric", maxlength: 4, placeholder: "YYYY" });
    if (n === "title") attrs.autofocus = true;
    const box = input(n, label, attrs);
    if (n === "password") {
      const inp = box.querySelector("input")!;
      box.append(
        h("div", { class: "inline" },
          h("button", { type: "button", class: "small", onclick: () => {
            inp.type = inp.type === "password" ? "text" : "password";
          } }, t("show")),
          h("button", { type: "button", class: "small", onclick: () => showGenerator((pw) => {
            inp.value = pw;
            inp.type = "text";
          }) }, t("generate")),
        ),
      );
    }
    if (n === "totp") box.append(h("span", { class: "muted small" }, t("totpHint")));
    if (n === "card_number") box.append(cardNote);
    return box;
  });

  const f = form(
    async (d) => {
      const set: FieldInput[] = [];
      const remove: string[] = [];
      for (const n of names) {
        let v = String(d.get(n) ?? "").trim();
        if (n === "exp_month" && /^\d$/.test(v)) v = `0${v}`;
        if (v) set.push({ name: n, value: v });
        else if (existing && field(existing, n)) remove.push(n);
      }
      if (!set.some((x) => x.name === "title")) {
        set.push({ name: "title", value: kind === "Card" ? t("newCard") : t("untitled") });
      }
      if (existing) {
        await call({ type: "update", id: existing.id, set, remove });
        await showItem(existing.id);
      } else {
        await call({ type: "create", kind, fields: set });
        await showHome();
      }
    },
    ...inputs,
    h("div", { class: "actions" },
      h("button", { type: "submit", class: "primary" }, t("save")),
      h("button", { type: "button", onclick: () => (existing ? showItem(existing.id) : showHome()) }, t("cancel")),
    ),
  );
  screen(header(existing ? t("edit") : t(kind === "Card" ? "newCard" : kind === "Login" ? "newLogin" : "newNote"), {
    back: () => (existing ? showItem(existing.id) : showHome()),
  }), h("main", {}, f));
}

function showGenerator(use: (pw: string) => void) {
  const dlg = h("dialog", { class: "generator" }) as HTMLDialogElement;
  const out = h("output", { class: "mono big", dir: "ltr", "data-testid": "generated" });
  const rules: PasswordRules = { length: 20, lowercase: true, uppercase: true, digits: true, symbols: true, avoidAmbiguous: true };
  let words = 0;
  const regen = async () => {
    try {
      out.textContent = await call<string>({ type: "generate", rules, words: words || undefined });
    } catch (e) {
      out.textContent = errorText(e);
    }
  };
  const lenLabel = h("span", {}, `${t("length")}: ${rules.length}`);
  const toggle = (k: keyof PasswordRules, label: MessageKey) =>
    h("label", { class: "check" }, h("input", { type: "checkbox", checked: rules[k] as boolean, onchange: (e: Event) => {
      (rules[k] as boolean) = (e.target as HTMLInputElement).checked;
      void regen();
    } }), h("span", {}, t(label)));
  dlg.append(
    h("h2", {}, t("generator")),
    out,
    h("label", {}, lenLabel, h("input", { type: "range", min: 8, max: 64, value: rules.length, oninput: (e: Event) => {
      rules.length = Number((e.target as HTMLInputElement).value);
      lenLabel.textContent = `${t("length")}: ${rules.length}`;
      void regen();
    } })),
    h("div", { class: "inline" }, toggle("lowercase", "lowercase"), toggle("uppercase", "uppercase"), toggle("digits", "digits"), toggle("symbols", "symbols")),
    h("label", { class: "check" }, h("input", { type: "checkbox", onchange: (e: Event) => {
      words = (e.target as HTMLInputElement).checked ? 5 : 0;
      void regen();
    } }), h("span", {}, t("passphrase"))),
    h("div", { class: "actions" },
      h("button", { class: "primary", onclick: () => {
        use(out.textContent ?? "");
        dlg.close();
        dlg.remove();
      } }, t("use")),
      h("button", { onclick: regen }, t("generate")),
      h("button", { onclick: () => copy(out.textContent ?? "") }, t("copy")),
      h("button", { onclick: () => {
        dlg.close();
        dlg.remove();
      } }, t("cancel")),
    ),
  );
  document.body.append(dlg);
  dlg.showModal();
  void regen();
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

async function showSettings() {
  status = await call<Status>({ type: "status" });
  const s = status.settings;
  const lockOpts: [number, string][] = [
    [1, t("minutes", 1)],
    [5, t("minutes", 5)],
    [15, t("minutes", 15)],
    [60, t("hours", 1)],
    [240, t("hours", 4)],
    [0, t("never")],
  ];
  const select = (name: string, value: string, opts: [string, string][], onchange: (v: string) => void) =>
    h("select", { id: `f-${name}`, name, onchange: (e: Event) => onchange((e.target as HTMLSelectElement).value) },
      ...opts.map(([v, label]) => h("option", { value: v, selected: v === value }, label)));
  screen(
    header(t("settings"), { back: () => void showHome() }),
    h(
      "main",
      {},
      h("label", { for: "f-lang" }, h("span", {}, t("language")),
        select("lang", s.lang ?? "", [["", t("langAuto")], ["en", "English"], ["ar", "العربية"]], async (v) => {
          await call({ type: "settings", set: { lang: (v || null) as Status["settings"]["lang"] } });
          setLang((v || null) as Status["settings"]["lang"]);
          await showSettings();
        })),
      h("label", { for: "f-autolock" }, h("span", {}, t("autoLock")),
        select("autolock", String(s.autoLockMinutes), lockOpts.map(([v, l]) => [String(v), l]), async (v) => {
          await call({ type: "settings", set: { autoLockMinutes: Number(v) } });
        })),
      h("h2", {}, t("account")),
      h("p", { dir: "ltr" }, status.email ?? ""),
      h("p", { class: "muted small", dir: "ltr" }, status.server ?? ""),
      h("p", { class: "muted small" }, status.lastSyncMs ? t("lastSync", ago(status.lastSyncMs)) : t("neverSynced")),
      syncBanner(),
      h("div", { class: "actions" },
        h("button", { onclick: async () => {
          try {
            await call({ type: "sync" });
            toast(t("lastSync", ago(Date.now())));
          } catch (e) {
            toast(errorText(e), "err");
          }
          await showSettings();
        } }, t("syncNow")),
        h("button", { onclick: async () => {
          await call({ type: "lock" });
          await refresh();
        } }, t("lock")),
        h("button", { class: "danger", onclick: signOut }, t("signOut")),
      ),
      h("p", { class: "muted small" }, t("version", status.version)),
      h("p", { class: "muted small" }, t("betaNote")),
    ),
  );
}

main().catch((e) => {
  app.textContent = errorText(e);
});
