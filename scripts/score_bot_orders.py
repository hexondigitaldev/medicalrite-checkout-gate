"""Score the bot-order CSV against the rules in docs/spec.md.

Usage: python3 scripts/score_bot_orders.py data/bot-orders.csv
Prints aggregate results only (no emails, names, IPs or card data).
"""
import csv, re, sys
from collections import Counter, defaultdict

FREEMAIL = {
    "gmail.com", "yahoo.com", "hotmail.com", "outlook.com", "aol.com", "icloud.com",
    "proton.me", "protonmail.com", "yandex.com", "mail.com", "gmx.com", "live.com", "msn.com",
}
BLOCKED_NAMES = {"james anderson"}
BLOCKED_ADDR1 = {"428 st", "428 w 45th st", "230 west 55th street"}
HIDE_CARD_SCORE = 7

def norm(s):
    return re.sub(r"\s+", " ", (s or "").strip().lower())

def entry(landing):
    l = (landing or "").strip()
    if re.match(r"^/cart/\d+:\d+", l): return "permalink"
    if l.startswith("/cart/add"): return "add.js"
    if l == "": return "(blank)"
    return "store page: " + l.split("?")[0]

def score(r):
    pts, why = 0, []
    pts += 2; why.append("first")                      # every bot is a first-time buyer
    if float(r["product_subtotal"]) < 5: pts += 2; why.append("sub<5")
    if int(r["qty"]) == 1: pts += 1; why.append("1line")
    local, _, dom = r["email"].lower().partition("@")
    if dom in FREEMAIL and re.search(r"\d{2,}$", local): pts += 2; why.append("freemail+digits")
    return pts, why

def main(path):
    rows = list(csv.DictReader(open(path, encoding="utf-8-sig")))
    n = len(rows)
    c = Counter(); by_entry = Counter(); entry_caught = defaultdict(Counter)
    scores = Counter(); domains = Counter(); wave_entry = defaultdict(Counter)
    for r in rows:
        e = entry(r["landing_page"]); by_entry[e] += 1; wave_entry[r["wave"]][e] += 1
        addr1 = norm(r["ship_address"].split(",")[0])
        name_bl = norm(r["ship_name"]) in BLOCKED_NAMES
        addr_bl = addr1 in BLOCKED_ADDR1
        malformed = len(addr1) < 6 or not re.search(r"\d", addr1)
        s, _ = score(r); scores[s] += 1
        domains[r["email"].lower().split("@")[-1]] += 1
        rule_block = name_bl or addr_bl or malformed
        hide = s >= HIDE_CARD_SCORE
        no_token = e in ("permalink", "add.js")
        c["blocklist_name"] += name_bl; c["blocklist_addr"] += addr_bl; c["malformed_addr"] += malformed
        c["any_rule_block"] += rule_block; c["score_ge_7"] += hide
        c["sub_lt_2_50"] += float(r["product_subtotal"]) < 2.5
        c["no_token_entry"] += no_token
        c["caught_by_anything"] += (no_token or rule_block or hide)
        c["token_possible_and_uncaught"] += (not no_token and not rule_block and not hide)
        entry_caught[e]["rules_or_score"] += (rule_block or hide)
    pct = lambda k: f"{c[k]} / {n} ({100*c[k]/n:.1f}%)"
    print(f"Orders: {n}\n")
    print("Entry route (landing_page):")
    for e, k in by_entry.most_common(): print(f"  {e:35s} {k:4d}   caught by blocklist/score: {entry_caught[e]['rules_or_score']}")
    print("\nRule hits:")
    for k in ["no_token_entry","blocklist_name","blocklist_addr","malformed_addr","any_rule_block","score_ge_7","sub_lt_2_50","caught_by_anything","token_possible_and_uncaught"]:
        print(f"  {k:30s} {pct(k)}")
    print("\nScore distribution:", dict(sorted(scores.items())))
    print("\nEmail domains:", domains.most_common(15))
    print("\nWaves by entry route:")
    for w in sorted(wave_entry): print(f"  {w}: {dict(wave_entry[w])}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "data/bot-orders.csv")
