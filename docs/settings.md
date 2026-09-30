# Checkout settings (Stages 1 + 2)

Where: Shopify admin → **Content → Metaobjects → Bot Gate settings** → entry with handle **`main`**.

| Field | Meaning |
|---|---|
| **Enabled** | Kill switch. Off = every checkout goes through. |
| **Rules (JSON)** | The rules below. If the JSON is broken, checkouts go through and the log shows `cfg_errors`. |

```json
{
  "mode": "log_only",
  "support_phone": "(800) 548-6877",
  "skip_logged_in": true,
  "blocked_names": ["james anderson"],
  "blocked_address1": ["428 st", "428 w 45th st|10036", "230 w 55th st|10019", "123 main st|10080"],
  "blocked_zips": ["10080"],
  "blocked_email_domains": [],
  "token_mode": "off",
  "soft_tokens": "allow"
}
```

- `mode`: `"enforce"` blocks. Anything else only logs (`would_block`).
- `skip_logged_in`: logged-in customers are never checked — blocklist (D11) and token (D3). Default `true`.
- `token_mode` (Stage 2): `"off"` (default — token not checked), `"log_only"` (logs `would_block` with rule `token_missing` / `token_invalid` / `token_expired`), `"enforce"` (blocks those). Separate from `mode`, so the blocklist and the token can be switched independently. A typo counts as `log_only` and shows in `cfg_errors`.
- `soft_tokens` (Stage 2): `"allow"` (default, spec test 12 — a buyer whose Turnstile check failed or was slow still gets through), `"risky"` (soft token fails only for a guest cart with 1 item, quantity 1, under $5 — rule `token_soft_risky`) or `"block"` (every soft token fails — rule `token_soft`).
- Logged-in customers never need a token (D3), whatever `skip_logged_in` says.
- If the token server is down (no key update for ~70 minutes, or keys missing), the token check lets checkouts through and logs `tok:"keys_stale"` / `"keys_missing"`.
- Names: case/punctuation ignored. Address lines: also "West"="W", "Street"="St" etc.; must match the whole street line (a trailing "Apt 4" / "#4" / "Suite 2" still matches; "NW" or "Ext" does not). Address and ZIP rules apply to US addresses only.
- ZIPs: 5 digits (ZIP+4 also matches).
- **Tie an address to a ZIP** with `|`: `"428 w 45th st|10036"` blocks that street line only when the delivery ZIP is 10036 (use this for real buildings). Without `|zip` the address is blocked in every ZIP (use for fake lines like `"428 st"`). A bad ZIP after `|` skips that entry and the log shows `cfg_errors:["blocked_address1"]`.

Log line (Dev Dashboard → Logs, Pay step only), no personal data:
`{"v":3,"decision":"would_block","rules":["blocked_address1","token_missing"],"mode":"log_only","tmode":"log_only","tok":"missing","n":[1,4,1,0],"cfg_errors":[],"phone_missing":false,"sub":"0.99","lines":1,"qty":1,"authed":false,"addr":1,"email":true}`
Block lines also have `"enf":[...]` = the rules that were actually enforced. With a token: `"tid":"995432.fe264695","tage":0` (`tid` also appears in the token server log; `tage` = token age in 30-min windows). `sub` = product subtotal.
`tok` = token result: `ok`, `soft`, `missing`, `malformed`, `expired`, `bad_sig` (token from another cart / cart changed), `keys_missing`, `keys_stale`, or `off`.
`n` = entries per list (names, addresses, zips, email domains) — a quick check the settings were read.
