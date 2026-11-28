import { test } from "node:test";
import assert from "node:assert/strict";
import { planCard, planLogin, type ScannedFrame } from "../../src/background/policy.ts";
import type { Strength } from "../../src/shared/types.ts";

// Stand-ins for the core's matching (the real rules are tested in Rust,
// crates/amanfold-core/src/site.rs): same site = same last two labels.
const siteOf = (u: string) => {
  const host = new URL(u).hostname;
  return { host, site: host.split(".").slice(-2).join(".") };
};
const match = (saved: string, page: string): Strength => {
  if (new URL(saved).hostname === new URL(page).hostname) return "Origin";
  return siteOf(saved).site === siteOf(page).site ? "Site" : "None";
};
const login = { login: true, card: false, otp: false };
const card = { login: false, card: true, otp: false };
const none = { login: false, card: false, otp: false };
const f = (frameId: number, parentFrameId: number, url: string, scan = login): ScannedFrame => ({
  frameId,
  parentFrameId,
  url,
  scan,
});
const wantsLogin = (s: { login: boolean }) => s.login;

test("login fills the top page and same-site frames only", () => {
  const frames = [
    f(0, -1, "https://bank.com/login"),
    f(1, 0, "https://accounts.bank.com/frame"),
    f(2, 0, "https://evil.io/frame"),
    f(3, 0, "https://ads.io/frame", none),
  ];
  const p = planLogin(frames, ["https://bank.com"], match, siteOf, wantsLogin);
  assert.deepEqual(p.targets.map((x) => x.frameId), [0, 1]);
  assert.deepEqual(p.blocked, ["evil.io"]);
});

test("login is never filled into its own site framed by another site", () => {
  const frames = [f(0, -1, "https://evil.io/", none), f(1, 0, "https://bank.com/login")];
  const p = planLogin(frames, ["https://bank.com"], match, siteOf, wantsLogin);
  assert.deepEqual(p.targets, []);
  assert.deepEqual(p.blocked, ["bank.com"]);
});

test("a bank frame inside a foreign frame inside the bank is refused", () => {
  const frames = [
    f(0, -1, "https://bank.com/", none),
    f(1, 0, "https://widget.io/", none),
    f(2, 1, "https://bank.com/inner"),
  ];
  assert.deepEqual(planLogin(frames, ["https://bank.com"], match, siteOf, wantsLogin).targets, []);
});

test("a frame whose parent is unknown is refused", () => {
  const frames = [f(0, -1, "https://bank.com/", none), f(5, 4, "https://bank.com/x")];
  assert.deepEqual(planLogin(frames, ["https://bank.com"], match, siteOf, wantsLogin).targets, []);
});

test("cards: same site fills, other sites ask first, nested foreign chains are refused", () => {
  const frames = [
    f(0, -1, "https://shop.com/checkout", card),
    f(1, 0, "https://pay.shop.com/card", card),
    f(2, 0, "https://js.stripe.com/elements", card),
    f(3, 2, "https://other.io/inner", card),
  ];
  let p = planCard(frames, [], siteOf);
  assert.deepEqual(p.targets.map((x) => x.frameId), [0, 1]);
  assert.deepEqual(p.needsConfirm, [{ frameId: 2, host: "js.stripe.com" }]);
  assert.deepEqual(p.blocked, ["other.io"]);
  p = planCard(frames, [2], siteOf);
  assert.deepEqual(p.targets.map((x) => x.frameId), [0, 1, 2]);
  assert.deepEqual(p.needsConfirm, []);
});

test("cards: nothing without a web page on top", () => {
  assert.deepEqual(planCard([f(1, 0, "https://shop.com/", card)], [], siteOf).targets, []);
});
