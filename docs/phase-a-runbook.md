# Phase A runbook — run the probes on the dev store

The probes **never block** and **hide nothing by default**. They only make Shopify record what the functions can see.

## 1. One-time setup (your machine)

Requirements: Node ≥ 22.12, Rust (`rustup`), Git.

```bash
git clone https://github.com/hexondigitaldev/medicalrite-checkout-gate
cd medicalrite-checkout-gate
rustup target add wasm32-unknown-unknown
npm install
npm run config:link     # log in to Partners, choose "Create new app", name it "MedicalRite Bot Gate"
npm run dev             # choose the "Medicalrite Dev" store, install the app when prompted
```

`npm run dev` builds the two functions to Wasm and pushes all three extensions to the dev store.

## 2. Turn the probes on

1. **Theme probe:** Online Store → Themes → Customize → App embeds → turn on **Bot Gate probe** → Save.
2. **Validation probe:** Settings → Checkout → Checkout rules → Add rule → **Bot Gate probe: validation** → Turn on.
3. **Payment probe:** in the terminal running `npm run dev`, press `g` to open GraphiQL and run:

```graphql
query { shopifyFunctions(first: 10) { nodes { id title apiType } } }
```

Copy the id of **Bot Gate probe: payment**, then:

```graphql
mutation {
  paymentCustomizationCreate(paymentCustomization: {
    title: "Bot Gate probe", enabled: true, functionId: "PASTE_ID"
  }) { paymentCustomization { id } userErrors { message } }
}
```

## 3. Run these checkouts (Test gateway card number `1`)

| # | Flow | How |
|---|---|---|
| A | Normal checkout | Browse store → add a <$3 item → checkout → complete |
| B | Permalink (bot door 2) | New incognito window → `https://<store>.myshopify.com/cart/<variantId>:1` → complete |
| C | Buy it now | Product page → Buy it now → complete |
| D | PayPal express | Cart → PayPal button (sandbox) → complete |
| E | Returning customer | Log in as a customer with a past order → checkout |
| F | Draft order | Admin → Orders → Create order → Send invoice → pay from the invoice link |

For each run, open the browser console on the store page and copy the `[bot-gate-probe]` lines.

## 4. Collect the logs

Partner dashboard → Apps → MedicalRite Bot Gate → Extensions → each function → **Runs**. For each flow, copy the input JSON of the run (or screenshot it). Look for:

- `_bg_probe` present? (Q3)
- The `lines[].id` values vs the `items` keys in the console (Q2 line-ID binding)
- Anything identifying draft orders / returning customers (Q4)
- `paymentMethods` names, ids, placements (Q1)

Paste them into a new file `docs/phase-a-evidence/` (strip customer emails/addresses first) or send them to me, and I'll fill in `docs/phase-a-findings.md`.

## 5. Optional: prove hiding works (dev store only)

In GraphiQL (press `g` in the `npm run dev` terminal, so it runs as the app), set the probe config on the payment customization you created:

```graphql
mutation {
  metafieldsSet(metafields: [{
    ownerId: "PAYMENT_CUSTOMIZATION_ID",
    namespace: "$app:bot-gate-probe", key: "config", type: "json",
    value: "{\"hideNames\": [\"<exact method name from the log>\"], \"placements\": [\"PAYMENT_METHOD\"]}"
  }]) { userErrors { message } }
}
```

Reload checkout → that method should disappear. **Never set this on the live store.**
