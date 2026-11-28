// Which frames of a tab may receive which values. Pure functions, so the
// rules are unit-tested without a browser.
//
// Logins: only frames whose own address matches the login AND whose every
// ancestor, up to the top page, matches it too. A login for bank.com is never
// filled into a bank.com frame embedded by another site (clickjacking), nor
// into a frame from another site embedded by bank.com.
//
// Cards have no site. They go to frames on the same site as the top page;
// a frame from another site (a payment provider such as a card iframe) is
// filled only after the person says yes to that frame's host, and only if
// every frame above it is on the top page's site.

import type { ScanResult, Strength } from "../shared/types.ts";

export interface Frame {
  frameId: number;
  parentFrameId: number;
  url: string;
  documentId?: string;
}

export interface ScannedFrame extends Frame {
  scan: ScanResult;
}

export type MatchFn = (savedUrl: string, pageUrl: string) => Strength;
export type SiteFn = (pageUrl: string) => { site: string; host: string } | undefined;

function ancestors(frames: Frame[], f: Frame): Frame[] | undefined {
  const byId = new Map(frames.map((x) => [x.frameId, x]));
  const out: Frame[] = [];
  let cur = f;
  for (let guard = 0; cur.parentFrameId !== -1; guard++) {
    const p = byId.get(cur.parentFrameId);
    if (!p || guard > 64) return undefined; // broken chain: refuse
    out.push(p);
    cur = p;
  }
  return out;
}

export function matchesAny(urls: string[], page: string, match: MatchFn): boolean {
  return urls.some((u) => match(u, page) !== "None");
}

export interface LoginPlan {
  targets: Frame[];
  /** Hosts of frames with login fields that were refused. */
  blocked: string[];
}

export function planLogin(
  frames: ScannedFrame[],
  itemUrls: string[],
  match: MatchFn,
  site: SiteFn,
  wants: (s: ScanResult) => boolean,
): LoginPlan {
  const targets: Frame[] = [];
  const blocked: string[] = [];
  for (const f of frames) {
    if (!wants(f.scan)) continue;
    const chain = ancestors(frames, f);
    const ok =
      chain !== undefined &&
      matchesAny(itemUrls, f.url, match) &&
      chain.every((a) => matchesAny(itemUrls, a.url, match));
    if (ok) targets.push(f);
    else blocked.push(site(f.url)?.host ?? "?");
  }
  return { targets, blocked: [...new Set(blocked)] };
}

export interface CardPlan {
  targets: Frame[];
  needsConfirm: { frameId: number; host: string }[];
  blocked: string[];
}

export function planCard(frames: ScannedFrame[], confirmed: number[], site: SiteFn): CardPlan {
  const top = frames.find((f) => f.parentFrameId === -1);
  const topSite = top && site(top.url)?.site;
  const plan: CardPlan = { targets: [], needsConfirm: [], blocked: [] };
  if (!topSite) return plan;
  for (const f of frames) {
    if (!f.scan.card) continue;
    const s = site(f.url);
    const chain = ancestors(frames, f);
    if (!s || !chain) {
      plan.blocked.push(s?.host ?? "?");
      continue;
    }
    if (!chain.every((a) => site(a.url)?.site === topSite)) {
      plan.blocked.push(s.host);
    } else if (s.site === topSite || confirmed.includes(f.frameId)) {
      plan.targets.push(f);
    } else {
      plan.needsConfirm.push({ frameId: f.frameId, host: s.host });
    }
  }
  return plan;
}

/** Origin a content script must still be on when the values arrive. */
export function originOf(url: string): string {
  return new URL(url).origin;
}
