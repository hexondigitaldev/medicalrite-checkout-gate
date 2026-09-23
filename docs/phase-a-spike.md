# Phase A — Dev-store spike

Goal: answer the six open questions in `docs/spec.md` with evidence, then report to Timur. **No production-quality build in this phase.** Throwaway code is fine; findings are the deliverable.

## Already known (from research, 2026-09-23)

- Custom apps containing Shopify Functions only run on **Shopify Plus**. Dev store `Medicalrite Dev` is created on the Plus plan. Confirm live MedicalRite is Plus.
- Validation Function docs list **Draft Orders** and **Storefront Accelerated Checkout** as supported surfaces → draft orders *will* hit the rule unless exempted; express checkouts *will* be validated.
- Validation Function docs list **Subscriptions** as unsupported → Recurpay renewals likely bypass the rule (still confirm).
- Authorize.net **sandbox credentials are rejected** by Shopify's Authorize.net connect page (known issue). Dev store uses Shopify's Test payment gateway + PayPal sandbox instead.
- Live checkout payment options: express PayPal + Google Pay; payment step Credit card (Authorize.net) + PayPal. No Shop Pay. Google Pay routes through Authorize.net.
- A Payment Customization Function can hide methods but likely can't show the "please call us" notice by itself → probably needs a Checkout UI extension. Verify.

Findings so far: `docs/phase-a-findings.md`. How to run the probes: `docs/phase-a-runbook.md`.

## Setup

1. `shopify app init` (custom app, linked to the Partner org), install on `Medicalrite Dev`.
2. Dev store: products under $3 (mirror bot SKUs) + a $0.99 item + a $59.99 glove item; $9.95 Flat Rate Ground shipping; Test payment gateway; PayPal sandbox; customer accounts on; one returning test customer with a prior order.
3. Scaffold two probe functions that **only log their full input** (return no errors / no operations):
   - `probe-validation` (Cart & Checkout Validation)
   - `probe-payment` (Payment Customization)
4. `shopify app dev` / `shopify app function` logs to capture inputs.

## Questions → how to answer → record

| # | Question | Experiment | Record |
|---|---|---|---|
| Q1 | Card method visible in Payment Customization input and hideable? | Dev: log `paymentMethods`, try hiding Test gateway card. **Live (needs Timur OK):** deploy log-only `probe-payment` that hides nothing; capture method names/IDs incl. Credit card, PayPal, Google Pay | Exact method names/IDs; whether hide works; whether Google Pay/express wallets are affected |
| Q2 | What cart ID / time can the rule read? | Inspect the input schema for the API version; log everything available (cart id? attributes, `localTime`?) | Fields available for binding + expiry. If no cart id: propose alternative binding and its replay risk |
| Q3 | Do express checkouts keep theme-set attributes? | Set `_bg` test attribute from theme; run PayPal express, Buy it now, (Google Pay on live only) and check `probe-validation` input | Per flow: attribute present Y/N; proposed handling |
| Q4 | Draft orders / Recurpay through the rule? | Create + pay a draft order invoice; check probe log. Recurpay: check docs + ask Timur whether a test renewal can be triggered | Y/N per flow; which input fields identify them for exemption |
| Q5 | Where do logs live? | Evaluate function run logs retention/export; evaluate Web Pixel `alert_displayed` event and app-server logging for decisions | Recommendation Timur can use for a week of review |
| Q6 | Rate limit token requests per IP? | Check what the app proxy passes (client IP header); design a limit | Recommendation + limit values |

Also measure: instruction count of HMAC-SHA256 verification inside a Rust function (must fit Functions limits).

## Deliverable

`docs/phase-a-findings.md` — one section per question: answer, evidence (log snippets / screenshots), impact on the design, and any spec change proposed. Send to Timur; wait for his go-ahead before Phase B.
