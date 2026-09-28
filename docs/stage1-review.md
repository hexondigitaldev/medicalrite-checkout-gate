# Stage 1 review record

Builder: Claude (Cowork). Critic: separate agent using `.claude/agents/critic.md`.

| Round | Result | Main points |
|---|---|---|
| 1 | Changes requested (2 blockers, 6 major) | Input query over Shopify's cost limit (≈37/30) → settings reduced to 2 fields; prefix address match too broad; ZIP not US-only; street abbreviations applied to names; broken settings silent; no order-matching hints in log. (Blocker "doesn't compile" was a stale copy.) |
| 2 | Changes requested (1 major) | Logged-in exemption hid would-be hits; config type/typo mistakes silent; no signal when input data is empty. Glue tests found a real bug: country code compared with quotes, so address/ZIP rules could never fire. |
| 3 | **Nothing left to criticise** | Optional: log B2B would-be hits (done). |

Tests: 31 (rules + end-to-end through Shopify input shape). Query cost ≈ 19–21 / 30.

## Before switching live to `enforce` (gates)
- [ ] Timur: logged-in buyers skip the blocklist? (`skip_logged_in`, default true)
- [ ] Timur: scope blocked addresses to a ZIP (e.g. real residents of 428 W 45th St / 230 W 55th St)?
- [ ] Timur: confirm support phone number
- [ ] Dev store: settings upsert → log shows `n:[1,4,1,0]`, `cfg_errors:[]`, `addr` > 0
- [ ] Dev store: spec case 5/6 in `enforce` actually blocks (proves name/address fields arrive)
- [ ] Wasm size measured after build (limit 256 kB)
- [ ] Production app + config + scopes (`read_customers`, protected customer data) per release process
- [ ] Hexon code review
