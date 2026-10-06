# Stage 2 plan — browser token (built 2026-09-30, critic round 1 fixed)

Goal: a checkout can only complete if the buyer's browser passed an invisible Cloudflare Turnstile check on the store and got a short-lived signed **token** for this cart. Today's bots post straight to checkout (permalinks / scripts) and never load the store in a real browser, so they have no token.

Blocklist (Stage 1) keeps running alongside. Logged-in customers skip the token (D3). Drafts use Shopify's "ignore checkout rules" invoice toggle (D3).

## How it works (plain words)

1. **Store page** (product + cart): a theme app embed loads Turnstile invisibly.
2. Turnstile passes → the embed calls our **app proxy** `/apps/<neutral-path>/t` with the Turnstile answer and the cart contents.
3. **Token server** (Cloudflare Worker) checks the answer with Cloudflare, rate-limits per IP (Q6), and returns a token = HMAC(current key, cart contents + key window).
4. The embed saves the token as cart attribute `_bg` (`/cart/update.js`). It refreshes on page load and whenever the cart changes (D1).
5. **Checkout rule** at Pay now: recompute HMAC over the cart lines with the current or previous key. Valid → allow. Missing/invalid → block (or `would_block` in log mode).

## Design decisions to confirm in build/critic

| Topic | Plan | Why |
|---|---|---|
| Expiry | Server rotates the signing key every 30 min; rule accepts current + previous key → token lives 30–60 min (D1). | Functions have no clock and no cart ID (Q2). |
| Binding | Token covers sorted `variantId:qty` of the cart. | A copied token only works for an identical cart (Q2 option 3). |
| Keys | Stored in an app-owned metafield **on the checkout rule (validation)** — not the shop (T9: shop `$app` metafields are readable from Liquid), not the settings metaobject. Written by the Worker's cron via Admin API. | Keys never in the browser or merchant-editable data (hard rule). |
| Admin API access | Worker handles the one-time OAuth install and keeps the offline token as a Worker secret/KV. | Needed for key rotation. |
| Token rule mode | Its own `token_mode` (`off` / `log_only` / `enforce`) in the settings JSON, separate from the blocklist mode. | Roll out independently. |
| Fail open (D5) | If the key metafield is missing/stale (server down), the rule allows and logs `token_unavailable`. Worker cron health check emails us when rotation fails. | A server outage must not block real buyers. |
| Query cost | Add `attribute(key:"_bg")`, line `merchandise { ... on ProductVariant { id } }`, key metafield. Must stay ≤ 30. | Shopify limit. Critic measures. |
| Buy it now | **Hidden for now** (theme setting "dynamic checkout buttons" off on product pages), coordinate with Qckbot/Brandon. Cart-page PayPal/Google Pay stay (they keep `_bg`, Phase A). | D2; replacement later. |
| Permalinks | Tokenless → blocked when enforced. Ask Timur if marketing uses `/cart/...` links. | Bot door 2. |
| Neutral names | Embed, proxy path, attribute and script names reveal nothing about bot checks. | Don't tip off bot authors. |

## Pieces to build

1. `extensions/token-embed/` — theme app embed (JS, Turnstile invisible widget, cart watcher).
2. `server/` — Cloudflare Worker: `/t` token endpoint (Turnstile verify, rate limit, HMAC), OAuth install, cron key rotation + health alert. Secrets only in Worker secrets.
3. `extensions/checkout-rule/` — add token check + `token_mode`, tests (valid, expired, other cart, tampered, missing, key missing → allow, logged-in skip).
4. `shopify.app.*.toml` — app proxy config, `write_metafields`-level scope needed for key writes (to confirm).

## Accounts / things Hexon provides

- **Cloudflare account (free plan is enough to test):** Turnstile is free; Workers free plan = 100k requests/day. Create one Turnstile widget (Invisible) for the dev store domain first; add the live domain later. Keys go into Worker secrets, never into git or chat.
- If traffic ever needs more than the free plan, the Workers paid plan is small; ask Timur then.

## Rollout

Dev store (full flow + all checkout paths) → critic rounds → live with `token_mode: log_only` for a few days (target: ~100% of real Pay-step runs carry a valid token) → review with Timur → enforce.

## Questions for Timur before `token_mode: enforce`

| # | Question | Our suggestion |
|---|---|---|
| D17 | **Soft tokens.** The spec lets a buyer through when Turnstile fails ("soft" token). But anyone can ask our endpoint for a soft token with one plain request, so with soft tokens allowed the token check alone doesn't stop a scripted bot. Options: `allow` (spec; relies on blocklist + Stage 3), `risky` (soft token fails for guest + 1 item + qty 1 + under $5 — this **does block** a real guest buying one cheap item whose Turnstile failed, and a bot can dodge it with qty 2), `block` (every Turnstile failure is blocked, with the phone message). | Decide after log-only shows how many real orders are soft (`tok:"soft"`) and how many of those have the cheap single-item shape. Until then keep `allow`. |
| D18 | **Cloudflare paid plan ($5/month)** before enforcing. On the free plan a flood of requests can use up the daily quota; the check then switches itself off (fails open) until midnight UTC. | Yes, before enforce. |
| D19 | **Refresh timing.** D1 says "refresh on every page load". We check on every page load but only get a new token when less than 25 min is left (or the cart changed), to avoid a Turnstile check on every page. | OK as built. |
| — | **For information:** a token is bound to cart *contents*. A bot with a real browser could reuse one token for identical carts for up to an hour. The blocklist and Stage 3 cover that; logs will show it (`tid` repeating). | — |

## Dev store test plan (before any live deploy)

| # | Test | Expect |
|---|---|---|
| T0 | **Before anything else:** deploy with NO `$app:sc.vars` metafield and no Worker. `token_mode: "off"` + blocklist enforce + "428 st" → blocked with a normal log line. Then `token_mode: "log_only"` → `tok:"keys_stale"`. (Proves the query default works; if not, the whole rule — blocklist too — would stop running.) | as stated |
| T1 | `cargo build` wasm size; Pay-step instruction count in Dev Dashboard | < 256 kB; < 11M (Stage 1: 112 kB, 207k) |
| T2 | Add the checkout rule first, then open the app in admin | Page shows Connection: connected, Key update: published, Status: OK |
| T3 | Normal checkout, `token_mode: log_only` | Log `tok:"ok"`, `tage` 0 or 1, `tid` matches Worker log |
| T4 | Change quantity then click checkout fast; type a quantity in the cart page field and click Checkout without leaving the field; add via product page then checkout | `tok:"ok"` |
| T5 | Permalink `/cart/<variant>:1` and `add.js` + `/checkout` with no page load (spec 1, 2) | `would_block` `token_missing` |
| T6 | Copy `_bg` from cart A to cart B with other items (spec 11) | `token_invalid` |
| T7 | Block `challenges.cloudflare.com` in the browser (spec 12) | soft token, `tok:"soft"`, not blocked |
| T8 | From two different networks (e.g. wifi and phone data), plus once with a fake `X-Forwarded-For`, call `/apps/sc/t` and log the header on the Worker (temporary debug) | pick the `IP_HEADER_POS` entry that differs per network and ignores the fake one |
| T9 | Liquid `{{ shop.metafields['app--<id>--sc'].k }}` in a test theme + Storefront API query | empty (keys not readable from storefront) |
| T10 | Stop the cron (or delete `$app:sc.vars`), wait 70 min, checkout with no token in enforce | allowed, `tok:"keys_stale"` |
| T11 | Cart-page PayPal express right after a quantity change | `tok:"ok"` |
| T12 | Logged-in customer without token in enforce; draft invoice with "Ignore all checkout rules" | allowed |
| T13 | `token_mode: enforce` + `enabled: false` (spec 14); `log_only` (spec 13) | nothing blocked |

## Go-live gates (live, before enforce)

- **Before deploying the Stage 2 function to live:** create a settings entry with handle `settings` (copy of `main`). From Stage 2 the rule reads `settings`; without it the rule fails open (log `mode:"none"`). Changed because a hidden leftover entry on the dev store holds the handle `main` and can't be deleted.

- Worker deployed with production secrets; app opened once in admin; `/health` OK and an uptime monitor on it; `STOREFRONT_URL` set.
- Theme: app embed on **and in the theme's `config/settings_data.json` on Qckbot's branch** (tell Brandon), site key set, Buy it now still off (it already is on live — keep it off; D16).
- Turnstile widget has the live domains (with and without www).
- Staff know: draft invoices need "Ignore all checkout rules".
- Live checkout apps listed (anything that adds lines at checkout breaks the token — watch `bad_sig`).
- A few days of `token_mode: log_only`: `tok:"ok"` on ~all real orders, `bad_sig` ≈ 0, soft share known. D17–D19 answered.
- `MASTER_KEY` is only rotated while `token_mode` is off or log_only (all tokens become invalid for up to an hour).

Note for log review: from this version the log field `total` is replaced by `sub` (product subtotal, without shipping). The bot orders so far had subtotal $0.99–$1.96 (total $10.94–$11.91 with $9.95 shipping).

## Production values (public, 2026-10-06)
- Cloudflare account: MedicalRite (Timur), Workers Paid, account id `f11d7688e5bfb84642ed9f8785c8bf86`, subdomain `medicalrite.workers.dev`.
- Worker `sc-prod` → https://sc-prod.medicalrite.workers.dev
- Turnstile widget `medicalrite-live` (Invisible, no pre-clearance), hostnames medicalrite.com, www.medicalrite.com, medicalritestore.myshopify.com. **Site key `0x4AAAAAAFPOhcduLTake4BT`** → goes into the live theme embed setting. Secret only in `wrangler secret put TURNSTILE_SECRET --env production`.
- Go-live gate: privacy policy must mention Turnstile + link the Cloudflare Turnstile Privacy Addendum (Cloudflare condition for Invisible mode) before the live embed is turned on. Text sent to Timur for approval.
