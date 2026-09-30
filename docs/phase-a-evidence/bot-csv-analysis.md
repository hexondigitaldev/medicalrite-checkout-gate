# Bot-order CSV scored against the spec (495 orders, Aug 1 – Sep 15, 2026)

Source: Timur's `MedicalRite_CardTesting_Orders_2026-08-01_to_09-15` (kept locally in `data/`, git-ignored).
Script: `scripts/score_bot_orders.py` (prints aggregates only).
Assumption: orders whose landing page is a permalink or `/cart/add.js` would have **no token**; homepage/blank landings are treated as if they **might** have one (worst case).

## Headline

| Layer | Bots caught |
|---|---|
| No token (permalink / add.js entry) | 399 / 495 (80.6%) |
| Blocklist — address | 460 / 495 (92.9%) |
| Blocklist — name ("james anderson") | 328 / 495 (66.3%) |
| Malformed address (< 6 chars or no number) | **0 / 495** |
| Risk score ≥ 7 (hide card) | 451 / 495 (91.1%) |
| **All layers combined** | **494 / 495 (99.8%)** |
| Caught *only* by the token (no blocklist, score < 7) | 9 |

Score distribution: 7 → 451, 5 → 43, 3 → 1.

## Findings

1. **"Every bot scores 7" is not quite true** — 44 orders score 5 or less: the Aug 4 wave bought a $10.99 item (misses "under $5"), and many emails have no trailing digits (`first.last@outlook.com`). Card-hiding is a strong backstop (91%), not a complete one.
2. **Malformed-address rule catches nothing as written.** The drop "428 st" is **exactly 6 characters**, so "under 6" misses it. Suggest "6 or fewer" — needs checking against real orders first — or rely on the blocklist, which already covers it.
3. **The Sep 5–6 wave (W15) entered through the homepage** (29 of 36 orders), i.e. loaded a real page and could have earned a token. 35/36 were still caught by the blocklist. **The blocklist is essential**, not a nice-to-have.
4. **One W15 order would slip through everything** — $7.99 item, zoho.com email, score 3, not on the blocklist, homepage entry. Worth Timur checking whether it was really a bot.
5. **Free-mail list must be broad**: hotmail, yahoo, outlook, icloud, gmail, aol, proton.me, protonmail.com, yandex, mail.com, zoho all appear.
6. **Headers are spoofed** (realistic Chrome/Safari/mobile user agents, `medicalrite.com` referrer on permalink hits) → header checks wouldn't help; supports the token design.
7. Today the blocklist does most of the work, but the operator already rotated identities once (Sep 9). The token is what stays effective when names/addresses change.
