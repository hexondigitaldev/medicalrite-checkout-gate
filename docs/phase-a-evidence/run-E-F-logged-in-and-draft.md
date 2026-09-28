# Runs E and F — logged-in customer, draft-order invoices (2026-09-23)

## Run E — logged-in customer (19:01–19:02 UTC)

- `isAuthenticated: true`, customer `9015532060855`, `_bg_probe` present through `CHECKOUT_COMPLETION` (normal cart flow).
- **`numberOfOrders` still 0** for this customer while logged in, after several completed orders. The admin customer card also showed "No orders" when creating the invoice. → `numberOfOrders` can't be trusted as the "returning customer" signal on its own; confirm on live with a real repeat customer.

## Run F1 — draft-order invoice, "Checkout rules" toggle OFF (rules apply) (19:02–19:04)

- Validation **runs** on the invoice checkout, through `CHECKOUT_COMPLETION`.
- No `_bg_probe` (no theme visit) → a token-required rule **would block every draft invoice**.
- Buyer shows `isAuthenticated: true` on the invoice checkout.
- Nothing in our current input query marks the cart as a draft order.

## Run F2 — draft-order invoice, "Checkout rules" toggle ON ("Ignore all checkout rules") (19:04)

- Only `CHECKOUT_INTERACTION` runs were logged, **no `CHECKOUT_COMPLETION` run** → Shopify skips the rule at payment. (Assumes the invoice was paid — confirm.)

## Conclusion for Q4 (draft orders)

- Use Shopify's built-in **"Ignore all checkout rules"** option on invoices; staff must leave it ON (it appeared ON by default). Document this for MedicalRite staff.
- Recurpay renewals: still to confirm with Timur (schema lists Subscriptions as unsupported → likely not validated).
