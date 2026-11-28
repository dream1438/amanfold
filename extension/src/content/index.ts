// Content script: runs in every frame of http(s) pages. It holds no vault
// data. It answers "what fields are here?", fills values the background sends
// for a fill the person started, and reports submitted login forms so the
// person can be offered to save them.

import { detect, type Found } from "./detect.ts";
import { fillCard, setValue } from "./fill.ts";
import { fillable, visibleInFrames } from "./visible.ts";
import type { ContentEvent, ContentReq, FillValues, ScanResult } from "../shared/types.ts";

function scan(): ScanResult {
  const f = detect();
  return {
    login: f.some((x) => x.role === "password" || x.role === "username"),
    card: f.some((x) => x.role.startsWith("cc")),
    otp: f.some((x) => x.role === "otp"),
  };
}

async function fill(values: FillValues): Promise<{ filled: number; skippedHidden: number }> {
  let filled = 0;
  let skippedHidden = 0;
  const inFrame = window !== window.top;
  const wanted = (f: Found): string | undefined => {
    switch (f.role) {
      case "username":
        return values.username;
      case "password":
        return values.password;
      case "otp":
        return values.totp;
      default:
        return f.role.startsWith("cc") && values.card ? "card" : undefined;
    }
  };
  for (const f of detect()) {
    const v = wanted(f);
    if (!v) continue;
    if (!fillable(f.el) || (inFrame && !(await visibleInFrames(f.el)))) {
      skippedHidden++;
      continue;
    }
    const ok = v === "card" ? fillCard(f.el, f.role, values.card!) : setValue(f.el, v);
    if (ok) filled++;
  }
  return { filled, skippedHidden };
}

chrome.runtime.onMessage.addListener((msg: ContentReq, sender, sendResponse) => {
  // Only our own background may talk to this script.
  if (sender.id !== chrome.runtime.id || sender.tab) return false;
  if (msg.type === "scan") {
    sendResponse(scan());
    return false;
  }
  if (msg.type === "fill") {
    // The frame may have navigated since the background checked it.
    if (location.origin !== msg.origin) {
      sendResponse({ filled: 0, skippedHidden: 0 });
      return false;
    }
    void fill(msg.values).then(sendResponse);
    return true;
  }
  return false;
});

// ---------------------------------------------------------------------------
// Save prompt: notice a login being submitted.
// ---------------------------------------------------------------------------

let lastSent = "";

function capture(scope: ParentNode | null) {
  const found = detect();
  const inScope = (el: Element) => !scope || scope === document || scope.contains(el);
  const pw =
    found.find((f) => f.role === "newPassword" && inScope(f.el) && f.el.value) ??
    found.find((f) => f.role === "password" && inScope(f.el) && f.el.value);
  if (!pw) return;
  const user = found.find((f) => f.role === "username" && inScope(f.el) && f.el.value);
  const ev: ContentEvent = { type: "submitted", username: user?.el.value.trim() ?? "", password: pw.el.value };
  const key = ev.username + "\u0000" + ev.password;
  if (key === lastSent) return;
  lastSent = key;
  void chrome.runtime.sendMessage(ev).catch(() => undefined);
}

// Only events the person caused: a page script can fire synthetic submits
// and clicks to plant a save prompt with a password it chose.
window.addEventListener("submit", (e) => e.isTrusted && capture(e.target as HTMLFormElement), true);
window.addEventListener(
  "click",
  (e) => {
    if (!e.isTrusted) return;
    const b = (e.target as Element | null)?.closest?.("button, input[type=submit], input[type=button], [role=button]");
    if (b) capture((b as HTMLButtonElement).form ?? document);
  },
  true,
);
window.addEventListener(
  "keydown",
  (e) => {
    if (e.isTrusted && e.key === "Enter" && e.target instanceof HTMLInputElement) capture(e.target.form ?? document);
  },
  true,
);
