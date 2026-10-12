# Decisions log

## 2026-09-24 — Timur's answers to the Phase A report (Slack)

| # | Topic | Decision |
|---|---|---|
| D1 | Token binding | **Approved:** token expires in 30–60 min, is bound to the cart contents, and is refreshed on every page load and every cart change so slow shoppers never get blocked. |
| D2 | Buy it now | Replace with our own button that keeps the token. If not ready at launch, **remove Buy it now** until it is. |
| D3 | Exemptions | Only **logged-in** customers skip the token (unauthenticated email match is spoofable). Draft orders use Shopify's "Ignore all checkout rules" invoice option. |
| D4 | Google Pay | Hide Google Pay **together with the card** on high-risk carts. PayPal stays visible. |
| D5 | App errors | **Fail open:** if the app errors, checkout goes through; the blocklist keeps running; send an alert. (Checkout rule setting "Block checkout if app experiences a problem" = OFF.) |
| D6 | Recurpay | Probably not validated — **confirm during log-only** by checking whether renewals appear in the logs. |
| D7 | Rollout order | **Blocklist first**, log-only for only a few days, then enforce. Token follows right behind (Sep 9 wave rotated names/addresses). |
| D8 | Blocklist additions | Add address "123 Main St" and ZIP **10080** (MR50664 confirmed bot). Result on the CSV: blocklist 461/495, all layers **495/495**. |
| D9 | Broken-address rule | **Dropped** (caught 0/495; blocklist covers it). |
| D10 | Live store probe | **Approved.** Tell Timur when it is installed and when it is removed. |

## Engineering decisions (ours)

- Part 1 only returns errors at `buyerJourney.step == CHECKOUT_COMPLETION` (it also runs on cart and checkout interaction — see run A).
- Blocklist matching is normalized (lower-case, punctuation stripped, whitespace collapsed, common abbreviations: west→w, street→st, avenue→ave, etc.).
- Functions in Rust, API 2026-07.

## 2026-09-25 — Stage 1 engineering decisions (after critic review)

- **Settings storage (replaces shop metafield `bot_gate.config` from the spec):** app-owned metaobject `$app:bot_gate_settings`, entry `main`, fields `enabled` (boolean kill switch) + `config` (JSON). Reason: staff can edit it in Content → Metaobjects without a server or redeploy. Only two fields because Shopify's input query cost limit is 30 and each metaobject field costs 4 (7 fields ≈ 37 → rejected). Stage 1 query ≈ 21.
- **Stage 2 HMAC keys must NOT go in this merchant-editable metaobject** — separate `$app` metafield without admin access.
- **Address matching:** whole street line after normalization; only unit designators (apt/ste/unit/fl/rm or a bare number from "#4") may follow. Address + ZIP rules US-only. Names normalized without street abbreviations.
- **Logged-in buyers skip the blocklist** by default (`skip_logged_in: true`) — ask Timur (spec: logged-in customers are never blocked).
- **Broken settings fail open and are reported** in the log line (`cfg_errors`, list counts `n`).
- Log line carries order-matching hints without PII: total, line count, quantity, logged-in.

## 2026-09-29 — Timur's answers before go-live (Slack)

| # | Topic | Decision |
|---|---|---|
| D11 | Logged-in customers | Skip the blocklist (`skip_logged_in: true`); would-be hits are still logged as `exempt_logged_in`. |
| D12 | Real-building addresses | Tie them to their ZIP: `428 w 45th st|10036`, `230 w 55th st|10019`, `123 main st|10080` (the only ZIPs the bots used — 98, 132 and 1 orders). `428 st` stays unscoped. |
| D13 | Support phone | Use the number on the site: (800) 548-6877. |
| D14 | Go-ahead | Approved to create the production app and go live in log-only. |

## 2026-09-30 — Stage 2 start

| # | Topic | Decision |
|---|---|---|
| D15 | Cloudflare account | Hexon creates a free account for building/testing. If a paid plan is needed, Timur sets one up for MedicalRite. |
| D16 | Buy it now | Hide it for now (D2 option "remove until replacement is ready"). |

## 2026-09-30 — Stage 2 engineering decisions (builder)

- **Token:** `1.<window>.<h|s>.<sig>`, sig = first 128 bits of HMAC-SHA256 over the window, hard/soft flag and the cart content (`variantId:qty`, summed per variant, sorted). Bound to cart contents (D1), not a cart ID (none in the function input, Q2).
- **Keys and expiry:** one key per 30-min window, derived on the server from `MASTER_KEY` (Worker secret, never published). The server publishes only the keys for previous/current/next window to metafield `$app:sc.keys` on the checkout rule (validation) every 10 min (was the shop until dev test T9 showed shop `$app` metafields are readable from theme Liquid). The rule accepts a token while its window key is published → lifetime 30–60 min. Tradeoff: published window keys are visible to app developers in Dev Dashboard run logs (not to merchants or storefront); each key is only useful for ~90 minutes.
- **Soft tokens (spec rule 2, test 12):** if Turnstile fails, errors, or takes >6 s, the server still issues a token flagged `s`. Policy `soft_tokens`: `allow` (default, spec), `risky` (fails only on the bot cart shape), `block`. Rate limits: 30/min/IP overall and 10/min/IP for soft (refused with 429); a store-wide soft budget of 60/min never refuses (that would block real soft buyers) but logs `soft_over_budget`. Failures on Cloudflare's side never count. Open question D17.
- **Fail open (D5):** the server writes the rule's input variable `freshUntil` (shop time, ~70 min ahead) on every publish; once it passes, the token check lets everyone through (`keys_stale`). Also: keys missing → allow; publish date 2+ days old → allow; publishing behind → server signs with the newest window the rule has. `/health` returns 503 with reasons.
- **Logged-in buyers never need a token (D3)**, independent of `skip_logged_in` (which only controls the blocklist, D11).
- **Logs:** block lines list `enf` (rules actually enforced) separately from all matched `rules`; `tid` (window + 8 sig chars) and `tage` match function logs to Worker logs. `sub` (product subtotal) replaces `total`.
- **Separate `token_mode`** (default off) so deploying Stage 2 changes nothing until switched on.
- **Server:** Cloudflare Worker (free plan). App proxy `/apps/sc/t`, signature verified with the app secret; admin access by token exchange when the app is opened in admin (offline token kept in Worker KV).
- **Theme:** app embed "Storefront helper" (neutral names), reads `/cart.js`, refreshes the token on load, after every cart change and every 20 min, and holds the checkout button (max 8 s) while a refresh is running.
- **Found during Stage 2 build:** the ZIP-scoped address change (D12) had not been saved to the repo, so live runs without it (ZIP-scoped entries never match; `428 st` and ZIP 10080 still work; live is log-only so no customer impact). Restored on `stage-2/token`; must be deployed to live before enforcing the blocklist.

- **2026-10-01: settings entry handle is now `settings` (was `main`).** On the dev store a hidden leftover entry kept the handle `main` after `app dev clean` + reinstall and cannot be deleted. Live must get a `settings` entry before the Stage 2 function is deployed there.
- **2026-10-01: Buy it now is already off on live.** Checked 4 live product pages (medicalrite.com): no Buy it now / dynamic checkout button; only the cart-type express buttons (`shopify-accelerated-checkout-cart`, cart drawer), which keep the token (Phase A). D16 needs no change now — just keep it off (tell Brandon/Qckbot not to turn it on).

## Timur's answers (2026-10-05, Slack)
- **Stage 1: blocklist to enforce — approved.** Live entry `main` switched `log_only` → `enforce` on 2026-10-05. Checked on live: guest checkout to `428 W 45th St 10036` shows "We couldn't verify this checkout… (800) 548-6877" at Pay, no charge.
- **D17 soft tokens:** keep allowing them (`soft_tokens: allow`); decide after a few days of log-only data.
- **D18 Cloudflare paid plan ($5/month):** approved. Asked Timur for a MedicalRite Cloudflare account (Workers Paid); until then `sc-prod` runs on Hexon's account in log-only, moved before token enforce.
- **D19 refresh timing** (renew under 25 min left or on cart change): approved.
- Timur: keep watching performance and keep improving as data comes in.

## Stage 2 round 2 (2026-10-08, after live review #1)
- **Draft-invoice exemption: tried and dropped (dev test 2026-10-08).** A guest who types an existing customer's email at checkout gets that customer attached (`buyerIdentity.customer` set, `isAuthenticated` false), so a cart link with no ticket passed as `exempt_customer` — bots could use any real customer's email. Kept only as the log field `"cust"`. Draft invoices: staff tick **"Ignore all checkout rules"** when sending (Shopify-supported), or the buyer logs in. In the dev test the invoice checkout itself showed `authed:true` (`exempt_logged_in`) — confirmed 23:36 in a fresh incognito: invoice links sign the buyer in (`isAuthenticated:true`), so invoices with a customer are exempt; only invoices without a customer need "Ignore all checkout rules".
- **Soft-ticket share (38% on live):** sf.js now (a) warms Turnstile up on page load and keeps one answer ready (used within 240 s, single use), (b) on a 4 s timeout still gives a soft ticket but keeps listening; a late answer swaps it for a hard ticket in the background, (c) a soft ticket already on the cart is swapped on the next page when Turnstile answers. The 5-minute soft floor still applies when Turnstile actually fails.
- **`read_products` scope** added (prod + dev) so live run logs are visible in the Dev Dashboard (Stage 2 input reads variant ids). Needs a one-time permission approval in the live admin after deploy.
