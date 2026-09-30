# Live probe results (medicalritestore.myshopify.com, Sep 25–28, 2026)

Method (read-only): order times/channel/total read from the Shopify admin order list; for each order, the probe-validation runs in a ~1–3 min window before it were pulled from the Dev Dashboard and the `CheckoutCompletion` (Pay step) run found. No customer names/emails/addresses recorded.
Volume: ~89,000 function runs in 3 days (mostly cart / checkout-typing steps).

## Pay-step runs matched to orders

| Order | Type | Theme value (`_bg_probe`) at Pay | Cart |
|---|---|---|---|
| MR53791 | web | present | 1 line, qty 6, $65.94 |
| MR53790 | web | present | 1 line, qty 1, $31.99 |
| MR53789 | web | present | 1 line, qty 1, $31.99 |
| MR53788 | web | present | 1 line, qty 1, $75.99 |
| MR53787 | web | present | 1 line, qty 1, $15.99 |
| MR53786 | web | present | 1 line, qty 1, $28.99 |
| MR53785 | web | present | 1 line, qty 1, $178.99 |
| MR53783 | web | present | 2 lines, qty 2, $47.98 |
| MR53775 | web | present | 2 lines, qty 2, $37.98 |
| MR53770 | web | present | 1 line, qty 4, $7.96 |
| MR53756 | web | present | 1 line, qty 1, $5.99 |
| MR53749 | web | present | 1 line, qty 1, $259.99 |
| MR53746 | web | present | 1 line, qty 15, $14.85 |
| **MR53747** | **bot** (james anderson, Evereye_Fraud) | **missing** | 1 line, qty 1, $0.99 |
| **MR53750** | **bot** (james anderson, Evereye_Fraud) | **missing** | 1 line, qty 1, $0.99 |
| MR53784 | Recurpay renewal | **no Pay-step run at all** | — |
| MR53778 | Recurpay renewal | **no Pay-step run at all** | — |

## Conclusions

1. **Token design works on live:** 13/13 real web orders carried the theme value to the Pay step; 2/2 bot orders did not → a token-required rule would have blocked both bots and none of the sampled real customers.
2. **Recurpay renewals don't run the checkout rule** (D6 answered): no Pay-step run for either renewal.
3. Small real orders ($5.99, $7.96 subtotal) carried the value — the token doesn't hurt cheap legit orders.
4. A single checkout can produce 2–5 Pay-step runs (retries / repeated Pay clicks) → dedupe when counting.
5. Not yet checked: which of these orders used **PayPal or Google Pay express** (payment method isn't in the order list). Needs a look at a few order pages' payment method, or a deliberate test purchase.
6. Heavy background traffic hits the cart (≈150 runs/min at times); Stage 1 exits early on non-Pay steps, so cost is low.

## Express checkout test (Sep 28, placed by Timur, then cancelled/refunded)

| Order | Payment | `_bg_probe` on the order (Additional details) |
|---|---|---|
| MR53822 | PayPal express (checkout page) | **present** |
| MR53824 | Google Pay express (checkout page) | **present** |

→ **Q3 answered for express buttons on the checkout page: PayPal and Google Pay keep the theme-set cart attribute.** Not tested: express buttons on the product page (same as Buy it now — creates a new cart; covered by D2).

Side finding: cart attributes appear on the order under "Additional details", so during log-only/enforce we can spot-check any order directly for the token.
