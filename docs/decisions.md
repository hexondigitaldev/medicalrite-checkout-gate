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
