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

## Live rollout (log-only) — 2026-10-06
- Worker `sc-prod` on the MedicalRite Cloudflare account (Workers Paid), KV `production-STATE`, 3 secrets set; `/health` reachable.
- Live entry `settings` created 10-05 (copy of `main`, enforce); `"token_mode":"log_only"` added 10-06.
- mr-checkout-tools-5 released, but built from `main` before the prod-config PR was merged → app URL pointed at `sc-prod.hexondigitaldev.workers.dev` (404). Merged PR, redeployed from `main` (mr-checkout-tools-6).
- App page then showed "Open this app from the Shopify admin" (`home_denied`, `verified_shop:null`): the live app had an unfinished secret rotation (Old + New). Set Worker to the New secret, revoked Old → app **connected, keys published 15:44 UTC**, status `embed_missing` (expected, embed off).
- Lesson: before a live deploy run `findstr workers.dev shopify.app.production.toml` on `main`.
- Blocked-address check on live after the switch to `settings`: **blocked** (support phone message). 
- Next: privacy-policy line (Turnstile) + Brandon heads-up → turn on embed with site key `0x4AAAAAAFPOhcduLTake4BT`; then a few days of log-only review.
- **Embed on (2026-10-06 ~18:00 UTC)** in live theme `medicalrite/main` (GitHub-connected → settings_data.json; Brandon told), site key `0x4AAAAAAFPOhcduLTake4BT`. Privacy policy got the approved Turnstile "Fraud Prevention" section first (Timur approved text).
- Live smoke test: `window.__sfh` true; Add to cart → cart attribute `_bg` = `1.995172.s.…`; Worker log `{"f":"s","why":"no_response","xff":1}`; `/health` → `{"ok":true}`.
- Watch item: first ticket on live was **soft** (Turnstile didn't answer within 4 s on a heavy live page). Same as dev first loads. In log-only, measure the soft share (Worker `why`) and the rule's `tok` mix on real orders; if soft is common, raise `TS_WAIT_MS` / load Turnstile earlier before enforcing.

## Live log-only review #1 — 2026-10-08 (orders Oct 5–8, 471 orders)
- Blocklist (Stage 1, enforce): 1 would-block in the export, MR54696 (Oct 5 07:19, before enforce; $1.99, High risk, refunded). 0 real customers matched. No bot-shaped orders in the Oct 5 13:14 EDT incident window.
- MR54895 (Oct 6 14:09, $1.99, High, pending) carries ticket tid 995172.db6aa3e8 = the live smoke-test cart → Hexon's own test order; to be voided/cancelled.
- Tickets on real web orders since the embed went on (Oct 6 14:00 EDT → Oct 8): **250 web orders: hard 152 (61%), soft 94 (38%), none 4 (2%)**; 16 subscription renewals (no checkout, rule n/a).
  - By day: Oct 6 40h/35s/2none, Oct 7 91h/49s/2none, Oct 8 21h/10s/0none.
  - **Soft share 38% is too high** to rely on: soft = Turnstile didn't answer in 4 s (`no_response`). Fix before enforce (see below).
  - 4 web orders without a ticket (MR54892 14:02 right after embed on — cart older than embed; MR54932, MR55020, MR55057): low risk, paid, real. Cause unknown (logged in? checkout link?). Need the rule log (`authed`) to tell.
- **Live run logs are hidden**: Dev Dashboard says "Full log details are hidden because your app is missing … read_products" (Stage 2 query reads the variant id). Add `read_products` to production scopes to see live decisions.
- Planned fixes: (1) `read_products` scope (prod + dev); (2) sf.js: load Turnstile on page load, and when the 4 s wait times out keep listening — if Turnstile answers later, fetch a hard ticket in the background (no visible delay). Then re-measure soft share.

## Round 2 dev tests — 2026-10-08 (medicalrite-bot-gate-7)
- 23:23:42 draft invoice (customer attached, box not ticked): `authed:true` → `exempt_logged_in`, `tok:"missing"`. (Browser may have had a customer login; re-check in a fresh incognito.)
- 23:24:55 **guest, cart link, existing customer's email: `cust:true`, `authed:false` → `exempt_customer`, `tok:"missing"` → exemption unsafe, removed.**
- 23:26:31 normal Add to cart → `tok:"ok"`, `cust:true` (same typed email).
- 23:27:55 warm-up (10 s on homepage, then Add to cart) → `tok:"ok"`.
- Live log visibility on dev unchanged; `read_products` still to be approved on live after the prod deploy.
- Retest after removing the exemption (medicalrite-bot-gate-8):
  - 23:35:17 guest, cart link, existing customer's email → `would_block`, `token_missing`, `authed:false`, `cust:true` ✔ (would be blocked in enforce).
  - 23:36:54 draft invoice ($699.95) opened in a fresh incognito → `isAuthenticated:true` → `exempt_logged_in` ✔. Invoice links sign the buyer in as the attached customer, so invoices **with a customer** never need the ticket; "Ignore all checkout rules" is only needed for invoices **without** a customer.
