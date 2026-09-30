# Stage 0 — live store probe record (medicalritestore.myshopify.com)

Approved by Timur 2026-09-24 (D10). Probes record only; they never block or hide.

| Item | Status | When |
|---|---|---|
| App installed (custom distribution) | Done | 2026-09-25 |
| Checkout rule "Bot Gate probe" | Active, "Block checkout if app experiences a problem" OFF (verified by screenshot) | 2026-09-25 |
| Theme app embed "Bot Gate probe" | On — live theme **medicalrite/main** (GitHub-connected) | 2026-09-25 |
| Payment customization "Bot Gate probe" | **Deferred** — `shopify app execute` only allows mutations on dev stores; needs our app's own admin page (Stage 2 server) | |
| Timur told "in" | _pending_ | |
| Removed | **App uninstalled from the live store** (probes gone) | 2026-09-29 |

Notes:
- Existing checkout rule on live: "Discount Genie GWP Checkout Validation Rule" (Inactive). Keep in mind if it is ever activated alongside ours.
- Theme embed writes to the live theme's settings_data.json — Qckbot (GitHub-synced theme) to be told; re-check after theme deploys.

- 2026-09-25: live run details were hidden (missing read_customers, read_products). Probe input trimmed to step + attributes + line qty + subtotal; redeployed.
