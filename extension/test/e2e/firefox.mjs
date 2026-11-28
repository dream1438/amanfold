// End-to-end test in a real Firefox, driven over WebDriver BiDi with
// puppeteer-core: the Firefox build (dist/firefox) installed as a temporary
// add-on, a real amanfold-server and the same test pages as the Chromium
// test. Covers sign-up, the autofill rules (hidden fields, frames from other
// sites, look-alike sites), one-time codes, cards in a payment frame, the save
// prompt, lock and unlock, and sign-in on a second profile.
//
//   FIREFOX=/usr/bin/firefox node test/e2e/firefox.mjs
//   E2E_SKIP_BUILD=1 ...     (server and WASM already built)
//
// Needs Firefox 128 or later.

import puppeteer from "puppeteer-core";
import path from "node:path";
import { mkdirSync } from "node:fs";
import assert from "node:assert/strict";
import { currentStep, ext, log, start } from "./harness.mjs";

const firefox = process.env.FIREFOX ?? "firefox";
const ADDON_ID = "extension@amanfold.com";
// A fixed internal UUID, so the test knows the popup's moz-extension:// URL.
const UUID = "4d1f6a52-7c3e-4b8a-9a35-6f0e2b1c9d70";
const BASE = `moz-extension://${UUID}`;

const env = await start();
const { tmp, url } = env;

async function launch(name) {
  const dir = path.join(tmp, name);
  mkdirSync(dir, { recursive: true });
  const browser = await puppeteer.launch({
    browser: "firefox",
    executablePath: firefox,
    headless: process.env.HEADED !== "1",
    userDataDir: dir,
    defaultViewport: { width: 1000, height: 800 },
    extraPrefsFirefox: {
      // Every made-up domain goes to the test page server.
      "network.dns.forceResolve": "127.0.0.1",
      "network.dns.disablePrefetch": true,
      "network.proxy.type": 0,
      // The test pages are plain http.
      "dom.security.https_first": false,
      "dom.security.https_first_pbm": false,
      "dom.security.https_first_schemeless": false,
      "dom.security.https_only_mode": false,
      "browser.fixup.alternate.enabled": false,
      "extensions.webextensions.uuids": JSON.stringify({ [ADDON_ID]: UUID }),
      "intl.locale.requested": "en-US",
      "intl.accept_languages": "en-US, en",
    },
  });
  const id = await browser.installExtension(path.join(ext, "dist/firefox"));
  assert.equal(id, ADDON_ID);
  return browser;
}

/** All tabs' ids by URL, asked from an extension page. */
async function tabIdFor(b, target) {
  const p = await b.newPage();
  await p.goto(`${BASE}/popup.html`);
  const want = target.url();
  const id = await p.evaluate(async (u) => (await browser.tabs.query({})).find((t) => t.url === u)?.id, want);
  await p.close();
  assert.ok(id !== undefined, `tab id for ${want}`);
  return id;
}

/** Open the popup as a tab, aimed at the tab showing `target`. */
async function popup(b, target, onDialog = () => {}) {
  const tab = target ? await tabIdFor(b, target) : undefined;
  const p = await b.newPage();
  p.on("dialog", (d) => {
    onDialog(d.message());
    void d.accept();
  });
  await p.goto(`${BASE}/popup.html${tab !== undefined ? `?tab=${tab}` : ""}`);
  await p.waitForSelector("main");
  return p;
}

/** Click the first button (inside `within`, if given) named exactly `text`. */
async function click(p, text, within) {
  await p.waitForFunction(
    (t, w) => {
      const root = w ? document.querySelector(w) : document;
      const btn = [...(root?.querySelectorAll("button, [role=menuitem]") ?? [])].find(
        (b) => (b.getAttribute("aria-label") ?? b.textContent.trim()) === t && !b.disabled,
      );
      if (!btn) return false;
      btn.click();
      return true;
    },
    { timeout: 15000 },
    text,
    within,
  );
}

async function fill(p, sel, v) {
  await p.locator(sel).fill(v);
}

const value = (frame, sel) => frame.$eval(sel, (el) => el.value);
const text = (p, sel) => p.$eval(sel, (el) => el.textContent);
const frameOn = (page, host) => page.frames().find((f) => new URL(f.url()).hostname === host);

async function waitValue(frame, sel) {
  await frame.waitForFunction((s) => document.querySelector(s)?.value !== "", { timeout: 15000 }, sel);
}

let failed = false;
const browsers = [];
try {
  const a = await launch("profile-a");
  browsers.push(a);
  console.log(`e2e: extension in ${await a.version()} against a local amanfold-server`);

  // --- Sign up --------------------------------------------------------------
  let p = await popup(a);
  await click(p, "Create account");
  await fill(p, "#f-email", "firefox@example.com");
  await fill(p, "#f-password", "correct horse battery staple");
  await fill(p, "#f-confirm", "correct horse battery staple");
  await p.click("input[name=understand]");
  await click(p, "Create account");
  await p.waitForSelector("[data-testid=secret-key]", { timeout: 60000 });
  const secretKey = (await text(p, "[data-testid=secret-key]")).trim();
  assert.match(secretKey, /^A1-/);
  assert.equal((await p.$$("[data-testid=recovery-words] li")).length, 24);
  await p.click("input[name=saved]");
  await click(p, "Continue");
  await p.waitForSelector("::-p-text(Your vault is empty)");
  log("sign-up shows the recovery kit, then the empty vault");

  // --- Add a login with a one-time code, and a card ---------------------------
  const site = await a.newPage();
  await site.goto(url("bank.com", "login.html"), { waitUntil: "load" });
  await p.close();
  p = await popup(a, site);
  await click(p, "+ Add");
  await click(p, "Login");
  await p.waitForSelector("#f-url");
  assert.equal(await value(p, "#f-url"), new URL(url("bank.com", "")).origin);
  await fill(p, "#f-title", "Bank");
  await fill(p, "#f-username", "eslam@example.com");
  await fill(p, "#f-password", "s3cret-Pass!");
  await fill(p, "#f-totp", "JBSWY3DPEHPK3PXP");
  await click(p, "Save");
  await p.waitForSelector("[data-testid=matches] li");
  await click(p, "+ Add");
  await click(p, "Card");
  await fill(p, "#f-title", "Visa");
  await fill(p, "#f-cardholder", "Eslam Test");
  await fill(p, "#f-card_number", "4111 1111 1111 1111");
  await fill(p, "#f-exp_month", "7");
  await fill(p, "#f-exp_year", "2028");
  await fill(p, "#f-cvv", "123");
  await click(p, "Save");
  await p.waitForSelector("[data-testid=cards] li");
  log("login and card added; the login defaults to the open site");

  // --- Fill: only visible fields on the right site ----------------------------
  await click(p, "Fill", "[data-testid=matches] li");
  await waitValue(site.mainFrame(), "#pass");
  assert.equal(await value(site, "#user"), "eslam@example.com");
  assert.equal(await value(site, "#pass"), "s3cret-Pass!");
  for (const hidden of ["#decoy-user", "#decoy-pass", "#offscreen", "#covered"]) {
    assert.equal(await value(site, hidden), "", `${hidden} must stay empty`);
  }
  log("fill goes into the visible form only (hidden, off-screen and covered fields stay empty)");
  const same = frameOn(site, "accounts.bank.com");
  const evil = frameOn(site, "evil.com");
  assert.ok(same && evil, "both frames loaded");
  await waitValue(same, "#fpass");
  assert.equal(await value(same, "#fpass"), "s3cret-Pass!");
  assert.equal(await value(evil, "#fpass"), "");
  assert.equal(await value(evil, "#fuser"), "");
  log("same-site frame filled; frame from another site (evil.com) left empty");
  await p.close();

  // --- Look-alike site --------------------------------------------------------
  const fake = await a.newPage();
  await fake.goto(url("bank-secure.com", "login.html"));
  p = await popup(a, fake);
  await p.waitForSelector("[data-testid=lookalike]");
  assert.match(await text(p, "[data-testid=lookalike]"), /bank\.com/);
  assert.equal(await p.$("[data-testid=matches]"), null);
  await p.evaluate(() =>
    [...document.querySelectorAll("[data-testid=all-items] .row-main")].find((r) => r.textContent.includes("Bank")).click(),
  );
  await click(p, "Fill");
  await p.waitForSelector(".toast.err");
  assert.match(await text(p, ".toast"), /imitates/);
  assert.equal(await value(fake, "#pass"), "");
  log("look-alike site bank-secure.com: warning shown, nothing offered, manual fill refused");
  await p.close();
  await fake.close();

  // --- One-time code ----------------------------------------------------------
  const otp = await a.newPage();
  await otp.goto(url("bank.com", "otp.html"));
  p = await popup(a, otp);
  await click(p, "Fill", "[data-testid=matches] li");
  await waitValue(otp.mainFrame(), "#otp");
  assert.match(await value(otp, "#otp"), /^\d{6}$/);
  log("one-time code filled from the login's TOTP secret");
  await p.close();
  await otp.close();

  // --- Card into the page and, after a yes, into the payment frame ------------
  const shop = await a.newPage();
  await shop.goto(url("shop.com", "checkout.html"), { waitUntil: "load" });
  const dialogs = [];
  p = await popup(a, shop, (m) => dialogs.push(m));
  await click(p, "Fill", "[data-testid=cards] li");
  await waitValue(shop.mainFrame(), "#cc-num");
  assert.ok(dialogs.some((m) => m.includes("pay.com")), "asked before filling pay.com");
  assert.equal(await value(shop, "#cc-num"), "4111111111111111");
  assert.equal(await value(shop, "#cc-month"), "07");
  assert.equal(await value(shop, "#cc-year"), "2028");
  const pay = frameOn(shop, "pay.com");
  await waitValue(pay, "#pcvc");
  assert.equal(await value(pay, "#pnum"), "4111111111111111");
  assert.equal(await value(pay, "#pexp"), "07 / 28");
  log("card filled (selects, MM / YY box) and into the pay.com frame only after asking");
  await p.close();
  await shop.close();

  // --- Save prompt ------------------------------------------------------------
  const reg = await a.newPage();
  await reg.goto(url("news.com", "register.html"));
  await reg.locator("#ruser").fill("reader");
  await reg.locator("#rpass").fill("Typed-By-Hand-42");
  await reg.click("#rgo");
  await reg.waitForFunction(() => document.title === "done");
  p = await popup(a, reg);
  await p.waitForSelector("[data-testid=save-prompt]");
  await click(p, "Save", "[data-testid=save-prompt]");
  await p.waitForSelector("[data-testid=matches] li");
  assert.match(await text(p, "[data-testid=matches]"), /reader/);
  log("submitted login offered for saving, saved for news.com");

  // --- Lock and unlock ----------------------------------------------------------
  await click(p, "Lock");
  await p.waitForSelector("#f-password");
  await fill(p, "#f-password", "wrong password here");
  await click(p, "Unlock");
  await p.waitForSelector(".error:not(:empty)");
  assert.match(await text(p, ".error"), /Wrong password/);
  await fill(p, "#f-password", "correct horse battery staple");
  await click(p, "Unlock");
  await p.waitForSelector("[data-testid=matches] li");
  log("lock; wrong password refused; right password unlocks");

  await click(p, "Sync now");
  await p.waitForSelector("::-p-text(Last synced)");
  await p.close();

  // --- Second profile: sign in with the Secret Key, items arrive ----------------
  const b = await launch("profile-b");
  browsers.push(b);
  p = await popup(b);
  await click(p, "Sign in");
  await fill(p, "#f-email", "firefox@example.com");
  await fill(p, "#f-password", "correct horse battery staple");
  await fill(p, "#f-secretKey", secretKey.toLowerCase());
  await click(p, "Sign in");
  await p.waitForSelector("[data-testid=all-items] li", { timeout: 60000 });
  const titles = await p.$$eval("[data-testid=all-items] .row-title", (els) => els.map((e) => e.textContent));
  assert.deepEqual([...titles].sort(), ["Bank", "Visa", "news.com"].sort());
  log("second profile signs in with the Secret Key and receives all three items");

  console.log("e2e (firefox): all passed");
} catch (e) {
  failed = true;
  console.error("e2e (firefox) FAILED at step", currentStep(), e);
  mkdirSync(path.join(ext, "test-results"), { recursive: true });
  for (const [bi, b] of browsers.entries()) {
    for (const [i, pg] of (await b.pages().catch(() => [])).entries()) {
      await pg.screenshot({ path: path.join(ext, `test-results/firefox-fail-${bi}-${i}.png`) }).catch(() => {});
    }
  }
} finally {
  for (const b of browsers) await b.close().catch(() => {});
  env.stop();
}
process.exit(failed ? 1 : 0);
