import { catalogs, type MessageKey } from "./locales.ts";
import type { Lang } from "./types.ts";

let lang: Lang = "en";

export function browserLang(): Lang {
  const ui = (globalThis.chrome?.i18n?.getUILanguage?.() ?? globalThis.navigator?.language ?? "en").toLowerCase();
  return ui.startsWith("ar") ? "ar" : "en";
}

export function setLang(l: Lang | null) {
  lang = l ?? browserLang();
}

export function currentLang(): Lang {
  return lang;
}

export function dir(l: Lang = lang): "rtl" | "ltr" {
  return l === "ar" ? "rtl" : "ltr";
}

/** Text for `key`, with {0}, {1}... replaced by `args`. */
export function t(key: MessageKey, ...args: (string | number)[]): string {
  const s: string = catalogs[lang][key] ?? catalogs.en[key];
  return s.replace(/\{(\d)\}/g, (_, i) => String(args[Number(i)] ?? ""));
}

/** Relative time, in the interface language. */
export function ago(ms: number): string {
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto" });
  const s = Math.round((ms - Date.now()) / 1000);
  if (Math.abs(s) < 60) return rtf.format(s, "second");
  if (Math.abs(s) < 3600) return rtf.format(Math.round(s / 60), "minute");
  if (Math.abs(s) < 86400) return rtf.format(Math.round(s / 3600), "hour");
  return rtf.format(Math.round(s / 86400), "day");
}
