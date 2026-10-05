"""Replay the live checkout-rule blocklist over a Shopify orders export.

Why: the Dev Dashboard can't filter function runs, and live has ~20k runs/day, so reading
every Pay-step log line isn't practical. In log_only mode every checkout still becomes an
order, so applying the same rules to the orders export shows what `enforce` would block.

Usage:
  1. Shopify admin -> Orders -> filter "Date: since Sep 29" -> Export -> "All orders matching
     your search", "Plain CSV file". Save it as data/live-orders.csv (git-ignored, has PII).
  2. python3 scripts/review_live_orders.py data/live-orders.csv '<settings JSON from the live entry>'
Prints counts plus order numbers only (no names, emails or addresses).
Logic mirrors extensions/checkout-rule/src/rules.rs (keep in sync).
"""
import csv, json, re, sys
from collections import Counter

STREET = {"west":"w","east":"e","north":"n","south":"s","street":"st","avenue":"ave","av":"ave","road":"rd",
          "boulevard":"blvd","drive":"dr","lane":"ln","place":"pl","court":"ct","suite":"ste","apartment":"apt",
          "parkway":"pkwy","highway":"hwy","square":"sq","terrace":"ter"}
UNIT = {"apt","ste","unit","fl","floor","rm","room"}

def basic(s): return re.sub(r"[^0-9a-z]+", " ", (s or "").lower()).split()
def norm_name(s): return " ".join(basic(s))
def norm_addr(s): return " ".join(STREET.get(w, w) for w in basic(s))

def zip5(z):
    z = (z or "").strip().lstrip("'")
    d = re.sub(r"\D", "", z)
    ok = (len(z) == 5 and len(d) == 5) or (len(z) == 9 and len(d) == 9) or (len(z) == 10 and len(d) == 9 and z[5] == "-")
    return d[:5] if ok else None

def addr_match(a1, blocked):
    a = norm_addr(a1)
    if a == blocked: return True
    if a.startswith(blocked + " "):
        first = a[len(blocked) + 1:].split(" ")[0]
        return first in UNIT or first.isdigit()
    return False

def main(path, cfg_json):
    cfg = json.loads(cfg_json)
    names = {norm_name(x) for x in cfg.get("blocked_names", [])}
    addrs = []
    for e in cfg.get("blocked_address1", []):
        a, _, z = e.partition("|")
        addrs.append((norm_addr(a), zip5(z) if z else None))
    zips = {zip5(z) for z in cfg.get("blocked_zips", []) if zip5(z)}
    doms = {d.strip().lstrip("@").lower() for d in cfg.get("blocked_email_domains", [])}

    orders = {}
    for r in csv.DictReader(open(path, encoding="utf-8-sig")):
        o = orders.setdefault(r["Name"], {"qty": 0, "lines": 0, **r})
        o["lines"] += 1
        try: o["qty"] += int(r.get("Lineitem quantity") or 0)
        except ValueError: pass

    hits, rules, pays, shapes = [], Counter(), Counter(), Counter()
    for name, o in orders.items():
        rs = []
        if norm_name(o.get("Shipping Name")) in names: rs.append("blocked_name")
        if (o.get("Shipping Country") or "").strip().upper() in ("US", "UNITED STATES"):
            z = zip5(o.get("Shipping Zip"))
            if any(addr_match(o.get("Shipping Address1"), a) and (bz is None or z == bz) for a, bz in addrs): rs.append("blocked_address1")
            if z and z in zips: rs.append("blocked_zip")
        dom = (o.get("Email") or "").rpartition("@")[2].strip().lower()
        if dom and dom in doms: rs.append("blocked_email_domain")
        if rs:
            hits.append((name, rs, o.get("Subtotal"), o["lines"], o["qty"], o.get("Financial Status"), o.get("Payment Method"), o.get("Source"), o.get("Risk Level"), "cancelled" if o.get("Cancelled at") else "", (o.get("Created at") or "")[:16]))
            for x in rs: rules[x] += 1
            pays[o.get("Payment Method") or "?"] += 1
            shapes["1 line, qty 1" if o["lines"] == 1 and o["qty"] == 1 else "bigger cart"] += 1

    print(f"Orders in export: {len(orders)}")
    print("Sources:", dict(Counter(o.get("Source") or "?" for o in orders.values())))
    print(f"Would be blocked in enforce: {len(hits)}  (logged-in customers are exempt in the rule; the export can't tell, see 'Customer' column if needed)")
    print("By rule:", dict(rules))
    print("Cart shape of would-blocks:", dict(shapes))
    print("Payment method of would-blocks:", dict(pays))
    print("\nOrder | rules | subtotal | lines | qty | financial status | payment | source | risk | cancelled | created")
    for h in sorted(hits, key=lambda h: h[-1]):
        print(" | ".join(str(x) if not isinstance(x, list) else ",".join(x) for x in h))

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
