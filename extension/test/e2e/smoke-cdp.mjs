// A shorter end-to-end test over the raw DevTools protocol, for browsers that
// Playwright can't attach to (Vivaldi: its own interface is made of extension
// pages, and Playwright's connectOverCDP never finishes there).
//
//   E2E_BROWSER=/usr/bin/vivaldi-stable npm run e2e:smoke
//
// Same servers as run.mjs. Covers: the extension loads, sign-up (core and
// OPAQUE against a real server), adding a login, and filling it into a page
// through the content script.

import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import path from "node:path";
import { currentStep, ext, log, start } from "./harness.mjs";

const browserPath = process.env.E2E_BROWSER;
if (!browserPath) throw new Error("set E2E_BROWSER");
const env = await start();
const { tmp, url } = env;

const proc = spawn(
  browserPath,
  [
    `--disable-extensions-except=${path.join(ext, "dist/chrome")}`,
    `--load-extension=${path.join(ext, "dist/chrome")}`,
    "--host-resolver-rules=MAP * 127.0.0.1, EXCLUDE 127.0.0.1",
    "--lang=en-US",
    "--disable-features=DisableLoadExtensionCommandLineSwitch",
    `--user-data-dir=${path.join(tmp, "profile")}`,
    "--remote-debugging-port=0",
    "--no-sandbox",
    "--no-first-run",
    "--disable-component-extensions-with-background-pages",
    "--no-default-browser-check",
    "--window-size=1000,800",
    ...(process.env.HEADED === "1" ? [] : ["--headless=new"]),
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "pipe"] },
);
process.on("exit", () => proc.kill());
let said = "";
proc.stderr.on("data", (d) => (said += d));

const T = 30000;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Minimal CDP client: one WebSocket, flat sessions. */
async function connect() {
  const ws = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("no DevTools address")), 60000);
    const look = () => {
      const m = /DevTools listening on (ws:\/\/\S+)/.exec(said);
      if (m) {
        clearTimeout(timer);
        resolve(m[1]);
      } else setTimeout(look, 100);
    };
    look();
    proc.on("exit", (code) => reject(new Error(`${browserPath} exited (${code})`)));
  });
  const sock = new WebSocket(ws);
  await new Promise((r, j) => {
    sock.onopen = r;
    sock.onerror = j;
  });
  let next = 1;
  const waiting = new Map();
  sock.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    const w = msg.id && waiting.get(msg.id);
    if (!w) return;
    waiting.delete(msg.id);
    if (msg.error) w.reject(new Error(`${w.method}: ${msg.error.message}`));
    else w.resolve(msg.result);
  };
  return (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = next++;
      const timer = setTimeout(() => {
        waiting.delete(id);
        reject(new Error(`${method}: no answer in ${T / 1000} s`));
      }, T);
      waiting.set(id, {
        resolve: (v) => (clearTimeout(timer), resolve(v)),
        reject: (e) => (clearTimeout(timer), reject(e)),
        method,
      });
      sock.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
}

let failed = false;
try {
  console.log(`e2e smoke: extension in ${path.basename(browserPath)} over CDP against a local amanfold-server`);
  const cdp = await connect();

  const until = async (what, f) => {
    const end = Date.now() + T;
    for (;;) {
      const v = await f();
      if (v) return v;
      if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
      await sleep(200);
    }
  };

  const sw = await until("the extension's service worker", async () => {
    const { targetInfos } = await cdp("Target.getTargets");
    return targetInfos.find((t) => t.type === "service_worker" && t.url.endsWith("/background.js"));
  });
  const id = new URL(sw.url).host;
  log("extension loaded");

  /** Open `address` in a new tab and return a page handle. */
  async function open(address) {
    const { targetId } = await cdp("Target.createTarget", { url: address });
    const { sessionId } = await cdp("Target.attachToTarget", { targetId, flatten: true });
    const page = {
      targetId,
      async ev(expr) {
        const r = await cdp("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true }, sessionId);
        if (r.exceptionDetails) throw new Error(`${expr}: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
        return r.result.value;
      },
      async wait(what, expr) {
        try {
          return await until(what, () => page.ev(expr).catch(() => undefined));
        } catch (e) {
          const shown = await page.ev("location.href + '\\n' + document.body?.innerText").catch((x) => x.message);
          throw new Error(`${e.message}; page shows:\n${shown}`);
        }
      },
      async click(text, sel = "button") {
        const js = JSON.stringify(text);
        await page.wait(`"${text}"`, `!!document.querySelector("main") && [...document.querySelectorAll(${JSON.stringify(sel)})].some((b) => b.textContent.trim() === ${js} && !b.disabled)`);
        await page.ev(`[...document.querySelectorAll(${JSON.stringify(sel)})].find((b) => b.textContent.trim() === ${js} && !b.disabled).click()`);
      },
      async fill(sel, value) {
        await page.wait(sel, `!!document.querySelector(${JSON.stringify(sel)})`);
        await page.ev(`(() => {
          const el = document.querySelector(${JSON.stringify(sel)});
          el.value = ${JSON.stringify(value)};
          el.dispatchEvent(new Event("input", { bubbles: true }));
          el.dispatchEvent(new Event("change", { bubbles: true }));
        })()`);
      },
      text: () => page.ev("document.body.innerText"),
      close: () => cdp("Target.closeTarget", { targetId }),
    };
    return page;
  }

  // --- Sign up ----------------------------------------------------------------
  let p = await open(`chrome-extension://${id}/popup.html`);
  await p.click("Create account");
  await p.fill("#f-email", "smoke@example.com");
  await p.fill("#f-password", "correct horse battery staple");
  await p.fill("#f-confirm", "correct horse battery staple");
  await p.ev(`document.querySelector("input[name=understand]").click()`);
  await p.click("Create account");
  const secretKey = await p.wait("the recovery kit", `document.querySelector("[data-testid=secret-key]")?.textContent.trim()`);
  assert.match(secretKey, /^A1-/);
  await p.ev(`document.querySelector("input[name=saved]").click()`);
  await p.click("Continue");
  await p.wait("the empty vault", `document.body.innerText.includes("Your vault is empty")`);
  log("sign-up shows the recovery kit, then the empty vault");

  // --- Add a login for the open site and fill it ------------------------------
  const siteUrl = url("bank.com", "login.html");
  const site = await open(siteUrl);
  await site.wait("the test page", `document.readyState === "complete" && !!document.querySelector("#pass")`);
  const tabId = await p.ev(`chrome.tabs.query({}).then((ts) => ts.find((t) => t.url === ${JSON.stringify(siteUrl)})?.id)`);
  assert.ok(tabId, "tab id");
  await p.close();
  p = await open(`chrome-extension://${id}/popup.html?tab=${tabId}`);
  await p.click("+ Add");
  await p.click("Login", "[role=menuitem]");
  await p.fill("#f-title", "Bank");
  await p.fill("#f-username", "smoke@example.com");
  await p.fill("#f-password", "s3cret-Pass!");
  await p.click("Save");
  await p.wait("the matching login", `!!document.querySelector("[data-testid=matches] li")`);
  log("a login added from the popup defaults to the open site");

  await p.click("Fill");
  await site.wait("the filled password", `document.querySelector("#pass").value === "s3cret-Pass!"`);
  assert.equal(await site.ev(`document.querySelector("#user").value`), "smoke@example.com");
  log("login filled into the page by the content script");

  console.log("e2e smoke: all passed");
} catch (e) {
  failed = true;
  console.error(`e2e smoke FAILED at step ${currentStep()}`, e);
  console.error(`${browserPath} said:\n${said.slice(-3000)}`);
} finally {
  proc.kill();
  await new Promise((r) => (proc.exitCode !== null ? r() : proc.once("exit", r)));
  env.stop();
}
process.exit(failed ? 1 : 0);
