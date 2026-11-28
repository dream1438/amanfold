// End-to-end test: a real Chromium with the built extension, a real
// amanfold-server, and test pages on made-up domains (all resolved to
// 127.0.0.1). Checks sign-up, recovery kit, autofill rules (hidden fields,
// frames from other sites, look-alike sites), cards, one-time codes, the
// save prompt, the service worker being stopped, Arabic, and sign-in on a
// second browser.
//
//   node test/e2e/run.mjs            (builds server and extension first)
//   E2E_SKIP_BUILD=1 node test/e2e/run.mjs
//
// Needs: cargo, wasm-bindgen 0.2.130, Playwright's Chromium.
//   E2E_BROWSER=/usr/bin/brave-browser node test/e2e/run.mjs   (another Chromium-based browser)

import { chromium } from "playwright";
import { build } from "esbuild";
import path from "node:path";
import assert from "node:assert/strict";
import { currentStep, ext, log, start } from "./harness.mjs";

const headless = process.env.HEADED !== "1";
// E2E_BROWSER=/path/to/brave (or Edge, Opera, Vivaldi) runs the same test in
// another Chromium-based browser instead of Playwright's Chromium.
const browserPath = process.env.E2E_BROWSER;
const env = await start();
const { tmp, url } = env;

async function launch(dir) {
  const ctx = await chromium.launchPersistentContext(dir, {
    ...(browserPath ? { executablePath: browserPath } : { channel: "chromium" }),
    headless,
    args: [
      `--disable-extensions-except=${path.join(ext, "dist/chrome")}`,
      `--load-extension=${path.join(ext, "dist/chrome")}`,
      // Every made-up domain goes to the fixture server.
      "--host-resolver-rules=MAP * 127.0.0.1, EXCLUDE 127.0.0.1",
      "--lang=en-US",
      // Branded builds (Chrome 137+, and some others) ignore --load-extension without this.
      "--disable-features=DisableLoadExtensionCommandLineSwitch",
    ],
    viewport: { width: 1000, height: 800 },
  });
  let [sw] = ctx.serviceWorkers();
  sw ??= await ctx.waitForEvent("serviceworker");
  const id = new URL(sw.url()).host;
  return { ctx, id };
}

/** Open the popup as a tab, aimed at the tab showing `target`. */
async function popup(b, target) {
  const p = await b.ctx.newPage();
  await p.goto(`chrome-extension://${b.id}/popup.html`);
  let tabId;
  if (target) {
    tabId = await p.evaluate(async (u) => {
      const tabs = await chrome.tabs.query({});
      return tabs.find((t) => t.url === u)?.id;
    }, target.url());
    assert.ok(tabId, "tab id");
    await p.goto(`chrome-extension://${b.id}/popup.html?tab=${tabId}`);
  }
  p.on("dialog", (d) => void d.accept());
  await p.waitForSelector("main");
  return p;
}

async function click(p, text) {
  await p.getByRole("button", { name: text, exact: true }).first().click();
}

// SCREENSHOTS=dir saves popup screenshots for review.
const shots = process.env.SCREENSHOTS;
async function snap(p, name) {
  if (!shots) return;
  await p.setViewportSize({ width: 380, height: 600 });
  await p.screenshot({ path: path.join(shots, `${name}.png`) });
}

const value = (page, sel, frame) => (frame ?? page).locator(sel).inputValue();

let failed = false;
const browsers = [];
try {
  console.log(`e2e: extension in ${browserPath ? path.basename(browserPath) : "Chromium"} against a local amanfold-server`);
  const a = await launch(path.join(tmp, "profile-a"));
  browsers.push(a);

  // --- Field detection on tricky forms ------------------------------------------
  const det = await build({
    entryPoints: [path.join(ext, "src/content/detect.ts")],
    bundle: true,
    write: false,
    format: "iife",
    globalName: "AmanfoldDetect",
  });
  const dp = await a.ctx.newPage();
  await dp.goto(url("cases.com", "detect.html"));
  await dp.addScriptTag({ content: det.outputFiles[0].text });
  const wrong = await dp.evaluate(() => {
    const found = new Map(AmanfoldDetect.detect().map((f) => [f.el, f.role]));
    const all = [...document.querySelectorAll("[data-expect]"), ...document.getElementById("host").shadowRoot.querySelectorAll("[data-expect]")];
    return all
      .map((el) => ({ el: el.id || el.name, want: el.dataset.expect, got: found.get(el) ?? "none" }))
      .filter((x) => x.want !== x.got);
  });
  assert.deepEqual(wrong, []);
  await dp.close();
  log("field detection: usernames, new passwords, Arabic labels, one-time codes, shadow DOM");

  // --- Sign up --------------------------------------------------------------
  let p = await popup(a);
  await click(p, "Create account");
  await p.fill("#f-email", "e2e@example.com");
  await p.fill("#f-password", "correct horse battery staple");
  await p.fill("#f-confirm", "correct horse battery staple");
  await p.check("input[name=understand]");
  await click(p, "Create account");
  await p.waitForSelector("[data-testid=secret-key]", { timeout: 30000 });
  const secretKey = (await p.textContent("[data-testid=secret-key]")).trim();
  const words = await p.locator("[data-testid=recovery-words] li").count();
  assert.match(secretKey, /^A1-/);
  assert.equal(words, 24);
  assert.ok(await p.getByRole("button", { name: "Continue" }).isDisabled());
  await p.check("input[name=saved]");
  await click(p, "Continue");
  await p.waitForSelector("text=Your vault is empty");
  log("sign-up shows the recovery kit once, then the empty vault");

  // --- Add a login with a one-time code, and a card ---------------------------
  const site = await a.ctx.newPage();
  await site.goto(url("bank.com", "login.html"), { waitUntil: "load" });
  await p.close();
  p = await popup(a, site);
  await click(p, "+ Add");
  await p.getByRole("menuitem", { name: "Login" }).click();
  assert.equal(await p.inputValue("#f-url"), new URL(url("bank.com", "")).origin);
  await p.fill("#f-title", "Bank");
  await p.fill("#f-username", "eslam@example.com");
  await p.fill("#f-password", "s3cret-Pass!");
  await p.fill("#f-totp", "JBSWY3DPEHPK3PXP");
  await click(p, "Save");
  await p.waitForSelector("[data-testid=matches] li");
  log("a login added from the popup defaults to the open site");

  await click(p, "+ Add");
  await p.getByRole("menuitem", { name: "Card" }).click();
  await p.fill("#f-title", "Visa");
  await p.fill("#f-cardholder", "Eslam Test");
  await p.fill("#f-card_number", "4111 1111 1111 1111");
  await p.fill("#f-exp_month", "7");
  await p.fill("#f-exp_year", "2028");
  await p.fill("#f-cvv", "123");
  await click(p, "Save");
  await p.waitForSelector("[data-testid=cards] li");
  await snap(p, "1-site-matches");

  // --- Fill: only visible fields on the right site ----------------------------
  await p.locator("[data-testid=matches] li").first().getByRole("button", { name: "Fill" }).click();
  await site.waitForFunction(() => document.querySelector("#pass").value !== "");
  assert.equal(await value(site, "#user"), "eslam@example.com");
  assert.equal(await value(site, "#pass"), "s3cret-Pass!");
  for (const hidden of ["#decoy-user", "#decoy-pass", "#offscreen", "#covered"]) {
    assert.equal(await value(site, hidden), "", `${hidden} must stay empty`);
  }
  log("fill goes into the visible form only (hidden, off-screen and covered fields stay empty)");
  // Frames are filled one after another; wait for the same-site frame.
  await site
    .frame({ url: /accounts\.bank\.com/ })
    .waitForFunction(() => document.querySelector("#fpass").value !== "", null, { timeout: 5000 })
    .catch(async (e) => {
      console.error("toast:", await p.locator(".toast").textContent().catch(() => "none"));
      throw e;
    });
  const same = site.frameLocator("#same");
  const evil = site.frameLocator("#evil");
  assert.equal(await same.locator("#fpass").inputValue(), "s3cret-Pass!");
  assert.equal(await evil.locator("#fpass").inputValue(), "");
  assert.equal(await evil.locator("#fuser").inputValue(), "");
  log("same-site frame filled; frame from another site (evil.com) left empty");

  // --- Look-alike site --------------------------------------------------------
  await p.close();
  const fake = await a.ctx.newPage();
  await fake.goto(url("bank-secure.com", "login.html"));
  p = await popup(a, fake);
  await p.waitForSelector("[data-testid=lookalike]");
  assert.match(await p.textContent("[data-testid=lookalike]"), /bank\.com/);
  await snap(p, "2-lookalike-warning");
  assert.equal(await p.locator("[data-testid=matches]").count(), 0);
  // Even choosing the login by hand is refused there.
  await p.locator("[data-testid=all-items] .row-main", { hasText: "Bank" }).click();
  await click(p, "Fill");
  await p.waitForSelector(".toast.err");
  assert.match(await p.textContent(".toast"), /imitates/);
  assert.equal(await value(fake, "#pass"), "");
  log("look-alike site bank-secure.com: warning shown, nothing offered, manual fill refused");
  await p.close();
  await fake.close();

  // --- One-time code ----------------------------------------------------------
  const otp = await a.ctx.newPage();
  await otp.goto(url("bank.com", "otp.html"));
  p = await popup(a, otp);
  await p.locator("[data-testid=matches] li").first().getByRole("button", { name: "Fill" }).click();
  await otp.waitForFunction(() => document.querySelector("#otp").value !== "");
  assert.match(await value(otp, "#otp"), /^\d{6}$/);
  log("one-time code filled from the login's TOTP secret");
  await p.close();
  await otp.close();

  // --- Card into the page and, after a yes, into the payment frame ------------
  const shop = await a.ctx.newPage();
  await shop.goto(url("shop.com", "checkout.html"), { waitUntil: "load" });
  p = await popup(a, shop);
  const dialogs = [];
  p.on("dialog", (d) => dialogs.push(d.message()));
  await p.locator("[data-testid=cards] li").first().getByRole("button", { name: "Fill" }).click();
  await shop.waitForFunction(() => document.querySelector("#cc-num").value !== "");
  assert.ok(dialogs.some((m) => m.includes("pay.com")), "asked before filling pay.com");
  assert.equal(await value(shop, "#cc-num"), "4111111111111111");
  assert.equal(await value(shop, "#cc-name"), "Eslam Test");
  assert.equal(await value(shop, "#cc-month"), "07");
  assert.equal(await value(shop, "#cc-year"), "2028");
  assert.equal(await value(shop, "#cc-cvv"), "123");
  await shop
    .frame({ url: /pay\.com/ })
    .waitForFunction(() => document.querySelector("#pcvc").value !== "", null, { timeout: 5000 });
  const pay = shop.frameLocator("#pay");
  assert.equal(await pay.locator("#pnum").inputValue(), "4111111111111111");
  assert.equal(await pay.locator("#pexp").inputValue(), "07 / 28");
  assert.equal(await pay.locator("#pcvc").inputValue(), "123");
  log("card filled (selects, MM / YY box) and into the pay.com frame only after asking");
  await p.close();
  await shop.close();

  // --- Save prompt ------------------------------------------------------------
  const reg = await a.ctx.newPage();
  await reg.goto(url("news.com", "register.html"));
  await reg.fill("#ruser", "reader");
  await reg.fill("#rpass", "Typed-By-Hand-42");
  await reg.click("#rgo");
  await reg.waitForFunction(() => document.title === "done");
  p = await popup(a, reg);
  await p.waitForSelector("[data-testid=save-prompt]");
  await snap(p, "3-save-prompt");
  await p.locator("[data-testid=save-prompt]").getByRole("button", { name: "Save" }).click();
  await p.waitForSelector("[data-testid=matches] li");
  assert.match(await p.textContent("[data-testid=matches]"), /reader/);
  log("submitted login offered for saving, saved for news.com");
  await p.close();

  // --- Service worker stopped: still unlocked, data intact ---------------------
  const cdpPage = await a.ctx.newPage();
  const cdp = await a.ctx.newCDPSession(cdpPage);
  await cdp.send("ServiceWorker.enable");
  await cdp.send("ServiceWorker.stopAllWorkers");
  await new Promise((r) => setTimeout(r, 500));
  p = await popup(a, site);
  await p.waitForSelector("[data-testid=matches] li", { timeout: 15000 });
  log("after the browser stops the service worker, the vault reopens without the password");
  await cdpPage.close();

  // --- Lock and unlock ----------------------------------------------------------
  await p.getByRole("button", { name: "Lock" }).click();
  await p.waitForSelector("#f-password");
  await p.fill("#f-password", "wrong password here");
  await click(p, "Unlock");
  await p.waitForSelector(".error:not(:empty)");
  assert.match(await p.textContent(".error"), /Wrong password/);
  await p.fill("#f-password", "correct horse battery staple");
  await click(p, "Unlock");
  await p.waitForSelector("[data-testid=matches] li");
  log("lock; wrong password refused; right password unlocks");

  // --- Arabic -------------------------------------------------------------------
  await p.getByRole("button", { name: "Settings" }).click();
  await p.selectOption("#f-lang", "ar");
  await p.waitForFunction(() => document.documentElement.dir === "rtl");
  assert.equal(await p.evaluate(() => document.documentElement.lang), "ar");
  assert.ok(await p.getByText("الإعدادات").first().isVisible());
  if (shots) {
    await p.getByRole("button", { name: "رجوع" }).click();
    await p.waitForSelector("[data-testid=matches] li");
    await snap(p, "4-arabic");
    await p.getByRole("button", { name: "الإعدادات" }).click();
    await p.waitForSelector("#f-lang");
  }
  await p.selectOption("#f-lang", "en");
  await p.waitForFunction(() => document.documentElement.dir === "ltr");
  log("Arabic interface switches the popup to right-to-left");

  // Make sure everything reached the server before the second browser signs in.
  await click(p, "Sync now");
  await p.waitForSelector("text=Last synced");
  await p.close();

  // --- Second browser: sign in with the Secret Key, items arrive -----------------
  const b = await launch(path.join(tmp, "profile-b"));
  browsers.push(b);
  p = await popup(b);
  await click(p, "Sign in");
  await p.fill("#f-email", "e2e@example.com");
  await p.fill("#f-password", "correct horse battery staple");
  await p.fill("#f-secretKey", secretKey.toLowerCase());
  await click(p, "Sign in");
  await p.waitForSelector("[data-testid=all-items] li", { timeout: 30000 });
  const titles = await p.locator("[data-testid=all-items] .row-title").allTextContents();
  assert.deepEqual([...titles].sort(), ["Bank", "Visa", "news.com"].sort());
  log("second browser signs in with the Secret Key and receives all three items");

  console.log("e2e: all passed");
} catch (e) {
  failed = true;
  console.error("e2e FAILED at step", currentStep(), e);
  for (const b of browsers) {
    for (const [i, pg] of b.ctx.pages().entries()) {
      await pg.screenshot({ path: path.join(ext, `test-results/fail-${browsers.indexOf(b)}-${i}.png`) }).catch(() => {});
    }
  }
} finally {
  for (const b of browsers) await b.ctx.close().catch(() => {});
  env.stop();
}
process.exit(failed ? 1 : 0);
