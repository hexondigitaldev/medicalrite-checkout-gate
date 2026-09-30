# Stage 1 review record

Builder: Claude (Cowork). Critic: separate agent using `.claude/agents/critic.md`.

| Round | Result | Main points |
|---|---|---|
| 1 | Changes requested (2 blockers, 6 major) | Input query over Shopify's cost limit (≈37/30) → settings reduced to 2 fields; prefix address match too broad; ZIP not US-only; street abbreviations applied to names; broken settings silent; no order-matching hints in log. (Blocker "doesn't compile" was a stale copy.) |
| 2 | Changes requested (1 major) | Logged-in exemption hid would-be hits; config type/typo mistakes silent; no signal when input data is empty. Glue tests found a real bug: country code compared with quotes, so address/ZIP rules could never fire. |
| 3 | **Nothing left to criticise** | Optional: log B2B would-be hits (done). |

Tests: 31 (rules + end-to-end through Shopify input shape). Query cost ≈ 19–21 / 30.

## Before switching live to `enforce` (gates)
- [x] Timur: logged-in buyers skip the blocklist — yes (D11)
- [x] Timur: scope real-building addresses to their ZIP — yes (D12, implemented, critic round 4 OK)
- [x] Timur: support phone = number on site, (800) 548-6877 (D13)
- [x] Dev store: settings upsert → log shows `n:[1,4,1,0]`, `cfg_errors:[]`, `addr` 1, `email` true (2026-09-29)
- [x] Dev store: spec case 5/6 in `enforce` actually blocks (proves name/address fields arrive) (2026-09-29)
- [x] Wasm size measured: 112,588 bytes (limit 256 kB), 2026-09-28. 31/31 tests pass on Windows.
- [x] Production app "MR Checkout Tools" (version mr-checkout-tools-2) installed on live, `read_customers` approved; protected customer data section not offered for this custom app, run details visible anyway (2026-09-29)
- [ ] A few days of live `log_only`, then would_block review with Timur
- [ ] Hexon code review

## Dev store test run — 2026-09-29 (mode `enforce`)

| # | Checkout | Result | Log decision |
|---|---|---|---|
| 1 | Name "James Anderson", normal address (ME) | Blocked, correct message + phone | `block` · `blocked_name` |
| 2 | Address "428 st", NY 10001 | Blocked | `block` · `blocked_address1` |
| 3 | "230 West 55th Street", NY 10019 | Blocked | `block` · `blocked_address1` |
| 4 | Normal name + address | Order placed | `allow` · `clean` |
| 5a | "428 st", email autofilled but **not signed in** | Blocked (correct: `authed:false`) | `block` · `blocked_address1` |
| 5b | "428 st", **signed in** | Order placed | `allow` · `exempt_logged_in` · rules `blocked_address1` |

Instruction count: ~6.6k on non-Pay steps, ~207k on the Pay step (limit 11M).
Note: an autofilled email is not a login — only a real sign-in sets `isAuthenticated`.

| 6 | Kill switch: Enabled **off**, "428 st" | Order placed | `allow` · `disabled` |
| 7 | Mode `log_only`, "428 st" | Order placed | `would_block` · `blocked_address1` |

**Dev store: all 7 checks pass (spec cases 5, 6, 8, 13, 14 + logged-in exemption + long-form address).** Settings changes in Content → Metaobjects took effect without redeploy.

## Live store — log_only started 2026-09-29

- App: MR Checkout Tools (`shopify.app.production.toml`), version `mr-checkout-tools-2`, custom distribution to medicalritestore.
- Settings entry `main` created in Content → Metaobjects → Checkout settings, mode `log_only`, same seed as dev (ZIP-scoped addresses, phone (800) 548-6877).
- Checkout rule active, "Block checkout if app experiences a problem" unticked. No theme embed.
- Old dev app not on live.
- First check (560 most recent runs read through Dev Dashboard, read-only): 7 Pay-step runs, all `allow · clean`, `mode:log_only`, `n:[1,4,1,0]`, `cfg_errors:[]`. Log lines contain no PII.
- Seen: one order produced 2 Pay-step runs 1 s apart (Shopify retry, normal); 2 Pay-step runs with `addr:0, email:false` (probably pickup or express) — watch in the review.
- Run details (input) are visible in Dev Dashboard for the production app.
