import test from "node:test";
import assert from "node:assert/strict";
import { handleToken, publish, health, turnstile } from "../src/index.js";
import { hmacHex, windowOf } from "../src/lib.js";

const SHOP = "medicalrite-dev.myshopify.com";
const SECRET = "app-secret";
const MASTER = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const NOW = 995432 * 1800 * 1000 + 60_000;

function kv(init = {}) {
  const m = new Map(Object.entries(init));
  return { get: async (k) => (m.has(k) ? m.get(k) : null), put: async (k, v) => void m.set(k, v), _m: m };
}
function limiter(allow = true) {
  const calls = [];
  return { calls, limit: async ({ key }) => (calls.push(key), { success: allow }) };
}
function env(over = {}) {
  return {
    SHOP, SHOPIFY_CLIENT_ID: "cid", SHOPIFY_CLIENT_SECRET: SECRET, TURNSTILE_SECRET: "ts", MASTER_KEY: MASTER,
    TURNSTILE_HOSTNAMES: SHOP, API_VERSION: "2026-07", IP_HEADER_POS: "first",
    STATE: kv({ pub: String(995433) }), LIMIT_IP: limiter(), LIMIT_SOFT: limiter(), LIMIT_SOFT_ALL: limiter(),
    ...over,
  };
}
async function signedUrl(params = {}, secret = SECRET) {
  const p = new URLSearchParams({ shop: SHOP, path_prefix: "/apps/sc", timestamp: String(Math.floor(NOW / 1000)), ...params });
  const msg = [...p.keys()].sort().map((k) => `${k}=${p.get(k)}`).join("");
  p.set("signature", await hmacHex(new TextEncoder().encode(secret), msg));
  return new URL(`https://w.example/proxy/t?${p}`);
}
const req = (body, headers = { "x-forwarded-for": "203.0.113.9" }) =>
  new Request("https://w.example/proxy/t", { method: "POST", body: typeof body === "string" ? body : JSON.stringify(body), headers });
const siteverify = (data, status = 200) => async () => new Response(JSON.stringify(data), { status });
const GOOD = { success: true, hostname: SHOP, action: "cart" };
const C = "41234567890:2,49876543210:1";

test("hard token when Turnstile passes; ttl 30-60 min; token matches shared vector", async () => {
  const r = await handleToken(req({ c: C, r: "tsresp" }), env(), await signedUrl(), NOW, siteverify(GOOD));
  assert.equal(r.status, 200);
  const b = await r.json();
  assert.equal(b.t, "1.995432.h.fe2646951b5fe6da175a3580ffebdd1f");
  assert.equal(b.f, "h");
  assert.ok(b.ttl >= 1800 && b.ttl <= 3600, String(b.ttl));
});

test("spec 12: every Turnstile problem gives a soft token, never an error", async () => {
  const cases = [
    [siteverify({ success: false }), "failed"],
    [siteverify({}, 500), "siteverify_500"],
    [siteverify({}, 400), "failed"],
    [async () => { throw new Error("down"); }, "siteverify_unreachable"],
    [siteverify({ ...GOOD, hostname: "evil.com" }), "hostname"],
    [siteverify({ ...GOOD, action: "other" }), "action"],
  ];
  for (const [f] of cases) {
    const r = await handleToken(req({ c: C, r: "x" }), env(), await signedUrl(), NOW, f);
    assert.equal(r.status, 200);
    assert.equal((await r.json()).f, "s");
  }
  const r = await handleToken(req({ c: C, r: null }), env(), await signedUrl(), NOW, siteverify(GOOD));
  assert.equal((await r.json()).t, "1.995432.s.48726d1a28a056f22f361509aff6f5b2");
  for (const [f, why] of cases) assert.deepEqual(await turnstile(env(), "x", null, f), ["s", why]);
});

test("rejects bad signature, old timestamp, wrong shop, wrong method, bad content", async () => {
  const e = env();
  assert.equal((await handleToken(req({ c: C }), e, await signedUrl({}, "wrong"), NOW)).status, 401);
  assert.equal((await handleToken(req({ c: C }), e, await signedUrl({ timestamp: String(Math.floor(NOW / 1000) - 3600) }), NOW)).status, 401);
  assert.equal((await handleToken(req({ c: C }), e, await signedUrl({ shop: "other.myshopify.com" }), NOW)).status, 403);
  const get = new Request("https://w.example/proxy/t");
  assert.equal((await handleToken(get, e, await signedUrl(), NOW)).status, 405);
  for (const b of ["{", { c: "2:1,1:1" }, { c: "" }, {}]) {
    assert.equal((await handleToken(req(b), e, await signedUrl(), NOW, siteverify(GOOD))).status, 400);
  }
});

test("rate limits: per IP, per IP for soft, store-wide soft; missing IP shares one strict key", async () => {
  let e = env({ LIMIT_IP: limiter(false) });
  assert.equal((await handleToken(req({ c: C, r: "x" }), e, await signedUrl(), NOW, siteverify(GOOD))).status, 429);
  e = env({ LIMIT_SOFT: limiter(false) });
  assert.equal((await handleToken(req({ c: C, r: "x" }), e, await signedUrl(), NOW, siteverify(GOOD))).status, 200, "hard path ignores soft limit");
  assert.equal((await handleToken(req({ c: C }), e, await signedUrl(), NOW, siteverify(GOOD))).status, 429);
  // store-wide soft budget never refuses (spec rule 2), it only flags
  e = env({ LIMIT_SOFT_ALL: limiter(false) });
  const over = await handleToken(req({ c: C }), e, await signedUrl(), NOW, siteverify(GOOD));
  assert.equal(over.status, 200);
  assert.equal((await over.json()).f, "s");
  // Cloudflare-side failures never count against soft limits
  e = env({ LIMIT_SOFT: limiter(false) });
  const cf = await handleToken(req({ c: C, r: "x" }), e, await signedUrl(), NOW, siteverify({}, 503));
  assert.equal(cf.status, 200);
  assert.deepEqual(e.LIMIT_SOFT.calls, []);
  // a 4xx from siteverify is the buyer's side: soft limit applies
  e = env({ LIMIT_SOFT: limiter(false) });
  assert.equal((await handleToken(req({ c: C, r: "x" }), e, await signedUrl(), NOW, siteverify({}, 400))).status, 429);
  e = env();
  await handleToken(req({ c: C }, {}), e, await signedUrl(), NOW, siteverify(GOOD));
  assert.deepEqual(e.LIMIT_IP.calls, ["noip"]);
  e = env();
  await handleToken(req({ c: C }, { "x-forwarded-for": "2001:db8:1:2:aaaa::1" }), e, await signedUrl(), NOW, siteverify(GOOD));
  assert.deepEqual(e.LIMIT_IP.calls, ["2001:db8:1:2::/64"]);
});

test("signs with the newest published window when publishing is behind (fail open)", async () => {
  const e = env({ STATE: kv({ pub: String(995430) }) });
  const b = await (await handleToken(req({ c: C, r: "x" }), e, await signedUrl(), NOW, siteverify(GOOD))).json();
  assert.equal(b.t.split(".")[1], "995430");
  assert.equal(b.ttl, 1800, "no early refresh storm while in fallback");
});

test("publish writes keys on the shop and freshness on this app's checkout rule only", async () => {
  const calls = [];
  const orig = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    const body = JSON.parse(init.body);
    calls.push({ url: String(url), body, token: init.headers["x-shopify-access-token"] });
    if (body.query.includes("validations")) {
      return new Response(JSON.stringify({ data: {
        shop: { id: "gid://shopify/Shop/1", ianaTimezone: "America/New_York" },
        validations: { nodes: [
          { id: "gid://shopify/Validation/7", shopifyFunction: { app: { apiKey: "cid" }, title: "Checkout rule" } },
          { id: "gid://shopify/Validation/8", shopifyFunction: { app: { apiKey: "someone-else" }, title: "Other" } },
        ] },
      } }));
    }
    return new Response(JSON.stringify({ data: { metafieldsSet: { userErrors: [] } } }));
  };
  try {
    const e = env({ STATE: kv({ auth: JSON.stringify({ at: "shpat_x", exp: null, rt: null }) }) });
    assert.equal(await publish(e, NOW), "published");
    const m = calls[1].body.variables.m;
    assert.equal(m.length, 2);
    assert.deepEqual([m[0].ownerId, m[0].namespace, m[0].key], ["gid://shopify/Shop/1", "$app:sc", "k"]);
    const keys = JSON.parse(m[0].value);
    assert.deepEqual(Object.keys(keys.k).sort(), ["995431", "995432", "995433"]);
    assert.equal(keys.c, 995432);
    assert.deepEqual([m[1].ownerId, m[1].key], ["gid://shopify/Validation/7", "v"]);
    assert.match(JSON.parse(m[1].value).freshUntil, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$/);
    assert.equal(calls[1].token, "shpat_x");
    assert.equal(await e.STATE.get("pub"), "995433");
    assert.equal(await publish(e, NOW), "up_to_date");
    assert.equal((await health(e, NOW)).ok, true);
    assert.deepEqual((await health(e, NOW + 3 * 1800 * 1000)).reasons, ["keys_out_of_date"]);
  } finally {
    globalThis.fetch = orig;
  }
});

test("publish without a connection fails loudly, health reports it", async () => {
  const e = env({ STATE: kv() });
  await assert.rejects(publish(e, NOW), /not_connected/);
  assert.equal((await health(e, NOW)).ok, false);
  assert.equal(windowOf(NOW), 995432);
});
