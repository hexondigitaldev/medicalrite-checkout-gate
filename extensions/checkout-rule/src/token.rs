//! Stage 2 browser token check. Pure logic, unit-testable.
//!
//! Token (cart attribute `_bg`), made by the token server (server/src/lib.js):
//!   "1.<window>.<h|s>.<32 hex>"
//!   window = floor(unix_seconds / 1800)       (30-minute key windows)
//!   h = hard (Turnstile passed), s = soft (Turnstile failed or was slow; spec test 12)
//!   sig = first 16 bytes of HMAC-SHA256(key[window], "1|<window>|<h|s>|<content>")
//!   content = cart lines as "variantId:qty", same variant summed, sorted by id, joined by ","
//!
//! Keys: the server derives one key per window from a master secret that never leaves the
//! server, and publishes only the keys for (current-1, current, current+1) into an app-owned
//! shop metafield `$app:sc.k` (no merchant or storefront access):
//!   {"v":1,"d":"YYYY-MM-DD","c":<current window>,"k":{"<window>":"<64 hex>", ...}}
//! A token is valid while its window's key is still published => 30-60 minutes (D1).
//! Functions have no clock, so expiry comes from key rotation (Phase A, Q2).
//!
//! Freshness (fail open, D5): on every publish the server also writes the function's input
//! variable `freshUntil` (shop-local time, ~60-90 min ahead). If the server stops publishing,
//! `shop.localTime.dateTimeAfter(freshUntil)` turns true within the hour and the token check
//! lets everyone through (`keys_stale`). Backstop: publish date 2+ days old.

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// Valid hard token.
    Ok,
    /// Valid soft token (Turnstile failed or was slow).
    Soft,
    Missing,
    /// Not in our format.
    Malformed,
    /// Format fine, but its window's key is no longer (or not yet) published.
    Expired,
    /// Signature does not match this cart's contents (copied from another cart, cart changed, or forged).
    BadSig,
    /// Key metafield missing or unreadable: fail open (D5).
    KeysMissing,
    /// Keys not refreshed for 2+ days (token server down): fail open (D5).
    KeysStale,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Soft => "soft",
            Status::Missing => "missing",
            Status::Malformed => "malformed",
            Status::Expired => "expired",
            Status::BadSig => "bad_sig",
            Status::KeysMissing => "keys_missing",
            Status::KeysStale => "keys_stale",
        }
    }
}

/// Numeric id from "gid://shopify/ProductVariant/123" (or "123").
pub fn variant_number(gid: &str) -> Option<u64> {
    gid.rsplit('/').next()?.parse().ok()
}

/// Canonical cart content string. Must match `cartContent()` in the theme script and
/// `contentValid()` in the server.
pub fn content(lines: &[(u64, i64)]) -> String {
    let mut agg: Vec<(u64, i64)> = Vec::new();
    for &(id, qty) in lines {
        match agg.iter_mut().find(|(i, _)| *i == id) {
            Some(e) => e.1 += qty,
            None => agg.push((id, qty)),
        }
    }
    agg.retain(|&(_, q)| q > 0);
    agg.sort_by_key(|&(i, _)| i);
    agg.iter().map(|(i, q)| format!("{}:{}", i, q)).collect::<Vec<_>>().join(",")
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    b.chunks(2).map(|p| Some(hex_val(p[0])? << 4 | hex_val(p[1])?)).collect()
}

/// Days since 1970-01-01 for "YYYY-MM-DD" (proleptic Gregorian).
fn days(date: &str) -> Option<i64> {
    let mut it = date.get(..10)?.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Published keys, parsed from the metafield.
pub struct Keys {
    date: String,
    current: Option<u64>,
    keys: Vec<(u64, Vec<u8>)>,
}

/// Result of a token check plus log correlators (no secrets: `tid` is the window and the
/// first 8 of 32 signature hex chars, enough to match server logs, useless for forging).
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub status: Status,
    /// Windows since the token was issued (0 = this window), when known.
    pub age: Option<i64>,
    pub tid: Option<String>,
}

impl Checked {
    pub fn only(status: Status) -> Self {
        Checked { status, age: None, tid: None }
    }
}

pub fn parse_keys(raw: Option<&str>) -> Option<Keys> {
    let v: Value = serde_json::from_str(raw?).ok()?;
    if v.get("v")?.as_u64()? != 1 {
        return None;
    }
    let date = v.get("d")?.as_str()?.to_string();
    let current = v.get("c").and_then(|c| c.as_u64());
    let mut keys = Vec::new();
    for (w, k) in v.get("k")?.as_object()? {
        let key = hex_decode(k.as_str()?)?;
        if key.len() != 32 {
            return None;
        }
        keys.push((w.parse().ok()?, key));
    }
    if keys.is_empty() {
        None
    } else {
        Some(Keys { date, current, keys })
    }
}

/// `today` = shop.localTime.date; `past_fresh` = shop.localTime.dateTimeAfter(freshUntil);
/// `lines` = (variant number, quantity).
pub fn check(token: Option<&str>, keys_raw: Option<&str>, today: Option<&str>, past_fresh: bool, lines: &[(u64, i64)]) -> Checked {
    let keys = match parse_keys(keys_raw) {
        Some(k) => k,
        None => return Checked::only(Status::KeysMissing),
    };
    if past_fresh {
        return Checked::only(Status::KeysStale);
    }
    // Backstop. The server writes the date in UTC; the shop date is local.
    if let (Some(pub_d), Some(now_d)) = (days(&keys.date), today.and_then(days)) {
        if now_d - pub_d >= 2 {
            return Checked::only(Status::KeysStale);
        }
    }
    let token = match token.map(str::trim) {
        None | Some("") => return Checked::only(Status::Missing),
        Some(t) => t,
    };
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 4 || parts[0] != "1" || !(parts[2] == "h" || parts[2] == "s") || parts[3].len() != 32 {
        return Checked::only(Status::Malformed);
    }
    let window: u64 = match parts[1].parse() {
        Ok(w) if parts[1].len() <= 12 => w,
        _ => return Checked::only(Status::Malformed),
    };
    let sig = match hex_decode(parts[3]) {
        Some(s) => s,
        None => return Checked::only(Status::Malformed),
    };
    let tid = Some(format!("{}.{}", window, &parts[3][..8]));
    let age = keys.current.map(|c| c as i64 - window as i64);
    let done = |status| Checked { status, age, tid: tid.clone() };
    let key = match keys.keys.iter().find(|(w, _)| *w == window) {
        Some((_, k)) => k,
        None => return done(Status::Expired),
    };
    let mut mac = HmacSha256::new_from_slice(key).expect("any key length");
    mac.update(format!("1|{}|{}|{}", window, parts[2], content(lines)).as_bytes());
    // Constant-time comparison of the truncated tag.
    if mac.verify_truncated_left(&sig).is_err() {
        return done(Status::BadSig);
    }
    done(if parts[2] == "h" { Status::Ok } else { Status::Soft })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shared vector: same values are asserted in server/test/lib.test.js.
    const W: u64 = 995432;
    const KEY: &str = "51d5e2631986a6e7ddf19c718d6f8eba8ef5c1ab511299424ff77d454fba0c99";
    const HARD: &str = "1.995432.h.fe2646951b5fe6da175a3580ffebdd1f";
    const SOFT: &str = "1.995432.s.48726d1a28a056f22f361509aff6f5b2";
    const LINES: &[(u64, i64)] = &[(49876543210, 1), (41234567890, 2)];

    fn keys(date: &str, windows: &[u64]) -> String {
        let k: Vec<String> = windows.iter().map(|w| format!("\"{}\":\"{}\"", w, KEY)).collect();
        format!("{{\"v\":1,\"d\":\"{}\",\"c\":{},\"k\":{{{}}}}}", date, W, k.join(","))
    }
    fn live() -> String {
        keys("2026-09-30", &[W - 1, W, W + 1])
    }
    fn st(t: Option<&str>, keys: &str, today: Option<&str>, lines: &[(u64, i64)]) -> Status {
        check(t, Some(keys), today, false, lines).status
    }
    fn chk(t: Option<&str>) -> Status {
        st(t, &live(), Some("2026-09-30"), LINES)
    }

    #[test]
    fn content_is_canonical() {
        assert_eq!(content(LINES), "41234567890:2,49876543210:1");
        // same variant on two lines is summed, zero lines dropped, order does not matter
        assert_eq!(content(&[(5, 1), (3, 1), (5, 2), (9, 0)]), "3:1,5:3");
        assert_eq!(content(&[]), "");
        assert_eq!(variant_number("gid://shopify/ProductVariant/41234567890"), Some(41234567890));
        assert_eq!(variant_number("gid://shopify/CustomProduct/abc"), None);
    }

    #[test]
    fn shared_vector_hard_and_soft() {
        assert_eq!(chk(Some(HARD)), Status::Ok);
        assert_eq!(chk(Some(SOFT)), Status::Soft);
        assert_eq!(chk(Some(&format!("  {}  ", HARD))), Status::Ok);
    }

    #[test]
    fn spec_11_token_from_another_cart_or_changed_cart() {
        let other: &[(u64, i64)] = &[(41234567890, 1), (49876543210, 1)];
        assert_eq!(st(Some(HARD), &live(), Some("2026-09-30"), other), Status::BadSig);
        assert_eq!(st(Some(HARD), &live(), Some("2026-09-30"), &[]), Status::BadSig);
    }

    #[test]
    fn tampering_is_rejected() {
        // flip soft -> hard keeps the signature: must fail
        assert_eq!(chk(Some(&SOFT.replace(".s.", ".h."))), Status::BadSig);
        let mut last = HARD.to_string();
        last.pop();
        last.push('0');
        assert_eq!(chk(Some(&last)), Status::BadSig);
        assert_eq!(chk(Some(&HARD.replace("995432", "995433"))), Status::BadSig); // key exists, sig wrong
    }

    #[test]
    fn spec_1_2_missing_or_junk_token() {
        assert_eq!(chk(None), Status::Missing);
        assert_eq!(chk(Some("")), Status::Missing);
        for bad in ["x", "1.995432.h", "2.995432.h.fe2646951b5fe6da175a3580ffebdd1f", "1.995432.x.fe2646951b5fe6da175a3580ffebdd1f",
                    "1.abc.h.fe2646951b5fe6da175a3580ffebdd1f", "1.995432.h.zz2646951b5fe6da175a3580ffebdd1f",
                    "1.995432.h.fe26", "1.9999999999999.h.fe2646951b5fe6da175a3580ffebdd1f"] {
            assert_eq!(chk(Some(bad)), Status::Malformed, "{}", bad);
        }
    }

    #[test]
    fn expiry_by_rotation() {
        // two windows later the key is gone
        let later = keys("2026-09-30", &[W + 1, W + 2, W + 3]);
        assert_eq!(st(Some(HARD), &later, Some("2026-09-30"), LINES), Status::Expired);
        // one window later: still valid (previous key kept)
        let next = keys("2026-09-30", &[W, W + 1, W + 2]);
        assert_eq!(st(Some(HARD), &next, Some("2026-09-30"), LINES), Status::Ok);
    }

    #[test]
    fn keys_missing_or_broken_fail_open_status() {
        for raw in [None, Some(""), Some("{"), Some("{\"v\":2,\"d\":\"2026-09-30\",\"k\":{}}"),
                    Some("{\"v\":1,\"d\":\"2026-09-30\",\"k\":{}}"), Some("{\"v\":1,\"d\":\"2026-09-30\",\"k\":{\"1\":\"abcd\"}}")] {
            assert_eq!(check(Some(HARD), raw, Some("2026-09-30"), false, LINES).status, Status::KeysMissing, "{:?}", raw);
        }
    }

    #[test]
    fn stale_keys_fail_open_status() {
        let old = keys("2026-09-28", &[W - 1, W, W + 1]);
        assert_eq!(st(None, &old, Some("2026-09-30"), LINES), Status::KeysStale);
        // one day difference (UTC vs shop time zone) is fine
        let yday = keys("2026-09-29", &[W - 1, W, W + 1]);
        assert_eq!(st(Some(HARD), &yday, Some("2026-09-30"), LINES), Status::Ok);
        // unknown dates: no staleness decision, normal check
        assert_eq!(st(Some(HARD), &live(), None, LINES), Status::Ok);
    }

    #[test]
    fn freshness_variable_fails_open_within_the_hour() {
        let c = check(Some(HARD), Some(&live()), Some("2026-09-30"), true, LINES);
        assert_eq!(c.status, Status::KeysStale);
        let c = check(None, Some(&live()), Some("2026-09-30"), true, LINES);
        assert_eq!(c.status, Status::KeysStale, "no token + server down = let through");
    }

    #[test]
    fn correlators_age_and_tid() {
        let c = check(Some(HARD), Some(&live()), Some("2026-09-30"), false, LINES);
        assert_eq!(c.age, Some(0));
        assert_eq!(c.tid.as_deref(), Some("995432.fe264695"));
        let next = keys("2026-09-30", &[W, W + 1, W + 2]).replace(&format!("\"c\":{}", W), &format!("\"c\":{}", W + 1));
        assert_eq!(check(Some(HARD), Some(&next), None, false, LINES).age, Some(1));
        assert_eq!(check(Some("junk"), Some(&live()), None, false, LINES).tid, None);
    }

    #[test]
    fn day_math() {
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(days("2026-03-01").unwrap() - days("2026-02-28").unwrap(), 1);
        assert_eq!(days("2024-03-01").unwrap() - days("2024-02-28").unwrap(), 2);
        assert_eq!(days("2027-01-01").unwrap() - days("2026-12-31").unwrap(), 1);
        assert_eq!(days("2026-13-01"), None);
    }
}
