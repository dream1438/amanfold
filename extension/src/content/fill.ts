// Writing values into page fields the way typing would, so sites built on
// React, Vue or Angular notice the change.

import type { CardValues } from "../shared/types.ts";

type Control = HTMLInputElement | HTMLSelectElement;

function nativeSet(el: Control, value: string) {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  if (setter) setter.call(el, value);
  else el.value = value;
}

export function setValue(el: Control, value: string): boolean {
  if (el instanceof HTMLSelectElement) return setSelect(el, [value]);
  el.focus({ preventScroll: true });
  nativeSet(el, value);
  el.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertReplacementText" }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

function setSelect(el: HTMLSelectElement, candidates: string[]): boolean {
  const want = candidates.map((c) => c.trim().toLowerCase());
  for (const opt of Array.from(el.options)) {
    const v = opt.value.trim().toLowerCase();
    const t = (opt.textContent ?? "").trim().toLowerCase();
    if (want.includes(v) || want.includes(t)) {
      nativeSet(el, opt.value);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    }
  }
  return false;
}

const MONTHS = [
  ["january", "jan", "يناير"],
  ["february", "feb", "فبراير"],
  ["march", "mar", "مارس"],
  ["april", "apr", "أبريل"],
  ["may", "may", "مايو"],
  ["june", "jun", "يونيو"],
  ["july", "jul", "يوليو"],
  ["august", "aug", "أغسطس"],
  ["september", "sep", "سبتمبر"],
  ["october", "oct", "أكتوبر"],
  ["november", "nov", "نوفمبر"],
  ["december", "dec", "ديسمبر"],
];

export function monthCandidates(m: string): string[] {
  const n = parseInt(m, 10);
  if (!(n >= 1 && n <= 12)) return [m];
  const mm = String(n).padStart(2, "0");
  const names = MONTHS[n - 1] ?? [];
  return [mm, String(n), ...names, ...names.map((x) => `${mm} - ${x}`), `${mm} - ${names[0]}`];
}

export function yearCandidates(y: string): string[] {
  const full = y.length === 2 ? `20${y}` : y;
  return [full, full.slice(-2)];
}

/** "MM/YY" style for a single expiry box, following its placeholder or size. */
export function expiryText(el: HTMLInputElement, month: string, year: string): string {
  const mm = month.padStart(2, "0");
  const full = year.length === 2 ? `20${year}` : year;
  const ph = (el.placeholder || el.getAttribute("aria-label") || "").toLowerCase();
  const sep = /(mm|ش)\s+[/-]\s+/.test(ph) ? " / " : ph.includes("-") ? "-" : "/";
  let long: boolean;
  if (/yyyy|aaaa|سسسس/.test(ph)) long = true;
  else if (/yy|aa|سس/.test(ph)) long = false;
  else long = el.maxLength >= 2 + sep.length + 4;
  return `${mm}${sep}${long ? full : full.slice(-2)}`;
}

export function fillCard(el: Control, role: string, c: CardValues): boolean {
  switch (role) {
    case "ccNumber":
      return !!c.number && setValue(el, c.number);
    case "ccName":
      return !!c.name && setValue(el, c.name);
    case "ccCsc":
      return !!c.cvv && setValue(el, c.cvv);
    case "ccExpMonth":
      if (!c.expMonth) return false;
      return el instanceof HTMLSelectElement
        ? setSelect(el, monthCandidates(c.expMonth))
        : setValue(el, c.expMonth.padStart(2, "0"));
    case "ccExpYear":
      if (!c.expYear) return false;
      if (el instanceof HTMLSelectElement) return setSelect(el, yearCandidates(c.expYear));
      return setValue(el, el.maxLength === 2 ? yearCandidates(c.expYear)[1]! : yearCandidates(c.expYear)[0]!);
    case "ccExp":
      if (!c.expMonth || !c.expYear || !(el instanceof HTMLInputElement)) return false;
      return setValue(el, expiryText(el, c.expMonth, c.expYear));
  }
  return false;
}
