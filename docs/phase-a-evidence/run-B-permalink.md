# Run B — cart permalink `/cart/49227620516023:1` in a fresh incognito window (2026-09-23)

Store password entered first, then the permalink. 18 function runs (`.shopify/logs/20260923_1851*`–`1852*`).

## Observations

1. **No `_bg_probe` at any step, including `CHECKOUT_COMPLETION`.** The permalink path never runs the theme script → a "token required" rule would block bot door 2 as designed. (The password page did not set the attribute either, so the test is clean.)
2. **Same email as run A now resolves to a customer record** (`Customer/9015532060855`) with **`numberOfOrders: 0`**, even though run A placed an order with that email ~3 minutes earlier. Either the count updates with a delay, or guest orders matched by email aren't counted.
3. **Security concern for the spec's exemption rule:** the customer is attached just by typing an email, without login (`isAuthenticated: false`). If "returning customer (`numberOfOrders ≥ 1`) skips token + score" applies to unauthenticated buyers, a bot could type any real customer's email and skip the gate. Proposal: skip the **score** for returning customers, but only skip the **token** when `isAuthenticated = true`. → Timur.
