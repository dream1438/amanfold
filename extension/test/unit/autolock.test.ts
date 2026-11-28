// Regression test: alarms, badges and tab events must not count as activity,
// so an idle unlocked vault locks after the auto-lock time. (Before the fix
// the 5-minute sync alarm reset the idle timer and the vault never locked.)
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import init, * as core from "../../pkg/amanfold_wasm.js";
import { toB64 } from "../../src/shared/b64.ts";

let now = Date.UTC(2026, 0, 1, 10, 0, 0);
const RealDate = Date;
class FakeDate extends RealDate {
  constructor(...a: unknown[]) {
    super(...((a.length ? a : [now]) as []));
  }
  static override now() {
    return now;
  }
}
const g = globalThis as Record<string, unknown>;
g.Date = FakeDate;
g.__AMANFOLD_VERSION__ = "test";
g.__AMANFOLD_TARGET__ = "chrome";
g.__AMANFOLD_SERVER__ = "http://127.0.0.1:1";
const wasm = await readFile(new URL("../../pkg/amanfold_wasm_bg.wasm", import.meta.url));
g.fetch = async (url: unknown) => {
  if (String(url).endsWith("core.wasm")) return new Response(wasm, { headers: { "content-type": "application/wasm" } });
  throw new Error("offline");
};

// Just enough IndexedDB for store.ts: one object store of key-value pairs.
const kv = new Map<string, unknown>();
const later = (f: () => void) => setTimeout(f, 0);
g.indexedDB = {
  open() {
    const req: Record<string, unknown> = {};
    const db = {
      createObjectStore() {},
      transaction() {
        const tx: Record<string, unknown> = {};
        const store = {
          get: (k: string) => op(() => structuredClone(kv.get(k))),
          put: (v: unknown, k: string) => op(() => void kv.set(k, structuredClone(v))),
          delete: (k: string) => op(() => void kv.delete(k)),
        };
        const op = (f: () => unknown) => {
          const r: Record<string, unknown> = {};
          later(() => {
            r.result = f();
            (tx.oncomplete as () => void)?.();
          });
          return r;
        };
        tx.objectStore = () => store;
        return tx;
      },
    };
    later(() => {
      req.result = db;
      (req.onupgradeneeded as () => void)?.();
      (req.onsuccess as () => void)?.();
    });
    return req;
  },
};

const area = () => {
  const d: Record<string, unknown> = {};
  return {
    d,
    async get(k: string | string[]) {
      const ks = Array.isArray(k) ? k : [k];
      return Object.fromEntries(ks.filter((x) => x in d).map((x) => [x, d[x]]));
    },
    async set(o: Record<string, unknown>) {
      Object.assign(d, structuredClone(o));
    },
    async remove(k: string | string[]) {
      for (const x of Array.isArray(k) ? k : [k]) delete d[x];
    },
    async clear() {
      for (const k in d) delete d[k];
    },
    onChanged: { addListener() {} },
    setAccessLevel: async () => {},
  };
};
const local = area();
const session = area();
let alarm: (a: { name: string }) => void = () => {};
let tabUpdated: (id: number, info: { status?: string; url?: string }) => void = () => {};
const noop = { addListener() {} };
g.chrome = {
  runtime: { id: "x", getURL: (p: string) => "chrome-extension://x/" + p, onMessage: noop },
  storage: { local, session },
  alarms: { create() {}, onAlarm: { addListener: (f: typeof alarm) => (alarm = f) } },
  tabs: {
    onActivated: noop,
    onUpdated: { addListener: (f: typeof tabUpdated) => (tabUpdated = f) },
    onRemoved: noop,
    query: async () => [],
    get: async () => ({ url: "https://example.com/" }),
  },
  commands: { onCommand: noop },
  action: { setBadgeText: async () => {}, setBadgeBackgroundColor: async () => {} },
};

const tick = () => new Promise((r) => setTimeout(r, 5));

test("an idle vault locks even while alarms and pages keep running", async () => {
  // A real account and vault, unlocked with the device key as after a
  // service-worker restart.
  await init({ module_or_path: wasm });
  const created = core.createAccount("correct horse battery staple");
  const nv = created.account.createVault();
  const q = created.account.enableQuickUnlock();
  await import("../../src/background/index.ts");
  // Data from an older version, in storage.local: moved to IndexedDB.
  local.d.account = {
    server: "http://127.0.0.1:1",
    email: "a@example.com",
    accountId: created.account.accountId(),
    bundle: toB64(created.bundle),
    secretKey: created.secretKey,
    grant: toB64(nv.grant),
    pending: {},
    seq: 0,
    token: "t",
    vaultId: nv.vault.id(),
  };
  local.d.settings = { lang: null, autoLockMinutes: 15, server: "x" };
  session.d.quick = { deviceKey: toB64(q.deviceKey), wrappedKeys: toB64(q.wrappedKeys), lastActiveMs: now };
  for (let min = 1; min <= 20; min++) {
    now += 60_000;
    tabUpdated(1, { status: "complete" }); // a page navigating keeps firing events
    alarm({ name: "tick" });
    await tick();
    if (min === 10) assert.ok(session.d.quick, "still unlocked after 10 minutes");
  }
  assert.equal(session.d.quick, undefined, "vault should be locked after 20 idle minutes with a 15-minute auto-lock");
  assert.equal(local.d.account, undefined, "account must not stay in storage.local, which content scripts can read");
  assert.ok(kv.get("account"), "account moved to the extension's IndexedDB");
});
