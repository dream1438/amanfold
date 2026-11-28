// Finding login, one-time-code and card fields on a page. Reads the page;
// never changes it. `autocomplete` attributes win; otherwise names, ids,
// labels and placeholders are matched in English and Arabic.

export type Role =
  | "username"
  | "password"
  | "newPassword"
  | "otp"
  | "ccNumber"
  | "ccName"
  | "ccExp"
  | "ccExpMonth"
  | "ccExpYear"
  | "ccCsc";

export interface Found {
  el: HTMLInputElement | HTMLSelectElement;
  role: Role;
}

type Control = HTMLInputElement | HTMLSelectElement;

const TEXTLIKE = new Set(["text", "email", "tel", "number", "search", "url", ""]);

/** All inputs and selects, including inside open shadow roots. */
export function controls(root: Document | ShadowRoot = document): Control[] {
  const out: Control[] = [];
  const walk = (r: Document | ShadowRoot | Element) => {
    for (const el of Array.from(r.querySelectorAll("*"))) {
      if (el instanceof HTMLInputElement || el instanceof HTMLSelectElement) out.push(el);
      if (el.shadowRoot) walk(el.shadowRoot);
    }
  };
  walk(root);
  return out;
}

function labelText(el: Control): string {
  const parts: string[] = [];
  for (const l of Array.from(el.labels ?? [])) parts.push(l.textContent ?? "");
  const by = el.getAttribute("aria-labelledby");
  if (by) {
    for (const id of by.split(/\s+/)) parts.push(el.ownerDocument.getElementById(id)?.textContent ?? "");
  }
  parts.push(el.getAttribute("aria-label") ?? "");
  if (el instanceof HTMLInputElement) parts.push(el.placeholder ?? "");
  return parts.join(" ").toLowerCase();
}

function hints(el: Control): string {
  return [el.name, el.id, el.getAttribute("data-testid") ?? "", el.className.toString(), labelText(el)]
    .join(" ")
    .toLowerCase();
}

function tokens(el: Control): string[] {
  return (el.getAttribute("autocomplete") ?? "").toLowerCase().split(/\s+/).filter(Boolean);
}

const RE = {
  ccNumber: /card.?num|cc.?num|ccnumber|cardnumber|card.?no\b|creditcard|credit.?card|pan\b|رقم.?البطاقة/,
  ccName: /name.?on.?card|card.?holder|cardholder|cc.?name|holder.?name|nameoncard|اسم.?حامل|الاسم.?على.?البطاقة/,
  ccCsc: /cvv|cvc|csc|cvn|security.?code|card.?code|card.?verif|\bcid\b|رمز.?الأمان|رمز.?التحقق/,
  ccExpMonth: /(exp|expiry|expiration|valid).{0,6}(month|mm\b)|\bmonth\b|\bmm\b|cc.?month|الشهر/,
  ccExpYear: /(exp|expiry|expiration|valid).{0,6}(year|yy)|\byear\b|\byy(yy)?\b|cc.?year|السنة/,
  ccExp: /exp|expiry|expiration|valid.?thru|mm.?\/.?yy|تاريخ.?الانتهاء|الانتهاء/,
  otp: /one.?time|otp|totp|2fa|two.?factor|mfa|verification.?code|auth.?code|security.?code|\bcode\b|token|رمز|الرمز/,
  username: /user|login|email|e-mail|account|ident|member|phone|mobile|اسم.?المستخدم|البريد|الهاتف/,
  newPassword: /new|confirm|repeat|retype|again|verify|create|جديدة|تأكيد/,
};

function cardRole(el: Control, t: string[], h: string): Role | undefined {
  const map: Record<string, Role> = {
    "cc-number": "ccNumber",
    "cc-name": "ccName",
    "cc-csc": "ccCsc",
    "cc-exp": "ccExp",
    "cc-exp-month": "ccExpMonth",
    "cc-exp-year": "ccExpYear",
  };
  for (const tok of t) if (map[tok]) return map[tok];
  if (el instanceof HTMLInputElement && !TEXTLIKE.has(el.type) && el.type !== "password") return undefined;
  if (RE.ccNumber.test(h)) return "ccNumber";
  if (RE.ccName.test(h)) return "ccName";
  if (RE.ccCsc.test(h)) return "ccCsc";
  if (RE.ccExpMonth.test(h) && /exp|valid|card|cc|month|mm|شهر|البطاقة/.test(h)) return "ccExpMonth";
  if (RE.ccExpYear.test(h) && /exp|valid|card|cc|year|yy|سنة|البطاقة/.test(h)) return "ccExpYear";
  if (RE.ccExp.test(h) && el instanceof HTMLInputElement) return "ccExp";
  return undefined;
}

function isOtp(el: HTMLInputElement, t: string[], h: string): boolean {
  if (t.includes("one-time-code")) return true;
  if (!TEXTLIKE.has(el.type) && el.type !== "password") return false;
  const max = el.maxLength;
  const shortish = max === -1 || (max >= 4 && max <= 10);
  return shortish && RE.otp.test(h);
}

/** Classify every control on the page. */
export function detect(root: Document = document): Found[] {
  const all = controls(root);
  const found: Found[] = [];
  const passwords: HTMLInputElement[] = [];
  // Card roles guessed from names count only in a form with a card number;
  // alone, "security code" is more likely a one-time code.
  const guessed = new Set<Control>();

  for (const el of all) {
    if (el instanceof HTMLInputElement && ["hidden", "submit", "button", "checkbox", "radio", "file", "image", "reset"].includes(el.type)) continue;
    // autocomplete=off is common on login forms too; it is not a reason to skip.
    const t = tokens(el);
    const h = hints(el);
    const card = cardRole(el, t, h);
    if (card) {
      found.push({ el, role: card });
      if (!t.some((x) => x.startsWith("cc-"))) guessed.add(el);
      continue;
    }
    if (!(el instanceof HTMLInputElement)) continue;
    if (el.type === "password") {
      const isNew = t.includes("new-password") || (!t.includes("current-password") && RE.newPassword.test(h));
      if (isOtp(el, t, h) && !t.includes("current-password")) {
        found.push({ el, role: "otp" });
      } else {
        found.push({ el, role: isNew ? "newPassword" : "password" });
        passwords.push(el);
      }
      continue;
    }
    if (isOtp(el, t, h)) {
      found.push({ el, role: "otp" });
    }
  }

  {
    const numberForms = new Set(found.filter((f) => f.role === "ccNumber").map((f) => f.el.form));
    for (let i = found.length - 1; i >= 0; i--) {
      const f = found[i]!;
      if (!guessed.has(f.el) || numberForms.has(f.el.form)) continue;
      found.splice(i, 1);
      if (f.el instanceof HTMLInputElement && RE.otp.test(hints(f.el))) found.push({ el: f.el, role: "otp" });
    }
  }

  // Usernames: the text field closest before each password field, in the
  // same form (or the same page when there is no form). On a page without a
  // password field (first step of a two-step login), a field that says it
  // is a username or email.
  const taken = new Set(found.map((f) => f.el));
  const textInputs = all.filter(
    (el): el is HTMLInputElement => el instanceof HTMLInputElement && TEXTLIKE.has(el.type) && !taken.has(el),
  );
  const users = new Set<HTMLInputElement>();
  for (const pw of passwords) {
    const before = textInputs.filter(
      (el) => el.form === pw.form && el.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING,
    );
    const pick =
      before.reverse().find((el) => tokens(el).some((x) => x === "username" || x === "email") || RE.username.test(hints(el))) ??
      before[0];
    if (pick) users.add(pick);
  }
  if (passwords.length === 0) {
    for (const el of textInputs) {
      const t = tokens(el);
      if (t.includes("username") || (t.includes("email") && el.form) || /user.?name|login|اسم.?المستخدم/.test(hints(el))) {
        users.add(el);
      }
    }
  }
  for (const el of users) found.push({ el, role: "username" });
  return found;
}
