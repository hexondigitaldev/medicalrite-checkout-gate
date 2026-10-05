//! Stage 1 blocklist rules. Pure logic: no Shopify types, so it is unit-testable.
//!
//! Safety properties (CLAUDE.md, docs/decisions.md):
//! - Only the CHECKOUT_COMPLETION step can ever block.
//! - Missing / disabled / broken settings => allow and say so in the log (fail open, D5).
//! - Any mode other than "enforce" behaves as log_only.
//! - One generic message for every rule.
//! - Address and ZIP rules apply to US addresses only; names are matched without
//!   street-word abbreviations; address entries match exactly (plus an explicit unit tail).
//! - Stage 2 token check has its own `token_mode` (off / log_only / enforce, default off),
//!   so it can be rolled out separately from the blocklist. Key problems fail open.

use crate::token::{Checked, Status as TokenStatus};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum TokenMode {
    #[default]
    Off,
    LogOnly,
    Enforce,
}

/// What a soft token (Turnstile failed or slow) counts as.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum SoftPolicy {
    /// Passes (spec rule 2 / test 12).
    #[default]
    Allow,
    /// Fails only on the bot cart shape: guest, one line, quantity 1, subtotal under $5
    /// (interim until Stage 3 scoring; a real buyer with that cart can still call or log in).
    Risky,
    /// Always fails (emergency switch).
    Block,
}

impl TokenMode {
    fn as_str(self) -> &'static str {
        match self {
            TokenMode::Off => "off",
            TokenMode::LogOnly => "log_only",
            TokenMode::Enforce => "enforce",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    pub enforce: bool,
    pub support_phone: String,
    pub skip_logged_in: bool,
    /// Stored already normalized.
    pub blocked_names: Vec<String>,
    /// (normalized street line, optional 5-digit ZIP it is tied to)
    pub blocked_address1: Vec<(String, Option<String>)>,
    pub blocked_zips: Vec<String>,
    pub blocked_email_domains: Vec<String>,
    pub token_mode: TokenMode,
    /// Spec test 12: soft tokens pass by default (D17 pending with Timur).
    pub soft_tokens: SoftPolicy,
    /// Problems found while reading settings (field names only, never values).
    pub errors: Vec<&'static str>,
}

/// Result of reading the metaobject.
#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    Missing,
    Disabled,
    Enabled(Settings),
}

#[derive(Debug, Clone, Default)]
pub struct Address {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub name: Option<String>,
    pub address1: Option<String>,
    pub zip: Option<String>,
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Checkout {
    pub is_completion: bool,
    pub email: Option<String>,
    pub is_authenticated: bool,
    pub is_b2b: bool,
    pub addresses: Vec<Address>,
    /// Product subtotal (for the soft-token cart shape and for matching logs to orders).
    pub subtotal: Option<String>,
    pub lines: usize,
    pub qty: i64,
    /// None when the token check did not run (token_mode off).
    pub token: Option<Checked>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Not the Pay step: do nothing, log nothing.
    Skip,
    Allow { reason: &'static str },
    /// Exempt buyer (e.g. logged in) whose checkout WOULD have matched: allowed, but logged.
    Exempt { reason: &'static str, rules: Vec<&'static str> },
    WouldBlock { rules: Vec<&'static str> },
    /// `rules` = everything that matched; `enforced` = the ones whose mode is enforce.
    Block { rules: Vec<&'static str>, enforced: Vec<&'static str>, message: String },
}

const STREET_WORDS: &[(&str, &str)] = &[
    ("west", "w"), ("east", "e"), ("north", "n"), ("south", "s"),
    ("street", "st"), ("avenue", "ave"), ("av", "ave"), ("road", "rd"),
    ("boulevard", "blvd"), ("drive", "dr"), ("lane", "ln"), ("place", "pl"),
    ("court", "ct"), ("suite", "ste"), ("apartment", "apt"), ("parkway", "pkwy"),
    ("highway", "hwy"), ("square", "sq"), ("terrace", "ter"),
];

/// Words that may follow a blocked street line (unit designators). Anything else
/// after the entry (e.g. "NW", "Ext") means a different address → no match.
const UNIT_WORDS: &[&str] = &["apt", "ste", "unit", "fl", "floor", "rm", "room"];

fn basic(s: &str) -> Vec<String> {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|w| w.to_string())
        .collect()
}

/// Names: lower-case, punctuation → space, collapse. No abbreviations.
pub fn normalize_name(s: &str) -> String {
    basic(s).join(" ")
}

/// Street lines: as names, plus common street-word abbreviations.
pub fn normalize_address(s: &str) -> String {
    basic(s)
        .into_iter()
        .map(|w| {
            STREET_WORDS
                .iter()
                .find(|(long, _)| *long == w)
                .map(|(_, short)| short.to_string())
                .unwrap_or(w)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `blocked` must already be normalized.
fn address_matches(address1: &str, blocked: &str) -> bool {
    let a = normalize_address(address1);
    if a == blocked {
        return true;
    }
    match a.strip_prefix(blocked).and_then(|rest| rest.strip_prefix(' ')) {
        // "123 main st apt 4", "123 main st 4" (from "#4")
        Some(rest) => {
            let first = rest.split(' ').next().unwrap_or("");
            UNIT_WORDS.contains(&first) || (!first.is_empty() && first.chars().all(|c| c.is_ascii_digit()))
        }
        None => false,
    }
}

/// Exactly 5 digits, or 5+4 digits. Anything else → None.
fn us_zip5(zip: &str) -> Option<String> {
    let z = zip.trim();
    let digits: String = z.chars().filter(|c| c.is_ascii_digit()).collect();
    let shape_ok = match z.len() {
        5 => digits.len() == 5,
        9 => digits.len() == 9,
        10 => digits.len() == 9 && z.as_bytes()[5] == b'-',
        _ => false,
    };
    if shape_ok { Some(digits[..5].to_string()) } else { None }
}

fn is_us(a: &Address) -> bool {
    a.country_code.as_deref().map(|c| c.eq_ignore_ascii_case("US")).unwrap_or(false)
}

fn full_name(a: &Address) -> String {
    let joined = format!(
        "{} {}",
        a.first_name.as_deref().unwrap_or(""),
        a.last_name.as_deref().unwrap_or("")
    );
    let n = normalize_name(&joined);
    if n.is_empty() { normalize_name(a.name.as_deref().unwrap_or("")) } else { n }
}

pub fn block_message(support_phone: &str) -> String {
    let phone = support_phone.trim();
    if phone.is_empty() {
        "We couldn't verify this checkout. Please refresh the page and try again, or contact us and we'll help.".to_string()
    } else {
        format!(
            "We couldn't verify this checkout. Please refresh the page and try again, or call us at {} and we'll help.",
            phone
        )
    }
}

fn list(obj: &serde_json::Map<String, Value>, key: &'static str, norm: fn(&str) -> String, errors: &mut Vec<&'static str>) -> Vec<String> {
    match obj.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for it in items {
                match it.as_str() {
                    Some(s) => {
                        let n = norm(s);
                        if n.is_empty() {
                            if !s.trim().is_empty() && !errors.contains(&key) {
                                errors.push(key) // e.g. ZIP "1008O"
                            }
                        } else if !out.contains(&n) {
                            out.push(n)
                        }
                    }
                    None => {
                        if !errors.contains(&key) {
                            errors.push(key)
                        }
                    }
                }
            }
            out
        }
        Some(_) => {
            errors.push(key);
            Vec::new()
        }
    }
}

fn domain(s: &str) -> String {
    s.trim().trim_start_matches('@').to_lowercase()
}

fn zip_entry(s: &str) -> String {
    us_zip5(s).unwrap_or_default()
}

/// Address entries: "428 st" (anywhere) or "428 w 45th st|10036" (only with that ZIP).
fn address_entries(obj: &serde_json::Map<String, Value>, errors: &mut Vec<&'static str>) -> Vec<(String, Option<String>)> {
    const KEY: &str = "blocked_address1";
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut bad = false;
    match obj.get(KEY) {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for it in items {
                let Some(raw) = it.as_str() else { bad = true; continue };
                if raw.trim().is_empty() {
                    continue;
                }
                let (addr, zip) = match raw.split_once('|') {
                    Some((a, z)) => match us_zip5(z) {
                        Some(z5) => (normalize_address(a), Some(z5)),
                        None => { bad = true; continue }
                    },
                    None => (normalize_address(raw), None),
                };
                if addr.is_empty() {
                    bad = true;
                    continue;
                }
                let e = (addr, zip);
                if !out.contains(&e) {
                    out.push(e);
                }
            }
        }
        Some(_) => bad = true,
    }
    if bad {
        errors.push(KEY);
    }
    out
}

/// `enabled` and `config` are the raw metaobject field values (None = field absent).
pub fn load(metaobject_present: bool, enabled: Option<&str>, config: Option<&str>) -> Loaded {
    if !metaobject_present {
        return Loaded::Missing;
    }
    if enabled.map(|v| v.trim()) != Some("true") {
        return Loaded::Disabled;
    }
    let mut errors: Vec<&'static str> = Vec::new();
    let obj = match config.map(serde_json::from_str::<Value>) {
        Some(Ok(Value::Object(m))) => m,
        None => {
            errors.push("config_missing");
            serde_json::Map::new()
        }
        _ => {
            errors.push("config_invalid_json");
            serde_json::Map::new()
        }
    };
    let enforce = match obj.get("mode") {
        Some(Value::String(m)) => m.trim().eq_ignore_ascii_case("enforce"),
        None => false,
        Some(_) => {
            errors.push("mode");
            false
        }
    };
    let support_phone = obj.get("support_phone").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let skip_logged_in = match obj.get("skip_logged_in") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => {
            errors.push("skip_logged_in");
            true
        }
    };
    let token_mode = match obj.get("token_mode") {
        None | Some(Value::Null) => TokenMode::Off,
        Some(Value::String(m)) => match m.trim().to_ascii_lowercase().as_str() {
            "off" => TokenMode::Off,
            "log_only" => TokenMode::LogOnly,
            "enforce" => TokenMode::Enforce,
            _ => {
                errors.push("token_mode");
                TokenMode::LogOnly // typo never blocks, but stays visible
            }
        },
        Some(_) => {
            errors.push("token_mode");
            TokenMode::LogOnly
        }
    };
    let soft_tokens = match obj.get("soft_tokens") {
        None | Some(Value::Null) => SoftPolicy::Allow,
        Some(Value::String(v)) => match v.trim().to_ascii_lowercase().as_str() {
            "allow" => SoftPolicy::Allow,
            "risky" => SoftPolicy::Risky,
            "block" => SoftPolicy::Block,
            _ => {
                errors.push("soft_tokens");
                SoftPolicy::Allow
            }
        },
        Some(_) => {
            errors.push("soft_tokens");
            SoftPolicy::Allow
        }
    };
    const KNOWN: &[&str] = &[
        "mode", "support_phone", "skip_logged_in", "blocked_names",
        "blocked_address1", "blocked_zips", "blocked_email_domains",
        "token_mode", "soft_tokens",
    ];
    if obj.keys().any(|k| !KNOWN.contains(&k.as_str())) {
        errors.push("unknown_key");
    }
    Loaded::Enabled(Settings {
        enforce,
        support_phone,
        skip_logged_in,
        blocked_names: list(&obj, "blocked_names", normalize_name, &mut errors),
        blocked_address1: address_entries(&obj, &mut errors),
        blocked_zips: list(&obj, "blocked_zips", zip_entry, &mut errors),
        blocked_email_domains: list(&obj, "blocked_email_domains", domain, &mut errors),
        token_mode,
        soft_tokens,
        errors,
    })
}

/// Whether the caller needs to compute the token status at all.
pub fn token_mode(loaded: &Loaded) -> TokenMode {
    match loaded {
        Loaded::Enabled(s) => s.token_mode,
        _ => TokenMode::Off,
    }
}

/// Guest, one line, quantity 1, product subtotal under $5 — the shape of every bot order so far.
fn bot_shape(c: &Checkout) -> bool {
    let sub = c.subtotal.as_deref().and_then(|v| v.trim().parse::<f64>().ok());
    !c.is_authenticated && c.lines == 1 && c.qty == 1 && matches!(sub, Some(v) if v < 5.0)
}

/// Rule name for a token problem; None = token fine or check failed open.
fn token_rule(status: TokenStatus, soft: SoftPolicy, shape: bool) -> Option<&'static str> {
    match status {
        TokenStatus::Ok | TokenStatus::KeysMissing | TokenStatus::KeysStale => None,
        TokenStatus::Soft => match soft {
            SoftPolicy::Allow => None,
            SoftPolicy::Risky => if shape { Some("token_soft_risky") } else { None },
            SoftPolicy::Block => Some("token_soft"),
        },
        TokenStatus::Missing => Some("token_missing"),
        TokenStatus::Malformed | TokenStatus::BadSig => Some("token_invalid"),
        TokenStatus::Expired => Some("token_expired"),
    }
}

pub fn decide(c: &Checkout, loaded: &Loaded) -> Decision {
    if !c.is_completion {
        return Decision::Skip;
    }
    let s = match loaded {
        Loaded::Missing => return Decision::Allow { reason: "no_settings" },
        Loaded::Disabled => return Decision::Allow { reason: "disabled" },
        Loaded::Enabled(s) => s,
    };
    let mut rules: Vec<&'static str> = Vec::new();
    let mut hit = |r: &'static str| {
        if !rules.contains(&r) {
            rules.push(r)
        }
    };
    for a in &c.addresses {
        let name = full_name(a);
        if !name.is_empty() && s.blocked_names.iter().any(|b| *b == name) {
            hit("blocked_name");
        }
        if is_us(a) {
            let zip = a.zip.as_deref().and_then(us_zip5);
            if let Some(a1) = a.address1.as_deref() {
                if s.blocked_address1.iter().any(|(b, bz)| {
                    address_matches(a1, b) && bz.as_ref().map_or(true, |bz| zip.as_ref() == Some(bz))
                }) {
                    hit("blocked_address1");
                }
            }
            if let Some(z) = a.zip.as_deref().and_then(us_zip5) {
                if s.blocked_zips.iter().any(|b| *b == z) {
                    hit("blocked_zip");
                }
            }
        }
    }
    if let Some((_, d)) = c.email.as_deref().and_then(|e| e.rsplit_once('@')) {
        let d = d.trim().to_lowercase();
        if !d.is_empty() && s.blocked_email_domains.iter().any(|b| *b == d) {
            hit("blocked_email_domain");
        }
    }

    let block_rules = rules;
    let token_hit = match (s.token_mode, &c.token) {
        (TokenMode::Off, _) | (_, None) => None,
        (_, Some(t)) => token_rule(t.status, s.soft_tokens, bot_shape(c)),
    };
    let mut all = block_rules.clone();
    all.extend(token_hit);
    if all.is_empty() {
        return Decision::Allow { reason: "clean" };
    }
    if c.is_b2b {
        return Decision::Exempt { reason: "exempt_b2b", rules: all };
    }
    // Logged in: the token never applies (D3); the blocklist applies only if skip_logged_in is off (D11).
    let (eff_block, eff_token) = if c.is_authenticated {
        (if s.skip_logged_in { Vec::new() } else { block_rules }, None)
    } else {
        (block_rules, token_hit)
    };
    let mut enforced: Vec<&'static str> = Vec::new();
    if s.enforce {
        enforced.extend(eff_block.iter().copied());
    }
    if s.token_mode == TokenMode::Enforce {
        enforced.extend(eff_token);
    }
    if !enforced.is_empty() {
        Decision::Block { rules: all, enforced, message: block_message(&s.support_phone) }
    } else if !eff_block.is_empty() || eff_token.is_some() {
        Decision::WouldBlock { rules: all }
    } else {
        Decision::Exempt { reason: "exempt_logged_in", rules: all }
    }
}

fn json_str_list(items: &[&'static str]) -> String {
    items.iter().map(|x| format!("\"{}\"", x)).collect::<Vec<_>>().join(",")
}

/// One log line per Pay-step decision. No personal data: only rule names, settings
/// health, and cart shape (total / line count / quantity / logged-in) for matching to orders.
pub fn log_line(d: &Decision, loaded: &Loaded, c: &Checkout) -> Option<String> {
    let (decision, detail) = match d {
        Decision::Skip => return None,
        Decision::Allow { reason } => ("allow", format!("\"reason\":\"{}\"", reason)),
        Decision::Exempt { reason, rules } => ("allow", format!("\"reason\":\"{}\",\"rules\":[{}]", reason, json_str_list(rules))),
        Decision::WouldBlock { rules } => ("would_block", format!("\"rules\":[{}]", json_str_list(rules))),
        Decision::Block { rules, enforced, .. } => ("block", format!("\"rules\":[{}],\"enf\":[{}]", json_str_list(rules), json_str_list(enforced))),
    };
    let settings = match loaded {
        Loaded::Missing => "\"mode\":\"none\"".to_string(),
        Loaded::Disabled => "\"mode\":\"disabled\"".to_string(),
        Loaded::Enabled(s) => format!(
            "\"mode\":\"{}\",\"tmode\":\"{}\",\"tok\":\"{}\",{}\"n\":[{},{},{},{}],\"cfg_errors\":[{}],\"phone_missing\":{}",
            if s.enforce { "enforce" } else { "log_only" },
            s.token_mode.as_str(),
            c.token.as_ref().map(|t| t.status.as_str()).unwrap_or("off"),
            match &c.token {
                Some(t) => format!(
                    "{}{}",
                    t.tid.as_ref().map(|x| format!("\"tid\":\"{}\",", x)).unwrap_or_default(),
                    t.age.map(|a| format!("\"tage\":{},", a)).unwrap_or_default()
                ),
                None => String::new(),
            },
            s.blocked_names.len(), s.blocked_address1.len(), s.blocked_zips.len(), s.blocked_email_domains.len(),
            json_str_list(&s.errors),
            s.support_phone.is_empty()
        ),
    };
    let sub: String = c.subtotal.as_deref().unwrap_or("").chars().filter(|ch| ch.is_ascii_digit() || *ch == '.').collect();
    Some(format!(
        "{{\"v\":3,\"decision\":\"{}\",{},{},\"sub\":\"{}\",\"lines\":{},\"qty\":{},\"authed\":{},\"addr\":{},\"email\":{}}}",
        decision, detail, settings, sub, c.lines, c.qty, c.is_authenticated,
        c.addresses.len(), c.email.as_deref().map(|e| !e.trim().is_empty()).unwrap_or(false)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
        "mode": "enforce",
        "support_phone": "(800) 548-6877",
        "blocked_names": ["james anderson"],
        "blocked_address1": ["428 st", "428 w 45th st|10036", "230 west 55th street|10019", "123 main st|10080"],
        "blocked_zips": ["10080"],
        "blocked_email_domains": []
    }"#;

    fn loaded(config: &str) -> Loaded {
        load(true, Some("true"), Some(config))
    }
    fn enforce() -> Loaded {
        loaded(CONFIG)
    }
    fn log_only() -> Loaded {
        loaded(&CONFIG.replace("\"enforce\"", "\"log_only\""))
    }

    fn checkout(name: (&str, &str), address1: &str, zip: &str, email: &str) -> Checkout {
        Checkout {
            is_completion: true,
            email: Some(email.into()),
            is_authenticated: false,
            is_b2b: false,
            addresses: vec![Address {
                first_name: Some(name.0.into()),
                last_name: Some(name.1.into()),
                name: None,
                address1: Some(address1.into()),
                zip: Some(zip.into()),
                country_code: Some("US".into()),
            }],
            subtotal: Some("0.99".into()),
            lines: 1,
            qty: 1,
            token: None,
        }
    }
    fn bot() -> Checkout {
        checkout(("James", "Anderson"), "428 st", "10080", "x@y.com")
    }
    fn is_block(d: &Decision) -> bool {
        matches!(d, Decision::Block { .. })
    }
    fn rules_of(d: Decision) -> Vec<&'static str> {
        match d {
            Decision::Block { rules, .. } | Decision::WouldBlock { rules } => rules,
            other => panic!("expected a block, got {:?}", other),
        }
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize_address("  230 West 55th Street. "), "230 w 55th st");
        assert_eq!(normalize_address("428 W. 45th St"), "428 w 45th st");
        assert_eq!(normalize_name("John West"), "john west");
    }

    #[test]
    fn spec_case_5_blocked_name() {
        let c = checkout(("James", "Anderson"), "500 Park Ave", "10022", "x@y.com");
        assert_eq!(
            decide(&c, &enforce()),
            Decision::Block { rules: vec!["blocked_name"], enforced: vec!["blocked_name"], message: block_message("(800) 548-6877") }
        );
    }

    #[test]
    fn spec_case_6_blocked_short_address() {
        assert!(is_block(&decide(&checkout(("M", "S"), "428 st", "10001", "m@y.com"), &enforce())));
    }

    #[test]
    fn long_form_matches() {
        let d = decide(&checkout(("A", "B"), "230 West 55th Street", "10019", "a@b.com"), &enforce());
        assert_eq!(rules_of(d), vec!["blocked_address1"]);
    }

    #[test]
    fn exact_with_trailing_dot() {
        assert!(is_block(&decide(&checkout(("A", "B"), "123 Main St.", "10080", "a@b.com"), &enforce())));
    }

    #[test]
    fn unit_tail_matches() {
        for a in ["123 Main St Apt 4", "123 Main St #4", "123 main street suite 200", "123 Main St Unit B"] {
            assert!(is_block(&decide(&checkout(("A", "B"), a, "10080", "a@b.com"), &enforce())), "{}", a);
        }
    }

    #[test]
    fn different_streets_do_not_match() {
        for a in ["123 Main St NW", "123 Main Street Northwest", "123 Main St Ext", "428 St Marks Pl", "4280 st", "1428 st"] {
            assert_eq!(decide(&checkout(("A", "B"), a, "20001", "a@b.com"), &enforce()), Decision::Allow { reason: "clean" }, "{}", a);
        }
    }

    #[test]
    fn zip_rules() {
        assert_eq!(rules_of(decide(&checkout(("A", "B"), "1 Real Rd", "10080-1234", "a@b.com"), &enforce())), vec!["blocked_zip"]);
        assert_eq!(rules_of(decide(&checkout(("A", "B"), "1 Real Rd", "100801234", "a@b.com"), &enforce())), vec!["blocked_zip"]);
        for z in ["100800", "1 0 0 8 0", "10080 "] {
            let d = decide(&checkout(("A", "B"), "1 Real Rd", z, "a@b.com"), &enforce());
            if z == "10080 " {
                assert!(is_block(&d), "trimmed 5-digit zip should match");
            } else {
                assert_eq!(d, Decision::Allow { reason: "clean" }, "{:?}", z);
            }
        }
    }

    #[test]
    fn address_and_zip_rules_are_us_only() {
        let mut c = checkout(("A", "B"), "428 st", "10080", "a@b.com");
        c.addresses[0].country_code = Some("IT".into());
        assert_eq!(decide(&c, &enforce()), Decision::Allow { reason: "clean" });
        c.addresses[0].country_code = None;
        assert_eq!(decide(&c, &enforce()), Decision::Allow { reason: "clean" });
    }

    #[test]
    fn names_are_not_abbreviated() {
        let cfg = CONFIG.replace("\"james anderson\"", "\"john west\"");
        assert_eq!(decide(&checkout(("John", "W."), "1 Real Rd", "90210", "a@b.com"), &loaded(&cfg)), Decision::Allow { reason: "clean" });
        assert!(is_block(&decide(&checkout(("John", "West"), "1 Real Rd", "90210", "a@b.com"), &loaded(&cfg))));
    }

    #[test]
    fn email_domain_rule_and_empty_entries_ignored() {
        let cfg = CONFIG.replace("\"blocked_email_domains\": []", "\"blocked_email_domains\": [\"@Zoho.com\", \"@\", \"\"]");
        let l = loaded(&cfg);
        assert!(is_block(&decide(&checkout(("A", "B"), "1 Real Rd", "90210", "abc@zoho.com"), &l)));
        assert_eq!(decide(&checkout(("A", "B"), "1 Real Rd", "90210", "abc@"), &l), Decision::Allow { reason: "clean" });
        if let Loaded::Enabled(s) = l { assert_eq!(s.blocked_email_domains, vec!["zoho.com"]); }
    }

    #[test]
    fn spec_case_8_real_order_goes_through() {
        let c = checkout(("Jane", "Doe"), "12312 W Olympic Blvd", "90064", "jane.doe@gmail.com");
        assert_eq!(decide(&c, &enforce()), Decision::Allow { reason: "clean" });
    }

    #[test]
    fn multiple_rules_once_each_and_multiple_groups() {
        let mut c = bot();
        c.addresses.push(c.addresses[0].clone());
        assert_eq!(rules_of(decide(&c, &enforce())), vec!["blocked_name", "blocked_address1", "blocked_zip"]);
    }

    #[test]
    fn spec_case_13_log_only_never_blocks() {
        assert!(matches!(decide(&bot(), &log_only()), Decision::WouldBlock { .. }));
    }

    #[test]
    fn unknown_mode_is_log_only() {
        let l = loaded(&CONFIG.replace("\"enforce\"", "\"enforce-typo\""));
        assert!(matches!(decide(&bot(), &l), Decision::WouldBlock { .. }));
    }

    #[test]
    fn spec_case_14_disabled_and_missing_fail_open() {
        assert_eq!(decide(&bot(), &load(true, Some("false"), Some(CONFIG))), Decision::Allow { reason: "disabled" });
        assert_eq!(decide(&bot(), &load(true, None, Some(CONFIG))), Decision::Allow { reason: "disabled" });
        assert_eq!(decide(&bot(), &load(false, None, None)), Decision::Allow { reason: "no_settings" });
    }

    #[test]
    fn broken_config_fails_open_and_is_reported() {
        let l = load(true, Some("true"), Some("{\"blocked_address1\": [\"428 st\""));
        assert_eq!(decide(&bot(), &l), Decision::Allow { reason: "clean" });
        let line = log_line(&decide(&bot(), &l), &l, &bot()).unwrap();
        assert!(line.contains("config_invalid_json"), "{}", line);

        let l = load(true, Some("true"), Some("{\"blocked_names\": \"james anderson\", \"blocked_zips\": [10080]}"));
        if let Loaded::Enabled(s) = &l { assert_eq!(s.errors, vec!["blocked_names", "blocked_zips"]); } else { panic!() }

        let l = load(true, Some("true"), None);
        assert!(log_line(&decide(&bot(), &l), &l, &bot()).unwrap().contains("config_missing"));
    }

    #[test]
    fn non_completion_steps_never_block_or_log() {
        let mut c = bot();
        c.is_completion = false;
        let d = decide(&c, &enforce());
        assert_eq!(d, Decision::Skip);
        assert_eq!(log_line(&d, &enforce(), &c), None);
    }

    #[test]
    fn b2b_and_logged_in_exemptions() {
        let mut c = bot();
        c.is_b2b = true;
        assert!(matches!(decide(&c, &enforce()), Decision::Exempt { reason: "exempt_b2b", .. }));
        let mut c = bot();
        c.is_authenticated = true;
        assert_eq!(
            decide(&c, &enforce()),
            Decision::Exempt { reason: "exempt_logged_in", rules: vec!["blocked_name", "blocked_address1", "blocked_zip"] }
        );
        let line = log_line(&decide(&c, &log_only()), &log_only(), &c).unwrap();
        assert!(line.contains("exempt_logged_in") && line.contains("blocked_address1"), "would-be hits stay visible: {}", line);
        // a clean logged-in buyer is just "clean"
        let mut real = checkout(("Jane", "Doe"), "1 Real Rd", "90210", "j@d.com");
        real.is_authenticated = true;
        assert_eq!(decide(&real, &enforce()), Decision::Allow { reason: "clean" });
        let l = loaded(&CONFIG.replace("\"mode\"", "\"skip_logged_in\": false, \"mode\""));
        assert!(is_block(&decide(&c, &l)), "skip_logged_in=false applies blocklist to logged-in buyers");
    }

    #[test]
    fn empty_fields_do_not_crash_or_match() {
        let c = Checkout { is_completion: true, addresses: vec![Address::default()], ..Default::default() };
        assert_eq!(decide(&c, &enforce()), Decision::Allow { reason: "clean" });
        assert!(log_line(&decide(&c, &enforce()), &enforce(), &c).is_some());
    }

    #[test]
    fn message_and_phone_flag() {
        assert!(block_message("").contains("contact us"));
        assert!(block_message("(800) 548-6877").contains("(800) 548-6877"));
        let l = loaded(&CONFIG.replace("(800) 548-6877", ""));
        assert!(log_line(&decide(&bot(), &l), &l, &bot()).unwrap().contains("\"phone_missing\":true"));
    }

    #[test]
    fn log_line_has_no_personal_data_but_has_correlators() {
        let c = checkout(("James", "Anderson"), "428 st", "10080", "secret@y.com");
        let l = log_only();
        let line = log_line(&decide(&c, &l), &l, &c).unwrap();
        for pii in ["james", "anderson", "428", "10080", "secret"] {
            assert!(!line.to_lowercase().contains(pii), "{} in {}", pii, line);
        }
        assert!(line.contains("\"decision\":\"would_block\""));
        assert!(line.contains("\"sub\":\"0.99\"") && line.contains("\"n\":[1,4,1,0]"), "{}", line);
        // must be valid JSON for the log summary script
        serde_json::from_str::<Value>(&line).unwrap();
    }

    #[test]
    fn csv_like_patterns_all_caught() {
        // Anonymised shapes of the three drops + 123 Main St seen in the bot CSV.
        let cases = [
            ("Mary", "Smith", "428 st", "10080"),
            ("Tom", "Lee", "428 W 45th St", "10036"),
            ("Ann", "Kim", "230 West 55th Street", "10019"),
            ("Joe", "Ray", "123 Main St", "10080"),
        ];
        for (f, l, a, z) in cases {
            assert!(is_block(&decide(&checkout((f, l), a, z, "q@w.com"), &enforce())), "{}", a);
        }
    }

    #[test]
    fn config_mistakes_are_reported() {
        let l = loaded(&CONFIG.replace("\"mode\"", "\"skip_logged_in\": \"false\", \"mode\""));
        if let Loaded::Enabled(s) = &l { assert!(s.skip_logged_in); assert!(s.errors.contains(&"skip_logged_in")); } else { panic!() }
        let l = loaded(&CONFIG.replace("blocked_address1", "blocked_adress1"));
        if let Loaded::Enabled(s) = &l { assert!(s.errors.contains(&"unknown_key")); } else { panic!() }
        let l = loaded(&CONFIG.replace("[\"10080\"]", "[\"1008O\"]"));
        if let Loaded::Enabled(s) = &l { assert!(s.errors.contains(&"blocked_zips")); assert!(s.blocked_zips.is_empty()); } else { panic!() }
        if let Loaded::Enabled(s) = enforce() { assert!(s.errors.is_empty(), "{:?}", s.errors); }
    }

    #[test]
    fn log_line_shows_when_input_data_is_missing() {
        let c = Checkout { is_completion: true, ..Default::default() };
        let line = log_line(&decide(&c, &enforce()), &enforce(), &c).unwrap();
        assert!(line.contains("\"addr\":0") && line.contains("\"email\":false"), "{}", line);
        let line = log_line(&decide(&bot(), &enforce()), &enforce(), &bot()).unwrap();
        assert!(line.contains("\"addr\":1") && line.contains("\"email\":true"), "{}", line);
    }

    #[test]
    fn zip_scoped_addresses_only_match_with_their_zip() {
        // Real buildings: blocked only with the ZIP the bots used (D12).
        assert!(is_block(&decide(&checkout(("A", "B"), "428 W 45th St", "10036", "a@b.com"), &enforce())));
        assert!(is_block(&decide(&checkout(("A", "B"), "230 West 55th Street", "10019-1234", "a@b.com"), &enforce())));
        for (a, z) in [("428 W 45th St", "10018"), ("230 W 55th St", "90210"), ("123 Main St", "11111"), ("428 W 45th St", "")] {
            assert_eq!(decide(&checkout(("A", "B"), a, z, "a@b.com"), &enforce()), Decision::Allow { reason: "clean" }, "{} {}", a, z);
        }
        // Unscoped "428 st" still blocks with any ZIP.
        assert!(is_block(&decide(&checkout(("A", "B"), "428 st", "30308", "a@b.com"), &enforce())));
    }

    #[test]
    fn bad_zip_scope_is_reported_and_skipped() {
        let cfg = CONFIG.replace("\"428 w 45th st|10036\"", "\"428 w 45th st|1003\"");
        let l = loaded(&cfg);
        if let Loaded::Enabled(s) = &l {
            assert!(s.errors.contains(&"blocked_address1"));
            assert!(!s.blocked_address1.iter().any(|(a, _)| a == "428 w 45th st"));
            assert_eq!(s.blocked_address1.len(), 3);
        } else { panic!() }
    }

    // ---- Stage 2: token mode ----

    fn with_token(mode: &str, soft: Option<&str>) -> Loaded {
        let mut extra = format!("\"token_mode\": \"{}\", ", mode);
        if let Some(v) = soft { extra.push_str(&format!("\"soft_tokens\": \"{}\", ", v)); }
        loaded(&CONFIG.replace("\"mode\": \"enforce\"", &format!("{}\"mode\": \"log_only\"", extra)))
    }
    fn real(tok: TokenStatus) -> Checkout {
        let mut c = checkout(("Jane", "Doe"), "12312 W Olympic Blvd", "90064", "jane.doe@gmail.com");
        c.token = Some(Checked::only(tok));
        c
    }

    #[test]
    fn token_off_by_default_and_for_old_configs() {
        let l = enforce();
        assert_eq!(token_mode(&l), TokenMode::Off);
        assert_eq!(decide(&real(TokenStatus::Missing), &l), Decision::Allow { reason: "clean" });
        assert_eq!(token_mode(&load(true, Some("false"), Some(CONFIG))), TokenMode::Off);
        assert_eq!(token_mode(&load(false, None, None)), TokenMode::Off);
    }

    #[test]
    fn token_enforce_blocks_missing_invalid_expired() {
        let l = with_token("enforce", None);
        for (st, rule) in [(TokenStatus::Missing, "token_missing"), (TokenStatus::Malformed, "token_invalid"),
                           (TokenStatus::BadSig, "token_invalid"), (TokenStatus::Expired, "token_expired")] {
            assert_eq!(decide(&real(st), &l), Decision::Block { rules: vec![rule], enforced: vec![rule], message: block_message("(800) 548-6877") }, "{:?}", st);
        }
        assert_eq!(decide(&real(TokenStatus::Ok), &l), Decision::Allow { reason: "clean" });
    }

    #[test]
    fn spec_12_soft_token_passes_unless_switched_off() {
        assert_eq!(decide(&real(TokenStatus::Soft), &with_token("enforce", None)), Decision::Allow { reason: "clean" });
        assert_eq!(decide(&real(TokenStatus::Soft), &with_token("enforce", Some("allow"))), Decision::Allow { reason: "clean" });
        assert!(is_block(&decide(&real(TokenStatus::Soft), &with_token("enforce", Some("block")))));
    }

    #[test]
    fn token_keys_problems_fail_open() {
        let l = with_token("enforce", None);
        for st in [TokenStatus::KeysMissing, TokenStatus::KeysStale] {
            assert_eq!(decide(&real(st), &l), Decision::Allow { reason: "clean" }, "{:?}", st);
        }
    }

    #[test]
    fn token_log_only_never_blocks_even_with_blocklist_log_only() {
        let l = with_token("log_only", None);
        assert_eq!(decide(&real(TokenStatus::Missing), &l), Decision::WouldBlock { rules: vec!["token_missing"] });
        let line = log_line(&decide(&real(TokenStatus::Missing), &l), &l, &real(TokenStatus::Missing)).unwrap();
        assert!(line.contains("\"tmode\":\"log_only\"") && line.contains("\"tok\":\"missing\""), "{}", line);
        serde_json::from_str::<Value>(&line).unwrap();
    }

    #[test]
    fn modes_are_independent() {
        // blocklist enforce + token log_only: token problem alone does not block
        let l = loaded(&CONFIG.replace("\"mode\"", "\"token_mode\": \"log_only\", \"mode\""));
        assert!(matches!(decide(&real(TokenStatus::Missing), &l), Decision::WouldBlock { .. }));
        let mut b = bot();
        b.token = Some(Checked::only(TokenStatus::Missing));
        assert_eq!(rules_of(decide(&b, &l)), vec!["blocked_name", "blocked_address1", "blocked_zip", "token_missing"]);
        assert!(is_block(&decide(&b, &l)));
        // blocklist log_only + token enforce: blocklist hit alone does not block
        let l = with_token("enforce", None);
        let mut b = bot();
        b.token = Some(Checked::only(TokenStatus::Ok));
        assert!(matches!(decide(&b, &l), Decision::WouldBlock { .. }));
    }

    #[test]
    fn token_exemptions_logged_in_and_b2b() {
        let l = with_token("enforce", None);
        let mut c = real(TokenStatus::Missing);
        c.is_authenticated = true;
        assert_eq!(decide(&c, &l), Decision::Exempt { reason: "exempt_logged_in", rules: vec!["token_missing"] });
        let mut c = real(TokenStatus::Missing);
        c.is_b2b = true;
        assert!(matches!(decide(&c, &l), Decision::Exempt { reason: "exempt_b2b", .. }));
    }

    #[test]
    fn token_setting_mistakes_never_block() {
        let l = with_token("enforse", Some("maybe"));
        assert_eq!(token_mode(&l), TokenMode::LogOnly);
        if let Loaded::Enabled(s) = &l {
            assert!(s.errors.contains(&"token_mode") && s.errors.contains(&"soft_tokens"), "{:?}", s.errors);
            assert_eq!(s.soft_tokens, SoftPolicy::Allow);
        } else { panic!() }
        assert!(!is_block(&decide(&real(TokenStatus::Missing), &l)));
        let l = with_token("off", None);
        if let Loaded::Enabled(s) = &l { assert!(s.errors.is_empty(), "{:?}", s.errors); }
    }

    #[test]
    fn soft_risky_only_fails_the_bot_cart_shape() {
        let l = with_token("enforce", Some("risky"));
        let mut c = real(TokenStatus::Soft); // guest, 1 line, qty 1, subtotal 0.99
        assert_eq!(rules_of(decide(&c, &l)), vec!["token_soft_risky"]);
        c.subtotal = Some("59.99".into()); // spec test 8 cart
        assert_eq!(decide(&c, &l), Decision::Allow { reason: "clean" });
        let mut c = real(TokenStatus::Soft);
        c.qty = 2;
        assert_eq!(decide(&c, &l), Decision::Allow { reason: "clean" });
        let mut c = real(TokenStatus::Soft);
        c.subtotal = None;
        assert_eq!(decide(&c, &l), Decision::Allow { reason: "clean" }, "unknown subtotal never counts as risky");
        let mut c = real(TokenStatus::Soft);
        c.is_authenticated = true;
        assert_eq!(decide(&c, &l), Decision::Allow { reason: "clean" });
        // hard token with the same shape is fine
        assert_eq!(decide(&real(TokenStatus::Ok), &l), Decision::Allow { reason: "clean" });
    }

    #[test]
    fn logged_in_always_skip_token_even_when_blocklist_applies_to_them() {
        let l = loaded(&CONFIG.replace("\"mode\"", "\"skip_logged_in\": false, \"token_mode\": \"enforce\", \"mode\""));
        let mut c = real(TokenStatus::Missing);
        c.is_authenticated = true;
        assert_eq!(decide(&c, &l), Decision::Exempt { reason: "exempt_logged_in", rules: vec!["token_missing"] });
        let mut b = bot();
        b.is_authenticated = true;
        b.token = Some(Checked::only(TokenStatus::Missing));
        match decide(&b, &l) {
            Decision::Block { enforced, rules, .. } => {
                assert_eq!(enforced, vec!["blocked_name", "blocked_address1", "blocked_zip"]);
                assert!(rules.contains(&"token_missing"));
            }
            d => panic!("{:?}", d),
        }
    }

    #[test]
    fn block_line_separates_enforced_rules() {
        let l = loaded(&CONFIG.replace("\"mode\"", "\"token_mode\": \"log_only\", \"mode\""));
        let mut b = bot();
        b.token = Some(Checked { status: TokenStatus::BadSig, age: Some(1), tid: Some("995432.fe264695".into()) });
        let line = log_line(&decide(&b, &l), &l, &b).unwrap();
        assert!(line.contains("\"enf\":[\"blocked_name\",\"blocked_address1\",\"blocked_zip\"]"), "{}", line);
        assert!(line.contains("\"tok\":\"bad_sig\",\"tid\":\"995432.fe264695\",\"tage\":1,"), "{}", line);
        serde_json::from_str::<Value>(&line).unwrap();
    }
}
