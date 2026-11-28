// End-to-end flow through the WebAssembly bindings (what the browser
// extension and web vault call). Usage: node tests/flow.cjs <pkg dir>
"use strict";
const assert = require("node:assert/strict");
const path = require("node:path");
const a = require(path.resolve(process.argv[2] || "pkg", "amanfold_wasm.js"));

const PW = "تفاحة-حصان-بطارية-دبوس";

function throwsKind(kind, fn) {
  assert.throws(fn, (e) => e instanceof Error && e.name === kind, `expected ${kind}`);
}

const server = new a.TestServer();

// Sign-up and registration.
const created = a.createAccount(PW);
const acct = created.account;
const bundle = created.bundle;
const reg = created.registration;
const record = server.registrationRecord(
  reg.finish(server.registrationResponse(acct.accountId(), reg.request())),
);
throwsKind("WrongStep", () => reg.request());
const me = acct.publicKeys();
assert.ok(me instanceof Uint8Array);

// Vault with a login and a card.
const nv = acct.createVault();
const v1 = nv.vault;
const login = v1.createItem("Login", [
  { name: "title", value: "Email" },
  { name: "password", value: "s3cret!" },
]);
const card = v1.createItem("Card", [{ name: "card_number", value: "4111 1111 1111 1111" }]);
const m1 = v1.signManifest(acct);
throwsKind("InvalidArgument", () => v1.createItem("Spaceship", []));
throwsKind("InvalidArgument", () => v1.createItem("Login", [{ nom: "x" }]));

// Second browser: login, unlock, verified sync.
const s = new a.LoginSession(server.prelogin(bundle), PW, created.secretKey);
const out = s.finish(server.loginResponse(s.accountId(), record, s.start()), server.publicKey());
assert.deepEqual(server.loginFinish(out.finalization), out.sessionKey);
const acct2 = s.unlock(bundle);
const v2 = acct2.openVault(nv.grant, me, undefined);
assert.equal(v2.sync(m1, [login.record, card.record], [me]).length, 0);
const c = v2.item(card.id);
assert.equal(c.kind, "Card");
assert.deepEqual(
  c.fields.find((f) => f.name === "card_number"),
  { name: "card_number", value: "4111111111111111", concealed: true },
);
assert.equal(typeof c.modifiedMs, "number");

// Edit on browser 2; browser 1 syncs; then the server replays.
const e = v2.updateItem(login.id, [{ name: "password", value: "n3w!" }], ["title"]);
const m2 = v2.signManifest(acct2);
v1.sync(m2, [e.record], [me]);
assert.equal(v1.item(login.id).fields.find((f) => f.name === "password").value, "n3w!");
throwsKind("Rollback", () => v1.sync(m1, [login.record], [me]));
// Signed by someone who is not a trusted writer, or garbage keys.
const stranger = a.createAccount("someone else entirely").account.publicKeys();
throwsKind("BadSignature", () => v1.sync(m2, [], [stranger]));
throwsKind("Malformed", () => v1.sync(m2, [], [new Uint8Array(0)]));

// Cache in IndexedDB, reopen after a restart.
const v1b = acct.openVault(nv.grant, me, v1.exportCache());
assert.equal(v1b.items(false).length, 2);
throwsKind("Rollback", () => v1b.sync(m1, [login.record], [me]));
throwsKind("Decrypt", () => acct.openVault(nv.grant, me, new Uint8Array(80)));

// Wrong password.
const bad = new a.LoginSession(server.prelogin(bundle), "wrong password!!", created.secretKey);
throwsKind("Decrypt", () =>
  bad.finish(server.loginResponse(bad.accountId(), record, bad.start()), server.publicKey()),
);

// Tools.
const rules = a.defaultPasswordRules();
assert.equal(a.generatePassword(rules).length, rules.length);
assert.equal(a.generatePassword({ ...rules, length: 32, symbols: false }).length, 32);
assert.equal(
  a.totpAt("otpauth://totp/x?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&digits=8", 59).code,
  "94287082",
);
assert.equal(a.totpNow("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").code.length, 6);
assert.deepEqual(a.cardInfo("٥٥٥٥ ٥٥٥٥ ٥٥٥٥ ٤٤٤٤"), {
  digits: "5555555555554444",
  brand: "Mastercard",
  masked: "•••• 4444",
  luhnValid: true,
});
assert.equal(a.checkSecretKey(created.secretKey.toLowerCase()), created.secretKey);
assert.equal(a.fingerprint(me), acct.fingerprint());

// Autofill matching (browser extension).
const web = v1.createItem("Login", [
  { name: "title", value: "Bank" },
  { name: "url", value: "https://www.bank.com" },
]);
const hits = v1.loginsForUrl("https://login.bank.com/x");
assert.deepEqual(hits.map((h) => [h.item.id, h.strength]), [[web.id, "Site"]]);
assert.deepEqual(v1.loginsForUrl("https://bank.com.evil.io/"), []);
assert.deepEqual(v1.lookalikeSites("https://bank-login.com/"), ["bank.com"]);
assert.equal(a.siteMatch("https://bank.com", "http://bank.com"), "None");
assert.deepEqual(a.pageSite("https://www.bücher.de/"), {
  secure: true,
  host: "www.xn--bcher-kva.de",
  displayHost: "www.bücher.de",
  site: "xn--bcher-kva.de",
});
assert.equal(a.pageSite("javascript:alert(1)"), undefined);

// Quick unlock (the extension keeps the device key in session storage).
const q = acct.enableQuickUnlock();
assert.equal(a.quickUnlock(bundle, q.deviceKey, q.wrappedKeys).accountId(), acct.accountId());
throwsKind("Decrypt", () => a.quickUnlock(bundle, new Uint8Array(32), q.wrappedKeys));

console.log("wasm bindings: all checks passed");
