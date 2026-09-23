# MedicalRite Checkout Bot Gate

A small custom Shopify app that stops card-testing bots at MedicalRite's checkout **before a card reaches Authorize.net**, without real customers noticing.

- Full spec: `docs/spec.md` (from Timur's brief, 2026-09-22). It is the source of truth; if this file and the spec disagree, the spec wins — flag the conflict.
- Current phase: `docs/phase-a-spike.md`. **Do not build beyond the current phase.**
- Stakeholders: Timur (owner of the brief, approves phase changes and go-live), Hexon (engineer, reviews all code).

## The three parts

| Part | Shopify surface | Job |
|---|---|---|
| 3 · Browser token | Theme app extension (JS) + app proxy `/apps/bot-gate/token` + app server | Cloudflare Turnstile → server verifies → returns HMAC-SHA256 token (cart binding method pending — see Q2 in `docs/phase-a-findings.md`) → theme saves it as cart attribute `_bg` |
| 1 · Checkout rule | Cart & Checkout Validation Function | Hard-blocks at `CHECKOUT_COMPLETION`: no/invalid/expired token, blocklisted name/address/email domain, broken address |
| 2 · Hide card | Payment Customization Function | Valid token but risk score ≥ `hide_card_score` → hide the Authorize.net card method |

Parts 1 and 2 **must import one shared scoring module** so they never disagree.

## Hard rules (never violate)

1. The HMAC signing secret never reaches the browser, the theme, or any storefront-readable metafield. It lives in an app-owned (`$app:` namespace) metafield and server env only.
2. Never hard-block a real visitor because Turnstile failed or was slow → issue a **soft** token instead (Part 1 lets it through, Part 2 adds +2).
3. Exempt: returning customers (`numberOfOrders ≥ 1` skips token + score), draft/staff orders, B2B (purchasing company set). Logged-in customers skip the score but still need the token.
4. Validation runs only at checkout, never on add-to-cart/cart page.
5. One generic block message for every rule (bots must not learn which rule fired):
   "We couldn't verify this checkout. Please refresh the page and try again, or call us at [number] and we'll help."
6. No URL/query-string bypass of any kind.
7. `mode: "log_only"` must never block; `enabled: false` must let everything through. Both are the kill switch.
8. Config comes from shop metafield `bot_gate.config` (JSON) — changing rules/thresholds must never require a redeploy.
9. Log every block and would-be block with rule name + cart identifier.
10. Constraint: no Shopify Payments. Gateway is Authorize.net. Store is Shopify Plus (required for custom apps with Functions).
11. Never commit secrets, API keys, `.env` files, or customer PII (the bot-order CSV stays out of git; use anonymised fixtures).

## Tech choices

- Shopify CLI v4, extension-only app for Phase A; function API version 2026-07.
- Functions in **Rust** (Shopify's recommendation for instruction limits; HMAC in Wasm). If JS is chosen, justify it against instruction-count measurements.
- Keep the app server minimal: app proxy endpoint, settings page, decision log.

## Environments

- Dev/staging: `Medicalrite Dev` (Plus dev store), Shopify **Test payment gateway** + PayPal sandbox. Authorize.net sandbox cannot connect to Shopify, so anything Authorize.net-specific is verified on live with a no-op/log-only build.
- Production: MedicalRite live store. Live checkout today shows: express PayPal + Google Pay; payment step Credit card (Authorize.net) + PayPal. Google Pay also goes through Authorize.net — open question for Timur whether Part 2 hides it too.
- Production deploys only after QA sign-off, and always start in `log_only`.

## How we work (builder / critic)

- **Builder**: implements one part per branch/PR, with unit tests for every rule and every test case in `docs/spec.md` that can be unit-tested. Updates `docs/decisions.md` for any non-obvious choice.
- **Critic** (`.claude/agents/critic.md`): reviews each PR against this file and the spec; opens concrete issues; repeats until nothing is left.
- A PR is done when: tests pass, critic has no open findings, Hexon has reviewed, and the relevant spec test cases are ticked in the PR description.
- Ask rather than guess on anything that changes customer-facing behaviour.
