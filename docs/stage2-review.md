# Stage 2 review record

Builder: Claude (Cowork). Critic: separate agent using `.claude/agents/critic.md`.

| Round | Result | Main points |
|---|---|---|
| 1 | Changes requested (2 blockers, 9 major) | Soft tokens bypassable with one request; server outage failed closed for 1–3 days; checkout hold could release with an old-cart token; token could be near expiry at checkout; endless retry on /cart.js errors; soft buyers rate-limited into blocks; X-Forwarded-For trust; express buttons race; storefront readability of keys unverified; no server/theme tests; theme embed off = everyone blocked. |
| 2 | Changes requested (4 major) | Input-variable default must be proven before live (T0); store-wide soft budget refused real buyers; classic cart form `updates[]` skipped the refresh; D17 wording. |
| 3 | Changes requested (2 minor) | Theme submit handlers bypassed on the `updates[]` path; siteverify 4xx exempt from soft limits. |
| 4 | Changes requested (1 minor) | Resumed submit ran theme handlers twice. |
| 5 | **Nothing left to criticise** | Remaining: dev-store tests T0–T13, go-live gates, Timur D17–D19. |

Tests: Rust 62 (rules, token, end-to-end through Shopify's input shape, shared token vector); server + theme 30 (node, jsdom). Query cost 28/30.

Also found: the D12 ZIP-scoped address change was never saved to the repo (see `docs/stage1-review.md`). Restored here; live needs a Stage-1-only hotfix.
