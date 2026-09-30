# Bot Gate settings (Stage 1)

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
  "blocked_email_domains": []
}
```

- `mode`: `"enforce"` blocks. Anything else only logs (`would_block`).
- `skip_logged_in`: logged-in customers are never checked (default `true`, pending Timur).
- Names: case/punctuation ignored. Address lines: also "West"="W", "Street"="St" etc.; must match the whole street line (a trailing "Apt 4" / "#4" / "Suite 2" still matches; "NW" or "Ext" does not). Address and ZIP rules apply to US addresses only.
- ZIPs: 5 digits (ZIP+4 also matches).
- **Tie an address to a ZIP** with `|`: `"428 w 45th st|10036"` blocks that street line only when the delivery ZIP is 10036 (use this for real buildings). Without `|zip` the address is blocked in every ZIP (use for fake lines like `"428 st"`). A bad ZIP after `|` skips that entry and the log shows `cfg_errors:["blocked_address1"]`.

Log line (Dev Dashboard → Logs, Pay step only), no personal data:
`{"v":2,"decision":"would_block","rules":["blocked_address1"],"mode":"log_only","n":[1,4,1,0],"cfg_errors":[],"phone_missing":false,"total":"11.91","lines":1,"qty":1,"authed":false}`
`n` = entries per list (names, addresses, zips, email domains) — a quick check the settings were read.
