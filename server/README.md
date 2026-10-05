# Token server (Cloudflare Worker)

Issues the short-lived cart token for the checkout rule and publishes the signing keys.
Design: `docs/stage2-plan.md`, `docs/decisions.md` (Stage 2 engineering decisions).

| Path | What |
|---|---|
| `POST /proxy/t` | App proxy target (`/apps/sc/t` on the store). Turnstile check → signed token. |
| `GET /` | App home in Shopify admin. Opening it once connects the app (token exchange) and publishes keys. |
| `GET /health` | `200` when the checkout rule has the current key, `503` otherwise. Point an uptime monitor at it. |
| cron (10 min) | Publishes window keys + freshness to metafields `$app:sc.keys` / `$app:sc.vars` on this app's checkout rule (validation). Not on the shop: readable from Liquid (T9). |

## One-time setup (dev)

Run in `server/`:

```bash
npm install
npx wrangler login                       # opens Cloudflare in the browser
npx wrangler kv namespace create STATE   # copy the id into wrangler.toml
npx wrangler secret put SHOPIFY_CLIENT_SECRET   # Dev Dashboard -> Medicalrite Bot Gate -> Settings -> Client secret
npx wrangler secret put TURNSTILE_SECRET        # Cloudflare -> Turnstile -> store-check -> Settings
npx wrangler secret put MASTER_KEY              # paste the output of the command below
node -e "console.log(require('crypto').randomBytes(32).toString('hex'))"
npm run deploy                           # prints https://sc-dev.<you>.workers.dev
```

Then put that host into `shopify.app.toml` (replace every `WORKER_HOST`) and run `npm run deploy` in the repo root (Shopify app).

Secrets are typed into the terminal only — never into chat, git, or `wrangler.toml`.

## Tests

`npm test` (Node 22+). The shared token vector is also checked in `extensions/checkout-rule/src/token.rs`.

## Monitoring

- Uptime monitor (e.g. any free service) on `https://<worker>/health`, alert on non-200. Reasons: `keys_out_of_date`, `no_checkout_rule`, `embed_missing`.
- Watch the `token` events' `why` field: a jump in soft tokens (`failed`, `hostname`, `siteverify_*`) means Turnstile or its settings broke, or bots are asking for soft tokens.
- Rotate `MASTER_KEY` only while `token_mode` is off or `log_only`.

## Logs

`npm run logs` (live tail). Events: `token` (flag h/s and reason), `keys_published`, `keys_publish_failed`, `rate_limited`, `error`. No IPs, cart contents or tokens are logged.
