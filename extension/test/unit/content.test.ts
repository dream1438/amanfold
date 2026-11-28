import { test } from "node:test";
import assert from "node:assert/strict";
import { expiryText, monthCandidates, yearCandidates } from "../../src/content/fill.ts";
import { fromB64, toB64 } from "../../src/shared/b64.ts";
import { ar, en } from "../../src/shared/locales.ts";

const box = (placeholder: string, maxLength = -1) =>
  ({ placeholder, maxLength, getAttribute: () => null }) as unknown as HTMLInputElement;

test("expiry box follows its placeholder or size", () => {
  assert.equal(expiryText(box("MM / YY", 7), "7", "2028"), "07 / 28");
  assert.equal(expiryText(box("MM/YY"), "12", "2030"), "12/30");
  assert.equal(expiryText(box("MM/YYYY"), "1", "28"), "01/2028");
  assert.equal(expiryText(box("", 7), "3", "2029"), "03/2029");
  assert.equal(expiryText(box("", 5), "3", "2029"), "03/29");
  assert.equal(expiryText(box("MM-YY"), "3", "2029"), "03-29");
});

test("month and year options", () => {
  assert.deepEqual(monthCandidates("7").slice(0, 4), ["07", "7", "july", "jul"]);
  assert.ok(monthCandidates("9").includes("سبتمبر"));
  assert.deepEqual(yearCandidates("2028"), ["2028", "28"]);
  assert.deepEqual(yearCandidates("28"), ["2028", "28"]);
});

test("base64url round trip without padding", () => {
  for (const n of [0, 1, 2, 3, 31, 32, 33, 70000]) {
    const b = new Uint8Array(n).map((_, i) => (i * 37) & 255);
    const s = toB64(b);
    assert.doesNotMatch(s, /[+/=]/);
    assert.deepEqual(fromB64(s), b);
  }
  assert.throws(() => fromB64("ab+c"));
});

test("Arabic has every message and the same placeholders", () => {
  for (const k of Object.keys(en) as (keyof typeof en)[]) {
    assert.ok(ar[k], `ar.${k}`);
    const ph = (s: string) => (s.match(/\{\d\}/g) ?? []).sort().join();
    assert.equal(ph(ar[k]), ph(en[k]), `placeholders in ar.${k}`);
  }
});
