# Phase A findings (in progress)

Status legend: **Answered (desk)** = from Shopify's 2026-07 function schemas/docs, still to confirm on the dev store · **Open** = needs the probes.

## Q1 — Can the Authorize.net card method be hidden? — Partly answered (desk), confirm on live

- The Payment Customization input lists every method with `id`, `name` and `placements` (`PAYMENT_METHOD`, `ACCELERATED_CHECKOUT`).
- `paymentMethodHide` accepts optional `placements`, so we can hide the card in the payment step **and** Google Pay in the express area, independently.
- There is **no "show a message" operation**. The notice "For this order, please call us…" needs a Checkout UI extension (Plus) or a `paymentMethodRename` workaround. To decide in Phase B.
- Still needed: the exact `name` Shopify gives the live Authorize.net card method and Google Pay → install `probe-payment` on live (hides nothing by default) with Timur's OK.

## Q2 — What cart ID / time can the checkout rule read? — Answered (desk): **no cart ID**. Design impact.

- The validation `Cart` input has attributes, buyer identity, cost, lines (each with a line `id`), delivery groups, metafields — **but no cart ID or cart token**.
- Time: only `shop.localTime.date` (today's date) plus yes/no comparisons against fixed values. No current timestamp.
- The `fetch` target (network call from the function) exists, but Shopify limits it to Shopify for enterprises; not available to Plus custom apps or dev stores.
- **Consequence:** the brief's design ("sign the cart token so a token can't be copied to other carts") can't be enforced inside the function as written. Options for Timur:
  1. ~~**Line-ID binding**~~ — **ruled out (run A):** function line IDs are positional (`CartLine/0`, `/1`…), identical across carts.
  2. **Rotating keys as expiry** — app rotates the signing key every ~30 min (key in app-owned metafield); tokens older than two windows fail. Copying is limited to a short window, not prevented.
  3. **Content binding** — sign the cart's variant IDs + quantities; the theme re-issues when the cart changes. A copied token only works for an identical cart.
  4. **Server-side reuse detection** — `carts/update` webhooks show which cart tokens carry which `_bg`; tokens seen on 2+ carts are added to a revoked list in a metafield. Asynchronous, so it can race a fast bot.
- Recommendation: 2 + 3 together (rotating keys + content binding), with 4 as a later add-on. Needs Timur's OK. Any of these still forces a bot to run a real browser and pass Turnstile, which today's bots never do.

## Q3 — Do express checkouts keep theme-set cart attributes? — Partly answered — normal checkout **keeps** attributes (run A); **Buy it now does not** (run C); PayPal/Google Pay express pending

- The validation API lists "Storefront Accelerated Checkout" as a supported surface, so express checkouts **are** validated — which means a missing token there would block real buyers.
- Probe: `probe-theme` sets `_bg_probe`; `probe-validation` logs whether it arrives, per flow (normal checkout, Buy it now, PayPal express; Google Pay on live).

## Q4 — Do draft orders / Recurpay renewals hit the rule? — Draft orders answered (runs F1/F2); Recurpay pending

- Draft orders: listed as a **supported** surface → the rule runs on them. Must be exempted explicitly. Probe: pay a draft-order invoice and look at the input to find a field that identifies it.
- Subscriptions: listed as **unsupported** → Recurpay renewals should not run the rule. Confirm with Timur/Recurpay.

## Q5 — Where should block logs live? — Open

- Functions can't call our server (see Q2), so a function can only record decisions in its own run log (Partner dashboard / `shopify app function` tooling).
- Candidates to evaluate: function run logs (check retention and export), a Web Pixel listening for checkout error events, and app-server logs of token issuance.

## Q6 — Rate-limit token requests per IP? — Open (Phase B design)

- The app proxy request reaches our server with the client IP, so yes, it's possible. Proposed: per-IP and per-/24 limits on token issuance, logged.

## Other facts gathered

- Custom apps with Functions require Shopify Plus (dev store is Plus; confirm live).
- Authorize.net sandbox keys are rejected by Shopify's Authorize.net connect page → dev store uses Test payment gateway + PayPal sandbox.
- Live checkout shows PayPal + Google Pay (express) and Credit card + PayPal (payment step). Google Pay runs through Authorize.net.

## New facts from run A

- Validation runs at `CART_INTERACTION`, `CHECKOUT_INTERACTION` and `CHECKOUT_COMPLETION`; Part 1 must only error at completion.
- Guest buyers have `customer = null`.
- Function cost is tiny (≤ 50k of 11M instructions).
- Details: `docs/phase-a-evidence/run-A-normal-checkout.md`.

## New facts from run B (permalink)

- Permalink checkouts arrive with **no theme attribute** → token check blocks bot door 2.
- Typing an email attaches the matching customer record **without login**; `numberOfOrders` was 0 right after a prior order. The returning-customer exemption must not skip the token for unauthenticated buyers (spoofable). Needs Timur's decision.
- Details: `docs/phase-a-evidence/run-B-permalink.md`.

## New facts from run C (Buy it now)

- Buy it now creates a fresh cart **without** the theme attribute → real buyers would be blocked. Recommend replacing it with our own button (add to cart → `/checkout`) or removing it. Details: `docs/phase-a-evidence/run-C-buy-it-now.md`.
- `numberOfOrders` stayed 0 after two guest orders with the same email.

## New facts from runs E/F

- Draft invoices **do** run the rule and carry no token; Shopify's invoice option **"Ignore all checkout rules"** skips it at payment → exemption handled by Shopify, staff must keep it on.
- `numberOfOrders` stayed 0 even for a logged-in customer with past orders on the dev store → verify on live before relying on it.
- Details: `docs/phase-a-evidence/run-E-F-logged-in-and-draft.md`.

## Bot-order CSV (495 orders) scored against the spec

- All layers combined catch 494/495 (99.8%). Token alone 80.6%, address blocklist 92.9%, score ≥ 7 91.1%.
- Malformed-address rule catches 0 ("428 st" is exactly 6 chars). W15 homepage wave shows bots can load pages → blocklist essential.
- Details: `docs/phase-a-evidence/bot-csv-analysis.md`.
