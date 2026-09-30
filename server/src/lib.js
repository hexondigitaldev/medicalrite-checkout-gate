// Pure helpers for the token server. No Cloudflare bindings here, so `node --test` can run them.
// Token format and key scheme: see extensions/checkout-rule/src/token.rs (must stay identical).

const enc = new TextEncoder();

export const WINDOW_SECONDS = 1800; // 30-minute key windows -> tokens live 30-60 min (D1)

export function windowOf(nowMs) {
  return Math.floor(nowMs / 1000 / WINDOW_SECONDS);
}

export function hex(buf) {
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export function unhex(s) {
  if (typeof s !== "string" || s.length % 2 || /[^0-9a-f]/i.test(s)) throw new Error("bad hex");
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(s.slice(i * 2, i * 2 + 2), 16);
  return out;
}

async function hmacRaw(keyBytes, message) {
  const key = await crypto.subtle.importKey("raw", keyBytes, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return crypto.subtle.sign("HMAC", key, enc.encode(message));
}

export async function hmacHex(keyBytes, message) {
  return hex(await hmacRaw(keyBytes, message));
}

/** Constant-time string compare (same length required). */
export function safeEqual(a, b) {
  if (typeof a !== "string" || typeof b !== "string" || a.length !== b.length) return false;
  let d = 0;
  for (let i = 0; i < a.length; i++) d |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return d === 0;
}

/** Per-window signing key. The master secret never leaves the server; only window keys are published. */
export async function windowKey(masterHex, shop, w) {
  return hmacHex(unhex(masterHex), `1|${shop}|${w}`);
}

/** Canonical cart content, "variantId:qty" summed per variant, sorted by id. Matches token.rs `content`. */
export function cartContent(items) {
  const agg = new Map();
  for (const it of items || []) {
    const id = String(it.variant_id ?? it.id ?? "");
    const q = Number(it.quantity) || 0;
    if (!/^\d{1,19}$/.test(id)) continue;
    agg.set(id, (agg.get(id) || 0) + q);
  }
  return [...agg.entries()]
    .filter(([, q]) => q > 0)
    .sort(([a], [b]) => (BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0))
    .map(([id, q]) => `${id}:${q}`)
    .join(",");
}

/** Accept only canonical content strings (what cartContent produces), max 250 lines. */
export function contentValid(c) {
  if (typeof c !== "string" || c.length === 0 || c.length > 8000) return false;
  const parts = c.split(",");
  if (parts.length > 250) return false;
  let prev = -1n;
  for (const p of parts) {
    const m = /^([1-9]\d{0,18}):([1-9]\d{0,5})$/.exec(p);
    if (!m) return false;
    const id = BigInt(m[1]);
    if (id <= prev) return false; // sorted, no duplicates
    prev = id;
  }
  return true;
}

/** "1.<w>.<h|s>.<32 hex>" */
export async function signToken(windowKeyHex, w, flag, content) {
  if (flag !== "h" && flag !== "s") throw new Error("flag");
  const sig = (await hmacHex(unhex(windowKeyHex), `1|${w}|${flag}|${content}`)).slice(0, 32);
  return `1.${w}.${flag}.${sig}`;
}

/** Metafield value published for the checkout rule: keys for w-1, w, w+1 (c = current window). */
export async function keysetValue(masterHex, shop, w, nowMs) {
  const k = {};
  for (const x of [w - 1, w, w + 1]) k[String(x)] = await windowKey(masterHex, shop, x);
  return JSON.stringify({ v: 1, d: new Date(nowMs).toISOString().slice(0, 10), c: w, k });
}

/** Seconds a token signed in window w is guaranteed to stay valid (its key is dropped from window w+2 on). */
export function tokenTtl(w, nowMs) {
  return Math.max(0, Math.floor(((w + 2) * WINDOW_SECONDS * 1000 - nowMs) / 1000));
}

/** "YYYY-MM-DDTHH:MM:SS" in the given IANA time zone (the checkout rule compares in shop time). */
export function localDateTime(ms, timeZone) {
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone, year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23",
  }).formatToParts(new Date(ms));
  const g = (t) => parts.find((p) => p.type === t).value;
  return `${g("year")}-${g("month")}-${g("day")}T${g("hour")}:${g("minute")}:${g("second")}`;
}

/**
 * Input variable for the checkout rule: after `freshUntil` (shop time) the rule treats the keys as
 * stale and lets checkouts through (D5). Set to the start of window w+2 plus 10 minutes slack, so
 * one missed cron run is tolerated and a dead server fails open within ~70 minutes.
 */
export function freshnessValue(w, timeZone, nowMs = w * WINDOW_SECONDS * 1000) {
  let target = ((w + 2) * WINDOW_SECONDS + 600) * 1000;
  // Daylight-saving change between now and target: local clock repeats/jumps an hour -> add slack.
  if (utcOffset(nowMs, timeZone) !== utcOffset(target, timeZone)) target += 3600 * 1000;
  return JSON.stringify({ freshUntil: localDateTime(target, timeZone) });
}

function utcOffset(ms, timeZone) {
  return Date.parse(localDateTime(ms, timeZone) + "Z") - Math.floor(ms / 1000) * 1000;
}

/**
 * Which window to sign with. Normally the current one. If key publishing has fallen behind
 * (Admin API problem), sign with the newest window the checkout rule already has, so real
 * buyers are not blocked by our own outage (D5). /health reports the problem.
 */
export function signingWindow(currentW, publishedMaxW) {
  // Published windows are publishedMaxW-2 .. publishedMaxW.
  if (!Number.isFinite(publishedMaxW)) return currentW;
  return Math.min(currentW, publishedMaxW);
}

/**
 * Shopify app proxy signature: all query params except `signature`, sorted by key,
 * "key=value" (multi-values joined by ","), concatenated without separator, HMAC-SHA256 hex.
 */
export async function verifyProxySignature(searchParams, clientSecret, nowMs = Date.now()) {
  const given = searchParams.get("signature");
  if (!given) return false;
  const ts = Number(searchParams.get("timestamp"));
  if (!Number.isFinite(ts) || Math.abs(nowMs / 1000 - ts) > 300) return false;
  const map = new Map();
  for (const [k, v] of searchParams) {
    if (k === "signature") continue;
    map.set(k, map.has(k) ? `${map.get(k)},${v}` : v);
  }
  const msg = [...map.keys()].sort().map((k) => `${k}=${map.get(k)}`).join("");
  return safeEqual(await hmacHex(enc.encode(clientSecret), msg), given.toLowerCase());
}

function b64urlToBytes(s) {
  const b = atob(s.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((s.length + 3) % 4));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}

/** Verify a Shopify session token (id_token, HS256 signed with the app's client secret). Returns shop domain or null. */
export async function verifyIdToken(jwt, clientId, clientSecret, nowMs) {
  const parts = String(jwt || "").split(".");
  if (parts.length !== 3) return null;
  let header, payload;
  try {
    header = JSON.parse(new TextDecoder().decode(b64urlToBytes(parts[0])));
    payload = JSON.parse(new TextDecoder().decode(b64urlToBytes(parts[1])));
  } catch {
    return null;
  }
  if (header.alg !== "HS256") return null;
  const key = await crypto.subtle.importKey("raw", enc.encode(clientSecret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  const ok = await crypto.subtle.verify("HMAC", key, b64urlToBytes(parts[2]), enc.encode(`${parts[0]}.${parts[1]}`));
  if (!ok) return null;
  const now = Math.floor(nowMs / 1000);
  if (payload.aud !== clientId) return null;
  if (typeof payload.exp !== "number" || payload.exp + 5 < now) return null;
  if (typeof payload.nbf === "number" && payload.nbf - 5 > now) return null;
  const m = /^https:\/\/([a-z0-9][a-z0-9-]*\.myshopify\.com)$/.exec(String(payload.dest || ""));
  return m ? m[1] : null;
}

/**
 * Buyer IP from X-Forwarded-For (Shopify adds it for app proxy requests). `pos` = "first" or "last"
 * entry — confirm on the dev store which one Shopify controls (docs/stage2-plan.md, dev test T8).
 */
export function clientIp(headers, pos = "first") {
  const list = (headers.get("x-forwarded-for") || "").split(",").map((x) => x.trim()).filter(Boolean);
  let ip = pos === "last" ? list[list.length - 1] : list[0];
  if (ip && /^::ffff:\d+\.\d+\.\d+\.\d+$/i.test(ip)) ip = ip.slice(7); // IPv4-mapped IPv6
  return ip && /^[0-9a-f:.]{3,45}$/i.test(ip) ? ip : null;
}

/** Rate-limit key: IPv4 as is, IPv6 per /64 (one household/device gets a whole /64), "noip" otherwise. */
export function limitKey(ip) {
  if (!ip) return "noip";
  if (!ip.includes(":")) return ip;
  const [head, tail] = ip.split("::");
  const h = head ? head.split(":") : [];
  const t = tail !== undefined ? (tail ? tail.split(":") : []) : [];
  const full = tail !== undefined ? [...h, ...Array(8 - h.length - t.length).fill("0"), ...t] : h;
  return full.slice(0, 4).map((x) => (x || "0").toLowerCase().replace(/^0+(?=.)/, "")).join(":") + "::/64";
}
