# Spec: Checkout Bot Gate (MedicalRite)

Source: "Project Brief: Checkout Bot Gate (MedicalRite)", prepared by Timur for the Shopify developer, 2026-09-22. Condensed faithfully; wording kept where it defines behaviour.

## Problem

Bots use MedicalRite's checkout to test stolen credit cards. ~1,400 fake orders since June 2026; 893 on Sept 19–20 alone. Each attempt costs an Authorize.net fee (even when declined), lowers approval rate (Visa/Mastercard fines or merchant-account holds), and creates a fake order + customer to clean up. Gateway filters can't fix it because they act after the bank answers. The fix must live inside Shopify checkout, before the card is sent.

## How bots get in (both skip medicalrite.com entirely)

| Door | Share | Implication |
|---|---|---|
| `POST /cart/add.js` on `medicalritestore.myshopify.com` | ~76% | Nothing on the medicalrite.com domain (theme scripts, firewall, CDN) sees it |
| Cart permalink `/cart/{variantId}:1` | ~24% | Plain GET, no JavaScript runs, straight to checkout |

## Bot order fingerprint

| Signal | Bot value |
|---|---|
| Cart | 1 line, qty 1, product under $3 (6 SKUs so far) |
| Shipping | $9.95 Flat Rate Ground |
| Total | $10.94, $11.91, $12.36 or $13.01 |
| Customer | First order, generic name ("James Smith"), free-mail with digits (`james.smith4881@gmail.com`) |
| Address | Reused drops: "428 st", 428 W 45th St, 230 West 55th Street, New York |
| IP | Rotated: 174 IPs across 146 blocks, <3 orders each |
| Channel | Online Store (`sourceName: web`), not app/API |

## Constraints

- **No Shopify Payments** (restricted Rx items). Gateway is Authorize.net. Must work with a third-party gateway.
- **Small real orders must keep working.** ~1% of real orders are a single item under $2.50; ~4% under $5 (nebulizer cups, mouthpieces).

## Definition of done

Bots are stopped
- [ ] Checkout started from `/cart/add.js` on myshopify.com with no store visit is blocked before payment.
- [ ] Checkout from a `/cart/{variant}:1` permalink with no store visit is blocked before payment.
- [ ] A cart matching the bot pattern is blocked or has the card option hidden, even via the normal store.
- [ ] A blocked checkout creates no Shopify order and no Authorize.net transaction.

Real customers are not
- [ ] A real first-time customer buying one $0.99 item through the store, with a real address, can check out.
- [ ] Returning, logged-in, draft, B2B orders and subscription renewals are never blocked.

We stay in control
- [ ] Rules and thresholds change in minutes, no redeploy.
- [ ] Every block is logged with the reason.
- [ ] A kill switch turns each part off instantly.

## Checkout path

1. Real visitor loads a store page; theme runs an invisible bot check (Turnstile).
2. App verifies it and returns a signed token.
3. Token saved on the cart.
4. A bot using add.js or a permalink skips 1–3 → reaches checkout with no token.
5. At checkout, Part 2: valid token but high risk score → hide card, show call-us message.
6. On Pay, Part 1: no valid token, blocklisted name/address, or broken address → checkout stops. No order, no card sent.
7. Everything else goes to Authorize.net as normal.

## Risk score (shared by Parts 1 and 2)

| Signal | Points |
|---|---|
| First order (or guest with no order history) | +2 |
| Product subtotal under $5 | +2 |
| Single line, quantity 1 | +1 |
| Free-mail address ending in 2+ digits | +2 |
| Soft token (Turnstile failed/slow) | +2 |
| Address line under 6 chars, or with no number | Block |
| Name or address on blocklist | Block |
| No valid browser token | Block |

- Score ≥ 7 hides the card. Every bot order seen so far scores 7.
- A real first-time buyer of one $0.99 item as `jane.doe@gmail.com` scores 5 → goes through.
- Threshold tuned after a week of logs.

## Part 1 — Checkout rule (Cart & Checkout Validation Function)

- Runs on Shopify's servers; returning an error stops checkout, no payment sent. Works with any gateway; can't be bypassed via myshopify.com or permalinks.
- Runs only at `buyerJourney.step = CHECKOUT_COMPLETION` (plus `CHECKOUT_INTERACTION` if needed). Never on the cart page.
- Can see: cart lines & cost, buyer identity (email, customer, numberOfOrders, logged-in state), shipping address, cart attributes (token). Can't see IP/browser; no network calls (hence Part 3).
- Blocks if any: (1) token missing/invalid/expired; (2) shipping name on `blocked_names`; (3) address line on `blocked_address1`; (4) address line < 6 chars or no number; (5) email domain on `blocked_email_domains` (empty at launch).
- Buyer message (same for every rule): "We couldn't verify this checkout. Please refresh the page and try again, or call us at [number] and we'll help."

### Settings — shop metafield `bot_gate.config`

```json
{
  "enabled": true,
  "mode": "enforce",
  "token_required": true,
  "hide_card_score": 7,
  "blocked_names": ["james anderson"],
  "blocked_address1": ["428 st", "428 w 45th st", "230 west 55th street"],
  "blocked_email_domains": []
}
```

`mode: "log_only"` → never blocks, only records what it would have blocked.

### Logging

Log every block and would-be block with rule name and cart token. If Partner-dashboard function logs aren't enough to review a week of traffic, send decisions to a simple log from the app side.

## Part 2 — Hide the card (Payment Customization Function)

- If the cart has a valid token and risk score ≥ `hide_card_score`, hide the Authorize.net card method. Uses the same scoring module as Part 1.
- Customer notice: "For this order, please call us at [number] or choose another payment option."
- PayPal/other wallets stay visible (need a real account, run own fraud checks). *Note: Google Pay on live runs through Authorize.net — pending Timur's decision.*
- Check first: on a store with Authorize.net, confirm the card method appears in `paymentMethods` input and can be hidden. If it can't, the same score triggers a block in Part 1 instead.

## Part 3 — Browser proof token

- Rule: the signing secret must never reach the browser. Token is signed on our server.
- Flow:
  1. Theme loads Cloudflare Turnstile (free, invisible) on every page.
  2. Theme reads cart token from `/cart.js`, sends it + Turnstile result to app proxy `/apps/bot-gate/token`.
  3. Server verifies with Cloudflare; on pass returns token = cart token + time, signed HMAC-SHA256 with server secret.
  4. Theme saves it via `/cart/update.js` as attribute `_bg` (underscore hides it from the buyer).
  5. Part 1 reads `_bg`, verifies signature with the same secret (app-owned metafield the storefront can't read), confirms it belongs to this cart.
- Details:
  - Bind to the cart (one real token can't be copied onto bot carts).
  - Refresh when the cart token changes, and on page load if missing.
  - Fail soft: Turnstile fails/slow → issue a "soft" token; Part 1 lets it through, Part 2 adds 2 points.
  - Rotate safely: keep current + previous key.

## What must never break

| Flow | Risk | What to do |
|---|---|---|
| Buy it now, Shop Pay, PayPal express | Can create a new cart that skips the token script | Test each. Put the token on that cart too, or exempt express checkouts. If neither works, remove those buttons before launch |
| Returning customers | Real buyers | Skip token and score checks when `numberOfOrders ≥ 1` |
| Logged-in customers | Real buyers | Skip score check; still require token |
| Draft orders and staff orders | No store visit | Exempt |
| B2B / company orders | Sometimes no store visit | Exempt when a purchasing company is set |
| Subscription renewals (Recurpay) | Created by the app, not checkout | Confirm renewals don't run through the checkout rule |
| Email/ad links to a cart permalink | Skip the theme → no token | Point them to product pages. No URL bypass (bots would copy it) |
| Real customer blocked by mistake | Lost sale | Message always shows a phone number; every block logged |

## Rollout

Launch quietly: `log_only` on live traffic for at least a few days; switch to `enforce` only after logs show real customers aren't caught.

1. Test the unknowns on a dev store; report back before building further.
2. Build Part 3 (theme script, app proxy, Turnstile, signing).
3. Build Part 1 (token check, block rules, settings).
4. Build Part 2 (hide card, shared scoring module).
5. Test on the dev store with the cases below.
6. Go live in `log_only`; review logs daily with Timur.
7. Switch to `enforce` once logs are clean; keep the kill switch handy.
8. Tune thresholds and blocklists after the first week.

## Test cases

| # | Scenario | Expected |
|---|---|---|
| 1 | `POST /cart/add.js` on myshopify.com, then checkout, no page load | Blocked |
| 2 | `/cart/{variant}:1` permalink straight to checkout | Blocked |
| 3 | Real browser, first-time guest, one $0.99 item, `jane.doe@gmail.com`, real address | Goes through |
| 4 | Real browser, first-time guest, one $0.99 item, `james.smith4881@gmail.com` | Card hidden, message shown |
| 5 | Real browser, shipping name "James Anderson" | Blocked |
| 6 | Real browser, address line "428 st" | Blocked |
| 7 | Returning customer, one $0.99 item | Goes through |
| 8 | $59.99 glove order, first-time guest, real address | Goes through |
| 9 | Buy it now, Shop Pay and PayPal express buttons | Go through |
| 10 | Draft order invoice paid by the customer | Goes through |
| 11 | Token copied from cart A onto cart B | Blocked |
| 12 | Turnstile blocked or slow | Soft token, not blocked |
| 13 | Settings mode = `log_only` | Nothing blocked, decisions logged |
| 14 | Settings enabled = `false` | Everything goes through |

## Open questions (answer in Phase A)

1. Does the Authorize.net card method show up in the Payment Customization input, and can it be hidden?
2. What cart ID, or current time, can the checkout rule read, so the token can be tied to one cart?
3. Do Buy it now, Shop Pay and PayPal express checkouts keep cart attributes set by the theme? If not, what's the cleanest way to cover them?
4. Do Recurpay renewals or draft orders run through the checkout rule?
5. Where should block logs live so Timur can review a week of decisions easily?
6. Should the app proxy limit token requests per IP? (Our server can see IPs even though Functions can't.)

## Resources

- Cart and Checkout Validation: build guide + Function API
- Payment Customization Function API
- Shopify Functions overview and limits
- App proxies
- Cart Ajax API (`/cart.js`, `/cart/update.js`)
- Cloudflare Turnstile: server-side validation
- CSV of 1,374 bot orders with totals, names and dates — ask Timur (do not commit to git).
