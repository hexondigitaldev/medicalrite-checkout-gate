# Run A — normal storefront checkout (2026-09-23, dev store)

Order D65D50AZK, 1 × $2.00 item, guest, Test gateway. Probe did not block (expected).
Source: `.shopify/logs/20260923_1847*`–`1849*` (22 function runs).

## Observations

1. **Theme-set cart attribute survives to checkout.** `_bg_probe` (set by the theme app embed via `/cart/update.js`) was present in every validation and payment run, including the final `CHECKOUT_COMPLETION` run.
2. **Cart token format** seen by the theme: `hWNHActoCzT46a2nOSu3KgNt?key=8198a2…` (new-style token with `?key=`).
3. **Cart line IDs are positional, not unique:** `gid://shopify/CartLine/0`, `/1` … They restart at 0 for every cart → **cannot be used to bind a token to one cart** (kills Q2 option 1).
4. **Validation runs at every step**, not just checkout: `CART_INTERACTION` (storefront add-to-cart / cart page), `CHECKOUT_INTERACTION` (many times while typing), then `CHECKOUT_COMPLETION` once on Pay. → Part 1 must return **no errors unless `step == CHECKOUT_COMPLETION`**, or it will break add-to-cart.
5. **Guest buyer** appears as `buyerIdentity.customer = null` (not `numberOfOrders: 0`). Email appears only once entered.
6. **Delivery address** is null until entered; `selectedDeliveryOption.title` gives the rate name ("Standard" here).
7. **Payment methods on dev store:** "Deferred", "(for testing) Bogus Gateway", "Stripe shared token", "Gift card" — all `PAYMENT_METHOD` placement. Live Authorize.net/Google Pay names still unknown (Q1 → live probe).
8. **Cost:** validation ≈ 19–27k instructions, payment ≈ 50k. Shopify's limit is 11M → HMAC-SHA256 verification fits easily.
