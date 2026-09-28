# Phase B plan

Order approved by Timur (D7): **Stage 0 live probe → Stage 1 blocklist → Stage 2 token → Stage 3 hide card/Google Pay.**
Each stage: plan → build (builder agent) → review (critic agent) → Hexon review → dev store test → live `log_only` → enforce.

---

## Stage 0 — Live store probe (approved, D10)

Goal: exact live names for the Authorize.net card method and Google Pay; check PayPal / Google Pay express flows keep the theme attribute.

1. Dev Dashboard → app → Distribution → **Custom distribution** → `medicalritestore.myshopify.com`. Send the install link to Timur (store staff approve the install).
2. `npm run deploy` → release the version (probes only: validation + payment + theme embed).
3. Turn on the probes on live: checkout rule "Bot Gate probe" (Active, "Block checkout if app experiences a problem" OFF); theme embed ON. **Payment probe deferred:** `shopify app execute` only allows mutations on dev stores, so on live a payment customization can only be created by our own app (admin page) → comes with the Stage 2 server.
4. Collect runs for ~24 h with `npx shopify app logs --store medicalritestore.myshopify.com > data/live-probe-logs.jsonl` (git-ignored) plus Dev Dashboard run logs. Test one PayPal express and one Google Pay express order ourselves.
5. Remove: delete the payment customization, deactivate the checkout rule, turn off the embed. **Tell Timur when in and when out.**

Probes never block and hide nothing (payment probe config empty on live).

---

## Stage 1 — Blocklist checkout rule (build next)

### Scope
New Rust function extension `checkout-rule` (Cart & Checkout Validation, API 2026-07). **No theme code, no server.**

### Behaviour
- Reads shop metafield `bot_gate.config` (JSON, editable in admin without redeploy). Missing/invalid config → allow + log `config_error` (fail open, D5).
- Returns errors **only** when `buyerJourney.step == CHECKOUT_COMPLETION`.
- `enabled: false` → allow everything. `mode: "log_only"` → never block, log would-be blocks.
- Exempt: `buyerIdentity.purchasingCompany` set (B2B). Draft orders: Shopify invoice toggle (D3).
- Rules (any match → block), all matched after normalization (lower-case, strip punctuation, collapse spaces, abbreviations: west→w, east→e, north→n, south→s, street→st, avenue→ave, road→rd, boulevard→blvd, drive→dr):
  - `blocked_address1` vs every delivery address `address1`
  - `blocked_zips` vs delivery ZIP (first 5 digits)
  - `blocked_names` vs delivery `firstName + lastName` (or `name`)
  - `blocked_email_domains` vs buyer email domain
- One generic message for every rule, target `$.cart`:
  "We couldn't verify this checkout. Please refresh the page and try again, or call us at [number] and we'll help." (phone number from config `support_phone`).
- Log one line per completion-step decision: `{"v":1,"decision":"block|would_block|allow","rules":[...],"mode":...}` — no PII in the log line.

### Config at launch
```json
{
  "enabled": true,
  "mode": "log_only",
  "support_phone": "<ask Timur>",
  "blocked_names": ["james anderson"],
  "blocked_address1": ["428 st", "428 w 45th st", "230 w 55th st", "123 main st"],
  "blocked_zips": ["10080"],
  "blocked_email_domains": []
}
```

### Tests (Rust unit tests, fixtures from the spec + CSV patterns, no real PII)
Spec cases 5, 6, 13, 14 plus: "230 West 55th Street" normalizes and matches; "123 Main St." matches; ZIP 10080 and 10080-1234 match; non-completion steps never block; B2B exempt; malformed config → allow; $59.99 real address allowed; `log_only` produces `would_block`.
CSV regression: `scripts/score_bot_orders.py` stays in sync (blocklist ≥ 461/495).

### Open items for Stage 1
- **Log visibility:** Dev Dashboard hides a run's details (including our log line) unless the app has the Admin API scopes for every field in the input query. The rule reads addresses/names/emails → add `read_customers`, `read_products` (if product fields are used) and complete the Protected customer data request in the Dev Dashboard; Timur approves the scope update on live.
- **Support phone number** for the message → Timur.
- **Name rule risk:** "james anderson" is a common real name, and on the CSV it adds **zero** extra catches beyond the address rule (461 with or without it). Proposal: keep it in `log_only` for a week and drop it if it never fires alone. → mention to Timur.
- **Error alert (D5):** functions can't send alerts. Proposal: daily check of function errors in Dev Dashboard during rollout; automated alert arrives with the Stage 2 server.
- **Log review (Q5):** Stage 1 uses Dev Dashboard run logs + `shopify app logs` exported to a file and summarized by a script; revisit when the server exists.

### Rollout
Dev store tests → deploy → live `log_only` (a few days, D7; watch for Recurpay renewals, D6) → review with Timur → `mode: "enforce"`.

---

## Stage 2 — Browser token (after Stage 1 is live)

Includes the **minimal app server** (hosted): app proxy for tokens, embedded admin page with a settings screen for `bot_gate.config` and switches to create/enable payment customizations (payment probe, then Stage 3 hide-card).

Turnstile in theme embed → app proxy `/apps/bot-gate/token` → server verifies with Cloudflare → HMAC token over (cart contents hash, issued-at, key id) with 30-min rotating keys in an app-owned metafield (D1); refresh on every page load and cart change; soft token on Turnstile failure; logged-in customers skip (D3); our own Buy it now button or remove Buy it now (D2); per-IP rate limit on the proxy; error alerting.

## Stage 3 — Hide card + Google Pay (after Stage 2)
Payment Customization: valid token + score ≥ `hide_card_score` → hide the Authorize.net card method and Google Pay (both placements), keep PayPal (D4); notice via Checkout UI extension. Shared scoring module with Stage 1/2. Free-mail list includes proton.me, protonmail.com, yandex.com, mail.com, zoho.com, etc.

---

## Release process (from Stage 1 on)

- **Two apps:** current "Medicalrite Bot Gate" = **dev/test app** (dev store only). A **new production app** with a neutral name, installed only on `medicalritestore.myshopify.com`, config in `shopify.app.production.toml`.
- The current app is removed from the live store when the production app goes live (after the Stage 0 probe is finished).
- Production: `shopify app deploy --config production --no-release` → final check → `shopify app release --config production --version=<v>`.
- Only deploy production from `main` after critic + Hexon review. Every production release starts with `mode: "log_only"`.
- Until then, `npm run deploy` on the current app releases to **both** dev and live — acceptable only for the never-blocking probes.
