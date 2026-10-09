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

**2026-10-01**
- Hidden leftover entries kept the handle `main` (even after uninstall/reinstall), so the rule now reads handle `settings` (decisions log).
- **T0 passed.** Blocklist enforce + `token_mode: log_only`, no Worker connection (`keys: null`, `pastFresh: true`):
  - `428 st` → **blocked**, log `decision:block, enf:["blocked_address1"], tok:"keys_missing"`.
  - Normal address → **order placed**, log `decision:allow, reason:clean, tok:"keys_missing"` (two Pay-step runs, Shopify retry).
- **T1:** Pay-step instruction count 182k–218k (limit 11M); wasm 141,871 bytes.
- **T2 passed:** app opened in admin → connected, keys published, status OK (after fixing metafield keys `k`/`v` → `keys`/`vars`, Shopify minimum 2 chars).
- **T3 (first full run, 13:32 UTC):** with the theme embed on, the cart carried a valid token for its contents → rule log `tok:"soft"`, `tid:"994923.3b0711bb"`, `tage:0`, `decision:allow`. Whole chain works (theme → app proxy → Worker → cart attribute → rule signature check). It was a *soft* token though — Turnstile did not pass; investigating (`why` in Worker log).
- Without the embed (13:26 UTC): `tok:"missing"` → `would_block` (log only), as designed.
- Turnstile was soft because the Worker's `TURNSTILE_SECRET` was wrong (error-codes now logged); re-entered → hard tokens. First check on a fresh page can still time out (`no_response`, 4 s) and get a soft token; the next refresh is hard. Watch the share in log-only.
- **T3 passed (13:46 UTC):** normal checkout via product page → Add to cart → checkout: `tok:"ok"`, `tid:"994923.f145e311"`, `tage:0`, `decision:allow`.

**2026-10-05** (batch run, times PKT; settings `mode: enforce`, `token_mode: log_only`)
- **T4 passed** (18:22): qty changed on cart page then checkout → `tok:"ok"`, qty 4, `decision:allow`.
- **T5 passed** (18:23): cart permalink → `tok:"missing"` → `would_block` (`token_missing`), log only.
- **T6 passed** (18:26, 18:27, 18:41): ticket copied into a cart with different contents → `tok:"bad_sig"` → `would_block` (`token_invalid`).
- **T7 passed** (18:28): Turnstile domain blocked → soft ticket → `tok:"soft"`, `decision:allow`.
- **T12a passed** (18:33): logged-in customer via permalink, no ticket → `allow`, `reason:exempt_logged_in`.
- **T12b finding** (18:39): draft order invoice paid by the customer, sent *without* "Ignore all checkout rules" → `tok:"missing"` → `would_block`. Invoice checkouts run the rule (Shopify: validation runs in draft-order checkout). Once `token_mode` is `enforce`, staff must tick **"Ignore all checkout rules"** when sending an invoice (or use `bypassCartValidations` via API). Add to the go-live note for Timur/staff.
- **T13 passed** (18:42): Enabled = False → `allow`, `reason:disabled`, checkout went through.
- **T8 passed:** every app proxy request had exactly one `X-Forwarded-For` entry (`xff:1`) → `IP_HEADER_POS = "first"` is right; keep it for production.
- Soft-ticket note: besides D (domain blocked on purpose), one normal page load at 18:24 got a soft ticket (`why:"no_response"`, Turnstile did not answer within 4 s). Same as seen on 10-01. Watch the soft share during live log-only before enforcing.
- **Dev testing done** (T10 stale-keys and T11 PayPal skipped as optional; T9 storefront metafield check optional).

- **T9 FAILED, fixed (2026-10-05):** a Custom Liquid block `{{ shop.metafields['app--427260968961--sc'].keys }}` printed the window keys on the storefront. App-reserved (`$app`) **shop** metafields are readable from theme Liquid, so any theme code (theme devs, other apps' blocks) could read them and mint valid tickets for ~90 min. Fix: keys now live on the checkout rule's own metafield (`validation { keys: metafield(...) }`), which Liquid and the Storefront API can't reach; the Worker writes keys + vars to the validation and deletes the old shop copy on every publish. Query cost unchanged (28). Window keys are one-way derived, so the master key is not exposed and the dev MASTER_KEY needs no rotation. **Re-run T9 after deploying** (expect `KEYS:[] []`) plus T3 (`tok:"ok"`).
- **T9 re-run passed (2026-10-05):** after the fix (dev version medicalrite-bot-gate-6, Worker redeployed, app opened), the same Liquid block prints `KEYS:[] []`.
- **T3 re-run passed (22:42 PKT):** with keys on the checkout rule: input has `validation.keys`, log `tok:"ok"`, `tid:"995123.bc90499e"`, `decision:allow`. Rust tests 62 pass.
  - Gotcha: first try (22:31) still ran the Oct 1 `app dev` preview (old query, `shop.keys` → `keys_missing`) although medicalrite-bot-gate-6 was released. Fixed by running `shopify app dev --config shopify.app.toml` once (preview refreshed to current code) and quitting. A dev preview always wins over released versions on the dev store.

## Live round 2 — 2026-10-09
- mr-checkout-tools-7 live 15:10 PKT (Turnstile warm-up + late hard ticket, `cust` log field, `read_products` in config).
- Blocked-address check on live after the deploy: **blocked** ✔.
- `read_products` not granted on live: no update prompt, and the custom-distribution install link returns `invalid_link` (already installed). Checkout unaffected; live run logs stay hidden. Reviews use the orders export (`_bg` note attribute) + Worker logs. Open item: add a scope-request button (App Bridge) to the app page later.
- Next review ~Oct 11: orders from Oct 9 15:10 PKT, compare soft share with 38% (review #1).
