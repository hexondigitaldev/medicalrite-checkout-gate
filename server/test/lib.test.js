import test from "node:test";
import assert from "node:assert/strict";
import {
  windowOf, windowKey, signToken, cartContent, contentValid, keysetValue, signingWindow,
  verifyProxySignature, verifyIdToken, clientIp, hmacHex, safeEqual,
} from "../src/lib.js";

// Shared vector — the same values are asserted in extensions/checkout-rule/src/token.rs.
const MASTER = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const SHOP = "medicalrite-dev.myshopify.com";
const W = 995432;
const KEY = "51d5e2631986a6e7ddf19c718d6f8eba8ef5c1ab511299424ff77d454fba0c99";
const CONTENT = "41234567890:2,49876543210:1";

test("shared vector: window key and tokens match the checkout rule", async () => {
  assert.equal(await windowKey(MASTER, SHOP, W), KEY);
  assert.equal(await signToken(KEY, W, "h", CONTENT), "1.995432.h.fe2646951b5fe6da175a3580ffebdd1f");
  assert.equal(await signToken(KEY, W, "s", CONTENT), "1.995432.s.48726d1a28a056f22f361509aff6f5b2");
  await assert.rejects(signToken(KEY, W, "x", CONTENT));
});

test("keys differ per shop and per window", async () => {
  assert.notEqual(await windowKey(MASTER, "other.myshopify.com", W), KEY);
  assert.notEqual(await windowKey(MASTER, SHOP, W + 1), KEY);
});

test("cart content: summed, sorted numerically, zero dropped", () => {
  const items = [
    { variant_id: 49876543210, quantity: 1 },
    { variant_id: 41234567890, quantity: 1 },
    { variant_id: 41234567890, quantity: 1 },
    { variant_id: 7, quantity: 0 },
  ];
  assert.equal(cartContent(items), CONTENT);
  assert.equal(cartContent([{ variant_id: 10, quantity: 1 }, { variant_id: 9, quantity: 1 }]), "9:1,10:1"); // numeric, not text order
  assert.equal(cartContent([]), "");
});

test("content validation accepts only canonical strings", () => {
  assert.ok(contentValid(CONTENT));
  assert.ok(contentValid("1:123456"), "6-digit quantity ok");
  for (const bad of ["", "a:1", "1:0", "1:1,1:1", "2:1,1:1", "01:1", "1:1,", "1:1234567", null, 5,
                     Array.from({ length: 251 }, (_, i) => `${i + 1}:1`).join(",")]) {
    assert.equal(contentValid(bad), false, String(bad).slice(0, 40));
  }
});

test("key set holds previous, current and next window", async () => {
  const now = W * 1800 * 1000 + 5000;
  assert.equal(windowOf(now), W);
  const v = JSON.parse(await keysetValue(MASTER, SHOP, W, now));
  assert.equal(v.v, 1);
  assert.deepEqual(Object.keys(v.k).sort(), [String(W - 1), String(W), String(W + 1)]);
  assert.equal(v.k[String(W)], KEY);
  assert.match(v.d, /^\d{4}-\d{2}-\d{2}$/);
});

test("freshness: ~70 min ahead in shop time, +60 min across a DST change", async () => {
  const { freshnessValue } = await import("../src/lib.js");
  // 2026-09-30 14:00 UTC = 10:00 EDT; window start
  const w = Math.floor(Date.UTC(2026, 8, 30, 14, 0, 0) / 1000 / 1800);
  const f = JSON.parse(freshnessValue(w, "America/New_York", w * 1800 * 1000)).freshUntil;
  assert.equal(f, "2026-09-30T11:10:00");
  // 2026-11-01 05:30 UTC = 01:30 EDT, the hour before clocks go back
  const w2 = Math.floor(Date.UTC(2026, 10, 1, 5, 30, 0) / 1000 / 1800);
  const f2 = JSON.parse(freshnessValue(w2, "America/New_York", w2 * 1800 * 1000)).freshUntil;
  assert.equal(f2, "2026-11-01T02:40:00", "06:40 UTC is 01:40 EST, but 01:45 EDT comes first: +60 min slack");
});

test("signing window falls back to the newest published key", () => {
  assert.equal(signingWindow(W, W + 1), W);
  assert.equal(signingWindow(W, W), W);
  assert.equal(signingWindow(W, W - 3), W - 3); // publishing stuck: still a key the rule has
  assert.equal(signingWindow(W, NaN), W);
});

test("app proxy signature (Shopify example format)", async () => {
  const secret = "hush";
  const params = new URLSearchParams("shop=medicalrite-dev.myshopify.com&path_prefix=%2Fapps%2Fsc&timestamp=1317327555&extra=1&extra=2");
  const msg = "extra=1,2path_prefix=/apps/scshop=medicalrite-dev.myshopify.comtimestamp=1317327555";
  params.append("signature", await hmacHex(new TextEncoder().encode(secret), msg));
  const t = 1317327555 * 1000;
  assert.equal(await verifyProxySignature(params, secret, t), true);
  assert.equal(await verifyProxySignature(params, secret, t + 301_000), false, "older than 5 minutes");
  assert.equal(await verifyProxySignature(params, "wrong", t), false);
  params.set("shop", "evil.myshopify.com");
  assert.equal(await verifyProxySignature(params, secret, t), false);
  assert.equal(await verifyProxySignature(new URLSearchParams("shop=x"), secret), false);
});

async function jwt(payload, secret, alg = "HS256") {
  const b64 = (o) => Buffer.from(JSON.stringify(o)).toString("base64url");
  const head = `${b64({ alg, typ: "JWT" })}.${b64(payload)}`;
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = Buffer.from(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(head))).toString("base64url");
  return `${head}.${sig}`;
}

test("session token verification", async () => {
  const now = 1_790_000_000_000;
  const ok = { aud: "cid", dest: `https://${SHOP}`, exp: now / 1000 + 60, nbf: now / 1000 - 5 };
  assert.equal(await verifyIdToken(await jwt(ok, "sec"), "cid", "sec", now), SHOP);
  assert.equal(await verifyIdToken(await jwt(ok, "other"), "cid", "sec", now), null);
  assert.equal(await verifyIdToken(await jwt({ ...ok, aud: "x" }, "sec"), "cid", "sec", now), null);
  assert.equal(await verifyIdToken(await jwt({ ...ok, exp: now / 1000 - 60 }, "sec"), "cid", "sec", now), null);
  assert.equal(await verifyIdToken(await jwt({ ...ok, dest: "https://evil.com" }, "sec"), "cid", "sec", now), null);
  assert.equal(await verifyIdToken(await jwt(ok, "sec", "none"), "cid", "sec", now), null);
  assert.equal(await verifyIdToken("junk", "cid", "sec", now), null);
});

test("client ip and constant-time compare", () => {
  assert.equal(clientIp(new Headers({ "x-forwarded-for": "203.0.113.9, 10.0.0.1" })), "203.0.113.9");
  assert.equal(clientIp(new Headers({ "x-forwarded-for": "2001:db8::1" })), "2001:db8::1");
  assert.equal(clientIp(new Headers({})), null);
  assert.equal(clientIp(new Headers({ "x-forwarded-for": "::ffff:198.51.100.4" })), "198.51.100.4");
  assert.equal(clientIp(new Headers({ "x-forwarded-for": "1.1.1.1, 2.2.2.2" }), "last"), "2.2.2.2");
  assert.equal(clientIp(new Headers({ "x-forwarded-for": "<script>" })), null);
  assert.equal(safeEqual("abc", "abc"), true);
  assert.equal(safeEqual("abc", "abd"), false);
  assert.equal(safeEqual("abc", "ab"), false);
});
