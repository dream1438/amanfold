// Filling a tab. Runs only after the person asked (popup button or keyboard
// shortcut); nothing is ever filled when a page loads.

import { loadCore } from "./core.ts";
import { originOf, planCard, planLogin, type Frame, type ScannedFrame } from "./policy.ts";
import type { ContentReq, FillResult, FillValues, ItemView, ScanResult, Strength } from "../shared/types.ts";

const field = (i: ItemView, name: string) => i.fields.find((f) => f.name === name)?.value;

export function itemUrls(i: ItemView): string[] {
  return i.fields.filter((f) => f.name === "url" || /^url\.\d+$/.test(f.name)).map((f) => f.value);
}

async function frames(tabId: number): Promise<Frame[]> {
  const all = (await chrome.webNavigation.getAllFrames({ tabId })) ?? [];
  return all
    .filter((f) => /^https?:/.test(f.url))
    .map((f) => ({
      frameId: f.frameId,
      parentFrameId: f.parentFrameId,
      url: f.url,
      documentId: (f as { documentId?: string }).documentId,
    }));
}

function send<T>(tabId: number, f: Frame, msg: ContentReq): Promise<T | undefined> {
  const opts: chrome.tabs.MessageSendOptions = f.documentId ? { documentId: f.documentId } : { frameId: f.frameId };
  return chrome.tabs.sendMessage(tabId, msg, opts).catch(() => undefined) as Promise<T | undefined>;
}

async function scanAll(tabId: number): Promise<ScannedFrame[]> {
  const fs = await frames(tabId);
  const scans = await Promise.all(fs.map((f) => send<ScanResult>(tabId, f, { type: "scan" })));
  return fs.flatMap((f, i) => {
    const scan = scans[i];
    return scan ? [{ ...f, scan }] : [];
  });
}

async function deliver(tabId: number, targets: Frame[], values: FillValues, result: FillResult) {
  for (const f of targets) {
    const r = await send<{ filled: number; skippedHidden: number }>(tabId, f, {
      type: "fill",
      origin: originOf(f.url),
      values,
    });
    if (r) {
      result.filled += r.filled;
      result.skippedHidden += r.skippedHidden;
    }
  }
}

function fail(name: string): never {
  throw Object.assign(new Error(name), { name });
}

/**
 * Fill `item` into the tab. Logins go only to frames on the login's site
 * (see policy.ts). `anySite` lets the person fill a login on a site it was
 * not saved for, after a warning; it is refused when the page imitates one
 * of their saved sites.
 */
export async function fillTab(
  tabId: number,
  item: ItemView,
  opts: { confirmFrames?: number[]; anySite?: boolean; lookalikes: (url: string) => string[] },
): Promise<FillResult> {
  const core = await loadCore();
  const match = (saved: string, page: string) => core.siteMatch(saved, page) as Strength;
  const site = (url: string) => core.pageSite(url) as { site: string; host: string } | undefined;
  const scanned = await scanAll(tabId);
  const result: FillResult = { filled: 0, skippedHidden: 0 };

  if (item.kind === "Login") {
    const top = scanned.find((f) => f.parentFrameId === -1) ?? fail("NoPage");
    let urls = itemUrls(item);
    if (!urls.some((u) => match(u, top.url) !== "None")) {
      if (!opts.anySite) fail("NotThisSite");
      if (opts.lookalikes(top.url).length > 0) fail("Lookalike");
      // The person chose this page: treat its own origin as the login's.
      urls = [originOf(top.url)];
    }
    let totp: string | undefined;
    const secret = field(item, "totp");
    if (secret) totp = core.totpNow(secret).code;
    const plan = planLogin(scanned, urls, match, site, (s) => s.login || (s.otp && !!totp));
    result.blockedFrames = plan.blocked;
    await deliver(tabId, plan.targets, { username: field(item, "username"), password: field(item, "password"), totp }, result);
    return result;
  }

  if (item.kind === "Card") {
    const plan = planCard(scanned, opts.confirmFrames ?? [], site);
    result.blockedFrames = plan.blocked;
    if (plan.needsConfirm.length > 0) {
      result.needsConfirm = plan.needsConfirm;
      return result;
    }
    await deliver(
      tabId,
      plan.targets,
      {
        card: {
          number: field(item, "card_number"),
          name: field(item, "cardholder"),
          expMonth: field(item, "exp_month"),
          expYear: field(item, "exp_year"),
          cvv: field(item, "cvv"),
        },
      },
      result,
    );
    return result;
  }
  fail("CannotFill");
}
