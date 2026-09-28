# Run C — "Buy it now" from a product page, normal browser window (2026-09-23)

22 function runs (`.shopify/logs/20260923_1853*`–`1854*`).

## Observations

1. The theme probe **did** run on the store pages (it set `_bg_probe` on the browsing cart).
2. Clicking **Buy it now** started checkout on a **new cart** containing only that product (variant …352183) with **no `_bg_probe`**, all the way to `CHECKOUT_COMPLETION`.
3. → With "token required", **every real Buy-it-now purchase would be blocked**. This answers Q3 for Buy it now: **attributes are not kept.**
4. The same email is still attached to `Customer/9015532060855` with `numberOfOrders: 0` after two completed guest orders → guest orders matched by email don't appear to count (or update much later). Don't rely on `numberOfOrders` for guests.

## Options for Buy it now (for Timur)

- **A. Replace the button (recommended):** hide Shopify's dynamic checkout button on product pages and show our own "Buy it now" that adds the item to the normal cart (which carries the token) and goes straight to `/checkout`. Same one-click feel, token preserved.
- **B. Remove Buy it now** from product pages before launch (the brief's fallback).
- **C. Exempt it:** not possible safely — the function can't tell a Buy-it-now cart from a bot cart.
