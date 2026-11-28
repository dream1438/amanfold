// End-to-end test in a real Firefox, driven by geckodriver (selenium-webdriver):
// the Firefox build (dist/firefox) installed as a temporary add-on, a real
// amanfold-server and the same test pages as the Chromium test. Covers
// sign-up, the autofill rules (hidden fields, frames from other sites,
// look-alike sites), one-time codes, cards in a payment frame, the save
// prompt, lock and unlock, and sign-in on a second profile.
//
//   FIREFOX=/usr/bin/firefox node test/e2e/firefox.mjs
//   E2E_SKIP_BUILD=1 ...     (server and WASM already built)
//
// Needs Firefox 128 or later and geckodriver on the PATH.

import { Builder, By, until } from "selenium-webdriver";
import firefox from "selenium-webdriver/firefox.js";
import path from "node:path";
import { mkdirSync, writeFileSync } from "node:fs";
import assert from "node:assert/strict";
import { currentStep, ext, log, start } from "./harness.mjs";

const ADDON_ID = "extension@amanfold.com";
// A fixed internal UUID, so the test knows the popup's moz-extension:// URL.
const UUID = "4d1f6a52-7c3e-4b8a-9a35-6f0e2b1c9d70";
const POPUP = `moz-extension://${UUID}/popup.html`;
const T = 20000;

const env = await start();
const { url } = env;

async function launch() {
  const o = new firefox.Options();
  if (process.env.FIREFOX) o.setBinary(process.env.FIREFOX);
  if (process.env.HEADED !== "1") o.addArguments("-headless");
  o.windowSize({ width: 1000, height: 800 });
  const prefs = {
    // Every made-up domain goes to the test page server.
    "network.dns.forceResolve": "127.0.0.1",
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
  };
  for (const [k, v] of Object.entries(prefs)) o.setPreference(k, v);
  // Lets the test open the popup page in a tab (see `tab`). geckodriver
  // passes it on to Firefox as -remote-allow-system-access.
  const service = new firefox.ServiceBuilder().addArguments("--allow-system-access");
  const d = await new Builder().forBrowser("firefox").setFirefoxOptions(o).setFirefoxService(service).build();
  const id = await d.installAddon(path.join(ext, "dist/firefox"), true);
  assert.equal(id, ADDON_ID);
  return d;
}

/** Open a new tab at `address` and return its window handle. */
async function tab(d, address) {
  if (!address.startsWith("moz-extension:")) {
    await d.switchTo().newWindow("tab");
    await d.get(address);
    return d.getWindowHandle();
  }
  // WebDriver may not navigate to extension pages, so the browser window
  // opens the tab itself (needs geckodriver --allow-system-access).
  const before = new Set(await d.getAllWindowHandles());
  await d.setContext(firefox.Context.CHROME);
  try {
    await d.executeScript((u) => {
      const w = Services.wm.getMostRecentWindow("navigator:browser");
      w.gBrowser.selectedTab = w.gBrowser.addTab(u, {
        triggeringPrincipal: Services.scriptSecurityManager.getSystemPrincipal(),
      });
    }, address);
  } finally {
    await d.setContext(firefox.Context.CONTENT);
  }
  let handle;
  await d.wait(async () => {
    handle = (await d.getAllWindowHandles()).find((h) => !before.has(h));
    return !!handle;
  }, T, "new tab");
  await d.switchTo().window(handle);
  return handle;
}

/** Open the popup in a new tab, aimed at the page `target` (an address). */
async function popup(d, target) {
  const h = await tab(d, target ? `${POPUP}?tabUrl=${encodeURIComponent(target)}` : POPUP);
  await d.wait(until.elementLocated(By.css("main")), T);
  return h;
}

/** Click the first enabled button (inside `within`) named exactly `text`. */
async function click(d, text, within) {
  await d.wait(
    () =>
      d.executeScript(
        (t, w) => {
          const root = w ? document.querySelector(w) : document;
          const btn = [...(root?.querySelectorAll("button, [role=menuitem]") ?? [])].find(
            (b) => (b.getAttribute("aria-label") ?? b.textContent.trim()) === t && !b.disabled,
          );
          if (!btn) return false;
          btn.click();
          return true;
        },
        text,
        within ?? null,
      ),
    T,
    `button ${text}`,
  );
}

async function fill(d, sel, v) {
  const el = await d.wait(until.elementLocated(By.css(sel)), T);
  await el.clear();
  await el.sendKeys(v);
}

const value = (d, sel) => d.findElement(By.css(sel)).getAttribute("value");
const text = (d, sel) => d.findElement(By.css(sel)).getText();
const waitFor = (d, sel) => d.wait(until.elementLocated(By.css(sel)), T, sel);
const waitText = (d, s) =>
  d.wait(() => d.executeScript((x) => document.body.textContent.includes(x), s), T, `text ${s}`);
const waitValue = (d, sel) =>
  d.wait(() => d.executeScript((s) => !!document.querySelector(s)?.value, sel), T, `value in ${sel}`);

/** Run `fn` inside the iframe `frameSel` of the current page. */
async function inFrame(d, frameSel, fn) {
  await d.switchTo().frame(await d.findElement(By.css(frameSel)));
  try {
    return await fn();
  } finally {
    await d.switchTo().defaultContent();
  }
}

let failed = false;
const drivers = [];
try {
  const a = await launch();
  drivers.push(a);
  const caps = await a.getCapabilities();
  console.log(`e2e: extension in Firefox ${caps.get("browserVersion")} against a local amanfold-server`);

  // --- Sign up --------------------------------------------------------------
  await popup(a);
  await click(a, "Create account");
  await fill(a, "#f-email", "firefox@example.com");
  await fill(a, "#f-password", "correct horse battery staple");
  await fill(a, "#f-confirm", "correct horse battery staple");
  await a.findElement(By.css("input[name=understand]")).click();
  await click(a, "Create account");
  await a.wait(until.elementLocated(By.css("[data-testid=secret-key]")), 60000);
  const secretKey = (await text(a, "[data-testid=secret-key]")).trim();
  assert.match(secretKey, /^A1-/);
  assert.equal((await a.findElements(By.css("[data-testid=recovery-words] li"))).length, 24);
  await a.findElement(By.css("input[name=saved]")).click();
  await click(a, "Continue");
  await waitText(a, "Your vault is empty");
  await a.close();
  log("sign-up shows the recovery kit, then the empty vault");

  // --- Add a login with a one-time code, and a card ---------------------------
  const bank = url("bank.com", "login.html");
  const [first] = await a.getAllWindowHandles();
  await a.switchTo().window(first);
  const site = await tab(a, bank);
  let p = await popup(a, bank);
  await click(a, "+ Add");
  await click(a, "Login");
  await waitFor(a, "#f-url");
  assert.equal(await value(a, "#f-url"), new URL(bank).origin);
  await fill(a, "#f-title", "Bank");
  await fill(a, "#f-username", "eslam@example.com");
  await fill(a, "#f-password", "s3cret-Pass!");
  await fill(a, "#f-totp", "JBSWY3DPEHPK3PXP");
  await click(a, "Save");
  await waitFor(a, "[data-testid=matches] li");
  await click(a, "+ Add");
  await click(a, "Card");
  await fill(a, "#f-title", "Visa");
  await fill(a, "#f-cardholder", "Eslam Test");
  await fill(a, "#f-card_number", "4111 1111 1111 1111");
  await fill(a, "#f-exp_month", "7");
  await fill(a, "#f-exp_year", "2028");
  await fill(a, "#f-cvv", "123");
  await click(a, "Save");
  await waitFor(a, "[data-testid=cards] li");
  log("login and card added; the login defaults to the open site");

  // --- Fill: only visible fields on the right site ----------------------------
  await click(a, "Fill", "[data-testid=matches] li");
  await a.switchTo().window(site);
  await waitValue(a, "#pass");
  assert.equal(await value(a, "#user"), "eslam@example.com");
  assert.equal(await value(a, "#pass"), "s3cret-Pass!");
  for (const hidden of ["#decoy-user", "#decoy-pass", "#offscreen", "#covered"]) {
    assert.equal(await value(a, hidden), "", `${hidden} must stay empty`);
  }
  log("fill goes into the visible form only (hidden, off-screen and covered fields stay empty)");
  await inFrame(a, "#same", async () => {
    await waitValue(a, "#fpass");
    assert.equal(await value(a, "#fpass"), "s3cret-Pass!");
  });
  await inFrame(a, "#evil", async () => {
    assert.equal(await value(a, "#fpass"), "");
    assert.equal(await value(a, "#fuser"), "");
  });
  log("same-site frame filled; frame from another site (evil.com) left empty");
  await a.switchTo().window(p);
  await a.close();

  // --- Look-alike site --------------------------------------------------------
  const fakeUrl = url("bank-secure.com", "login.html");
  await a.switchTo().window(site);
  const fake = await tab(a, fakeUrl);
  p = await popup(a, fakeUrl);
  await waitFor(a, "[data-testid=lookalike]");
  assert.match(await text(a, "[data-testid=lookalike]"), /bank\.com/);
  assert.equal((await a.findElements(By.css("[data-testid=matches]"))).length, 0);
  await a.executeScript(() =>
    [...document.querySelectorAll("[data-testid=all-items] .row-main")].find((r) => r.textContent.includes("Bank")).click(),
  );
  await click(a, "Fill");
  // "Fill on a site this login isn't for?": yes, and it's still refused.
  const anySite = await a.wait(until.alertIsPresent(), T);
  await anySite.accept();
  await waitFor(a, ".toast.err");
  assert.match(await text(a, ".toast"), /imitates/);
  await a.close();
  await a.switchTo().window(fake);
  assert.equal(await value(a, "#pass"), "");
  await a.close();
  log("look-alike site bank-secure.com: warning shown, nothing offered, manual fill refused");

  // --- One-time code ----------------------------------------------------------
  const otpUrl = url("bank.com", "otp.html");
  await a.switchTo().window(site);
  const otp = await tab(a, otpUrl);
  p = await popup(a, otpUrl);
  await click(a, "Fill", "[data-testid=matches] li");
  await a.switchTo().window(otp);
  await waitValue(a, "#otp");
  assert.match(await value(a, "#otp"), /^\d{6}$/);
  log("one-time code filled from the login's TOTP secret");
  await a.close();
  await a.switchTo().window(p);
  await a.close();

  // --- Card into the page and, after a yes, into the payment frame ------------
  const shopUrl = url("shop.com", "checkout.html");
  await a.switchTo().window(site);
  const shop = await tab(a, shopUrl);
  p = await popup(a, shopUrl);
  await click(a, "Fill", "[data-testid=cards] li");
  const alert = await a.wait(until.alertIsPresent(), T);
  assert.match(await alert.getText(), /pay\.com/);
  await alert.accept();
  await a.switchTo().window(shop);
  await waitValue(a, "#cc-num");
  assert.equal(await value(a, "#cc-num"), "4111111111111111");
  assert.equal(await value(a, "#cc-month"), "07");
  assert.equal(await value(a, "#cc-year"), "2028");
  await inFrame(a, "#pay", async () => {
    await waitValue(a, "#pcvc");
    assert.equal(await value(a, "#pnum"), "4111111111111111");
    assert.equal(await value(a, "#pexp"), "07 / 28");
  });
  log("card filled (selects, MM / YY box) and into the pay.com frame only after asking");
  await a.close();
  await a.switchTo().window(p);
  await a.close();

  // --- Save prompt ------------------------------------------------------------
  const regUrl = url("news.com", "register.html");
  await a.switchTo().window(site);
  await tab(a, regUrl);
  await fill(a, "#ruser", "reader");
  await fill(a, "#rpass", "Typed-By-Hand-42");
  await a.findElement(By.css("#rgo")).click();
  await a.wait(until.titleIs("done"), T);
  p = await popup(a, regUrl);
  await waitFor(a, "[data-testid=save-prompt]");
  await click(a, "Save", "[data-testid=save-prompt]");
  await waitFor(a, "[data-testid=matches] li");
  assert.match(await text(a, "[data-testid=matches]"), /reader/);
  log("submitted login offered for saving, saved for news.com");

  // --- Lock and unlock ----------------------------------------------------------
  await click(a, "Lock");
  await waitFor(a, "#f-password");
  await fill(a, "#f-password", "wrong password here");
  await click(a, "Unlock");
  await a.wait(() => a.executeScript(() => !!document.querySelector(".error")?.textContent), T);
  assert.match(await text(a, ".error"), /Wrong password/);
  await fill(a, "#f-password", "correct horse battery staple");
  await click(a, "Unlock");
  await waitFor(a, "[data-testid=matches] li");
  log("lock; wrong password refused; right password unlocks");

  await click(a, "Settings");
  await click(a, "Sync now");
  await waitText(a, "Last synced");

  // --- Second profile: sign in with the Secret Key, items arrive ----------------
  const b = await launch();
  drivers.push(b);
  await popup(b);
  await click(b, "Sign in");
  await fill(b, "#f-email", "firefox@example.com");
  await fill(b, "#f-password", "correct horse battery staple");
  await fill(b, "#f-secretKey", secretKey.toLowerCase());
  await click(b, "Sign in");
  await b.wait(until.elementLocated(By.css("[data-testid=all-items] li")), 60000);
  const titles = await b.executeScript(() =>
    [...document.querySelectorAll("[data-testid=all-items] .row-title")].map((e) => e.firstChild.textContent),
  );
  assert.deepEqual([...titles].sort(), ["Bank", "Visa", "news.com"].sort());
  log("second profile signs in with the Secret Key and receives all three items");

  console.log("e2e (firefox): all passed");
} catch (e) {
  failed = true;
  console.error("e2e (firefox) FAILED at step", currentStep(), e);
  mkdirSync(path.join(ext, "test-results"), { recursive: true });
  for (const [i, d] of drivers.entries()) {
    try {
      writeFileSync(path.join(ext, `test-results/firefox-fail-${i}.png`), await d.takeScreenshot(), "base64");
      console.error("page:", (await d.executeScript(() => document.body?.innerText ?? "")).slice(0, 2000));
    } catch {}
  }
} finally {
  for (const d of drivers) await d.quit().catch(() => {});
  env.stop();
}
process.exit(failed ? 1 : 0);
