// Token server (Cloudflare Worker). One Worker per app environment (dev / production).
//
//   POST /proxy/t   app proxy (/apps/sc/t on the store): Turnstile check -> signed cart token
//   GET  /          app home inside Shopify admin: (re)connects the app (token exchange), shows status
//   GET  /health    200 when everything is fine, 503 + reasons otherwise (point an uptime monitor at it)
//   cron (10 min)   publishes window keys ($app:sc.keys on the shop) and the freshness input variable
//                   ($app:sc.vars on the checkout rule); optionally checks the storefront still loads sf.js
//
// Secrets (wrangler secret put): SHOPIFY_CLIENT_SECRET, TURNSTILE_SECRET, MASTER_KEY.
// Nothing secret is ever returned to the browser or logged. No IPs or cart contents are logged.

import {
  windowOf, windowKey, signToken, keysetValue, freshnessValue, tokenTtl, signingWindow, contentValid,
  verifyProxySignature, verifyIdToken, clientIp, limitKey,
} from "./lib.js";

const SITEVERIFY = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

const json = (obj, status = 200) =>
  new Response(JSON.stringify(obj), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store" },
  });

function log(ev, fields = {}) {
  console.log(JSON.stringify({ ev, ...fields }));
}

const errText = (e) => String((e && e.message) || e).slice(0, 300);

async function readJson(req, maxBytes) {
  const text = await req.text();
  if (text.length > maxBytes) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

async function limited(binding, key) {
  if (!binding) return false;
  try {
    const { success } = await binding.limit({ key });
    return !success;
  } catch {
    return false; // limiter trouble never blocks a buyer
  }
}

// ---------- Turnstile ----------

/** Returns ["h", "ok"] when the check passed, otherwise ["s", reason]. Never throws. */
export async function turnstile(env, response, ip, fetchImpl = fetch) {
  if (typeof response !== "string" || response.length === 0 || response.length > 2048) return ["s", "no_response"];
  const form = new FormData();
  form.append("secret", env.TURNSTILE_SECRET);
  form.append("response", response);
  if (ip) form.append("remoteip", ip);
  let data;
  try {
    const r = await fetchImpl(SITEVERIFY, { method: "POST", body: form, signal: AbortSignal.timeout(2500) });
    // Cloudflare-side problem (5xx): soft, never hard-block (rule 2), not counted against soft limits.
    if (r.status >= 500) return ["s", `siteverify_${r.status}`];
    if (!r.ok) return ["s", "failed"];
    data = await r.json();
  } catch {
    return ["s", "siteverify_unreachable"];
  }
  // error-codes are Cloudflare's reason (e.g. invalid-input-secret, timeout-or-duplicate); no secrets in them.
  if (!data.success) return ["s", `failed:${[].concat(data["error-codes"] || []).join("+").slice(0, 80)}`];
  const hosts = String(env.TURNSTILE_HOSTNAMES || "").split(",").map((h) => h.trim()).filter(Boolean);
  if (!hosts.includes(data.hostname)) return ["s", "hostname"];
  if (data.action !== "cart") return ["s", "action"];
  return ["h", "ok"];
}

// ---------- Shopify Admin access (token exchange, optional refresh) ----------

async function saveAuth(env, data, nowMs) {
  const auth = {
    at: data.access_token,
    exp: data.expires_in ? nowMs + data.expires_in * 1000 : null,
    rt: data.refresh_token || null,
  };
  await env.STATE.put("auth", JSON.stringify(auth));
  return auth;
}

async function oauth(env, body) {
  const r = await fetch(`https://${env.SHOP}/admin/oauth/access_token`, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify({ client_id: env.SHOPIFY_CLIENT_ID, client_secret: env.SHOPIFY_CLIENT_SECRET, ...body }),
  });
  if (!r.ok) throw new Error(`oauth_${r.status}`);
  return r.json();
}

async function exchange(env, idToken, nowMs) {
  const data = await oauth(env, {
    grant_type: "urn:ietf:params:oauth:grant-type:token-exchange",
    subject_token: idToken,
    subject_token_type: "urn:ietf:params:oauth:token-type:id_token",
    requested_token_type: "urn:shopify:params:oauth:token-type:offline-access-token",
  });
  return saveAuth(env, data, nowMs);
}

async function accessToken(env, nowMs) {
  const raw = await env.STATE.get("auth");
  if (!raw) return null;
  let auth = JSON.parse(raw);
  if (auth.exp && auth.exp - 120000 < nowMs && auth.rt) {
    auth = await saveAuth(env, await oauth(env, { grant_type: "refresh_token", refresh_token: auth.rt }), nowMs);
  }
  return auth.at;
}

async function admin(env, token, query, variables) {
  const r = await fetch(`https://${env.SHOP}/admin/api/${env.API_VERSION}/graphql.json`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-shopify-access-token": token },
    body: JSON.stringify({ query, variables }),
  });
  if (r.status === 401 || r.status === 403) throw new Error(`admin_auth_${r.status} (open the app in Shopify admin to reconnect)`);
  if (!r.ok) throw new Error(`admin_${r.status}`);
  const body = await r.json();
  if (body.errors) throw new Error(`admin_graphql: ${JSON.stringify(body.errors).slice(0, 300)}`);
  return body.data;
}

// ---------- Key + freshness publishing ----------

const FIND = `{
  shop { id ianaTimezone }
  validations(first: 25) { nodes { id shopifyFunction { app { apiKey } title } } }
}`;

// Also deletes the old shop-level copy of the keys (pre-T9 location, readable from theme Liquid).
const SET = `mutation($m: [MetafieldsSetInput!]!, $d: [MetafieldIdentifierInput!]!) {
  metafieldsSet(metafields: $m) { userErrors { field message code } }
  metafieldsDelete(metafields: $d) { userErrors { field message } }
}`;

export async function publish(env, nowMs, force = false) {
  const w = windowOf(nowMs);
  const pub = Number(await env.STATE.get("pub"));
  if (!force && pub >= w + 1) return "up_to_date";
  const token = await accessToken(env, nowMs);
  if (!token) throw new Error("not_connected (open the app in Shopify admin)");
  const found = await admin(env, token, FIND);
  const mine = found.validations.nodes.filter((v) => v.shopifyFunction && v.shopifyFunction.app && v.shopifyFunction.app.apiKey === env.SHOPIFY_CLIENT_ID);
  if (!mine.length) {
    await env.STATE.put("pub_at", JSON.stringify({ at: new Date(nowMs).toISOString(), rules: 0 }));
    return "no checkout rule found yet";
  }
  // Keys and the freshness variable go on every checkout rule of this app (normally exactly one).
  // Not on the shop: shop $app metafields are readable from theme Liquid (dev test T9).
  const keys = await keysetValue(env.MASTER_KEY, env.SHOP, w, nowMs);
  const vars = freshnessValue(w, found.shop.ianaTimezone, nowMs);
  const m = [];
  for (const v of mine) {
    m.push({ ownerId: v.id, namespace: "$app:sc", key: "keys", type: "json", value: keys });
    m.push({ ownerId: v.id, namespace: "$app:sc", key: "vars", type: "json", value: vars });
  }
  const d = [{ ownerId: found.shop.id, namespace: "$app:sc", key: "keys" }];
  const data = await admin(env, token, SET, { m, d });
  const errs = data.metafieldsSet.userErrors;
  if (errs.length) throw new Error(`metafieldsSet: ${JSON.stringify(errs).slice(0, 300)}`);
  await env.STATE.put("pub", String(w + 1));
  await env.STATE.put("pub_at", JSON.stringify({ at: new Date(nowMs).toISOString(), rules: mine.length }));
  return "published";
}

async function publishLogged(env, nowMs, force) {
  try {
    const r = await publish(env, nowMs, force);
    if (r !== "up_to_date") log("keys_published", { w: windowOf(nowMs), r });
    return r;
  } catch (e) {
    log("keys_publish_failed", { error: errText(e) });
    return `error: ${errText(e).slice(0, 200)}`;
  }
}

/** Optional: make sure the live storefront still loads the helper (a theme publish can drop the embed). */
async function checkEmbed(env) {
  if (!env.STOREFRONT_URL) return;
  let state;
  try {
    const r = await fetch(env.STOREFRONT_URL, { headers: { "user-agent": "sc-health" }, signal: AbortSignal.timeout(8000) });
    if (!r.ok) state = "unreachable";
    else state = /<script[^>]*sf\.js[^>]*data-k="[^"]+"/.test(await r.text()) ? "ok" : "missing";
  } catch {
    state = "unreachable";
  }
  if ((await env.STATE.get("embed")) !== state) {
    await env.STATE.put("embed", state); // only on change: keeps KV writes low
    log("embed_state", { state });
  }
}

export async function health(env, nowMs) {
  const reasons = [];
  if (!(Number(await env.STATE.get("pub")) >= windowOf(nowMs))) reasons.push("keys_out_of_date");
  const pubAt = JSON.parse((await env.STATE.get("pub_at")) || "null");
  if (pubAt && pubAt.rules === 0) reasons.push("no_checkout_rule");
  if (env.STOREFRONT_URL && (await env.STATE.get("embed")) === "missing") reasons.push("embed_missing");
  return { ok: reasons.length === 0, reasons, pub_at: pubAt && pubAt.at };
}

// ---------- Routes ----------

export async function handleToken(req, env, url, nowMs, fetchImpl = fetch) {
  if (req.method !== "POST") return json({ error: "method" }, 405);
  if (!(await verifyProxySignature(url.searchParams, env.SHOPIFY_CLIENT_SECRET, nowMs))) return json({ error: "sig" }, 401);
  if (url.searchParams.get("shop") !== env.SHOP) return json({ error: "shop" }, 403);

  const ip = clientIp(req.headers, env.IP_HEADER_POS);
  const key = limitKey(ip);
  if (await limited(env.LIMIT_IP, key)) {
    log("rate_limited", { kind: "ip", noip: !ip });
    return json({ error: "slow_down" }, 429);
  }
  const body = await readJson(req, 12000);
  if (!body || !contentValid(body.c)) return json({ error: "bad_request" }, 400);

  const [flag, why] = await turnstile(env, body.r, ip, fetchImpl);
  let overBudget = false;
  // Soft limits only for failures the buyer's side caused; our/Cloudflare's own outage never counts.
  if (flag === "s" && !why.startsWith("siteverify_")) {
    if (await limited(env.LIMIT_SOFT, key)) {
      log("rate_limited", { kind: "soft", why });
      return json({ error: "slow_down" }, 429);
    }
    // Store-wide budget: never refuse (that would block real soft buyers, spec rule 2) — flag it for review.
    overBudget = await limited(env.LIMIT_SOFT_ALL, "all");
  }
  const cw = windowOf(nowMs);
  const w = signingWindow(cw, Number(await env.STATE.get("pub")));
  const t = await signToken(await windowKey(env.MASTER_KEY, env.SHOP, w), w, flag, body.c);
  // tid = window + first 8 signature chars: matches the checkout rule's log line.
  // xff: how many X-Forwarded-For entries Shopify sent (dev test T8 decides IP_HEADER_POS); no IPs logged.
  const xff = (req.headers.get("x-forwarded-for") || "").split(",").filter((x) => x.trim()).length;
  log("token", { f: flag, why, xff, lines: body.c.split(",").length, tid: `${w}.${t.split(".")[3].slice(0, 8)}`, stale_keys: w !== cw, soft_over_budget: overBudget || undefined });
  // In fallback the key stays published until publishing recovers, so don't make browsers refresh early.
  return json({ t, f: flag, ttl: w === cw ? tokenTtl(w, nowMs) : 1800 });
}

function page(env, title, rows) {
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const body = rows.map(([k, v]) => `<tr><th>${esc(k)}</th><td>${esc(v)}</td></tr>`).join("");
  return new Response(
    `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="shopify-api-key" content="${esc(env.SHOPIFY_CLIENT_ID)}"><script src="https://cdn.shopify.com/shopifycloud/app-bridge.js"></script>
<title>${esc(title)}</title><style>body{font:14px system-ui,sans-serif;margin:24px;color:#222}th{text-align:left;padding:4px 16px 4px 0;color:#666;font-weight:500}td{padding:4px 0}</style>
</head><body><h2>${esc(title)}</h2><table>${body}</table></body></html>`,
    {
      headers: {
        "content-type": "text/html; charset=utf-8",
        "cache-control": "no-store",
        "content-security-policy": `frame-ancestors https://${env.SHOP} https://admin.shopify.com;`,
      },
    },
  );
}

async function handleHome(env, url, nowMs) {
  const idToken = url.searchParams.get("id_token");
  const shop = await verifyIdToken(idToken, env.SHOPIFY_CLIENT_ID, env.SHOPIFY_CLIENT_SECRET, nowMs);
  if (!shop || shop !== env.SHOP) {
    // Why it was refused (no token values logged): helps tell a wrong client secret from a missing token.
    let claims = {};
    try {
      const p = JSON.parse(atob(String(idToken || "").split(".")[1].replace(/-/g, "+").replace(/_/g, "/")));
      claims = { aud_ok: p.aud === env.SHOPIFY_CLIENT_ID, dest: p.dest, expired: p.exp * 1000 < nowMs };
    } catch {}
    log("home_denied", { has_token: !!idToken, verified_shop: shop, ...claims });
    return new Response("Open this app from the Shopify admin.", { status: 401 });
  }
  // Re-exchange on every visit: repairs a revoked token (reinstall, scope change) as soon as staff open the app.
  let connect = "connected";
  try {
    await exchange(env, idToken, nowMs);
  } catch (e) {
    connect = `failed: ${errText(e).slice(0, 120)}`;
  }
  const pubResult = connect === "connected" ? await publishLogged(env, nowMs, true) : "skipped";
  const h = await health(env, nowMs);
  return page(env, "Checkout tools", [
    ["Connection", connect],
    ["Key update", pubResult],
    ["Last key update", h.pub_at || "never"],
    ["Status", h.ok ? "OK" : h.reasons.join(", ")],
  ]);
}

export default {
  async fetch(req, env) {
    const url = new URL(req.url);
    const now = Date.now();
    try {
      if (url.pathname === "/proxy/t") return await handleToken(req, env, url, now);
      if (url.pathname === "/health") {
        const h = await health(env, now);
        return json({ ok: h.ok, reasons: h.reasons }, h.ok ? 200 : 503);
      }
      if (url.pathname === "/" && req.method === "GET") return await handleHome(env, url, now);
      return new Response("Not found", { status: 404 });
    } catch (e) {
      log("error", { path: url.pathname, error: errText(e) });
      return json({ error: "server" }, 500);
    }
  },

  async scheduled(_event, env, ctx) {
    ctx.waitUntil(Promise.all([publishLogged(env, Date.now(), false), checkEmbed(env)]));
  },
};
