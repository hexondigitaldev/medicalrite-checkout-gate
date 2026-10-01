# Stage 2 review record

Builder: Claude (Cowork). Critic: separate agent using `.claude/agents/critic.md`.

| Round | Result | Main points |
|---|---|---|
| 1 | Changes requested (2 blockers, 9 major) | Soft tokens bypassable with one request; server outage failed closed for 1–3 days; checkout hold could release with an old-cart token; token could be near expiry at checkout; endless retry on /cart.js errors; soft buyers rate-limited into blocks; X-Forwarded-For trust; express buttons race; storefront readability of keys unverified; no server/theme tests; theme embed off = everyone blocked. |
| 2 | Changes requested (4 major) | Input-variable default must be proven before live (T0); store-wide soft budget refused real buyers; classic cart form `updates[]` skipped the refresh; D17 wording. |
| 3 | Changes requested (2 minor) | Theme submit handlers bypassed on the `updates[]` path; siteverify 4xx exempt from soft limits. |
| 4 | Changes requested (1 minor) | Resumed submit ran theme handlers twice. |
| 5 | **Nothing left to criticise** | Remaining: dev-store tests T0–T13, go-live gates, Timur D17–D19. |

Wasm size (Hexon's PC, 2026-09-30): 141,871 bytes (limit 256 kB; Stage 1 was 112,588). Tests: Rust 62 (rules, token, end-to-end through Shopify's input shape, shared token vector); server + theme 30 (node, jsdom). Query cost 28/30.

Also found: the D12 ZIP-scoped address change was never saved to the repo (see `docs/stage1-review.md`). Restored here; live needs a Stage-1-only hotfix.

## Dev store testing log

**2026-09-30**
- Worker `sc-dev` deployed (https://sc-dev.hexondigitaldev.workers.dev), KV + 3 secrets set; `/health` = `keys_out_of_date` (app not connected yet — expected).
- Dev app version medicalrite-bot-gate-5 released; `app dev` preview now runs the current code (log line `v:3`).
- **T0 core check passed:** Pay-step input showed `"keys": null` and `"pastFresh": true` → the query default works, the rule ran normally and still evaluated the blocklist (`rules:["blocked_address1"]`).
- Not yet shown: an actual *block* in enforce, because the rule reads a leftover hidden `main` settings entry with the old log-only JSON.
- Lessons:
  - An old `shopify app dev` preview keeps running old code on the dev store even after new versions are released. `shopify app dev clean` removes it, **and also the app-owned metaobject definition**.
  - A released version only re-applies config parts that *changed*, so after a clean the settings definition came back empty; changing each field's description forced the fields back.
  - The old `main` entry survived as a hidden entry (not listed, not found by handle, but the handle is taken and the function still reads it).
- **Next:** find/delete the hidden entry via `metaobjectDefinitionByType { metaobjects }`, or uninstall + reinstall the dev app on the dev store; create `main`; re-add the checkout rule; finish T0 (block with `428 st`, normal order passes, `tok:"keys_stale"`); then open the app (T2) and continue T3–T13.
