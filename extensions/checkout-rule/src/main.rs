//! Checkout rule (Cart & Checkout Validation). Blocklist logic in rules.rs, token check in token.rs.
use shopify_function::prelude::*;
use shopify_function::Result;
use std::process;

pub mod rules;
pub mod token;

#[typegen("schema.graphql")]
pub mod schema {
    #[query("src/run.graphql")]
    pub mod run {}
}

fn no_errors() -> Result<schema::CartValidationsGenerateRunResult> {
    Ok(schema::CartValidationsGenerateRunResult {
        operations: vec![schema::Operation::ValidationAdd(
            schema::ValidationAddOperation { errors: vec![] },
        )],
    })
}

#[shopify_function]
fn cart_validations_generate_run(
    input: schema::run::Input,
) -> Result<schema::CartValidationsGenerateRunResult> {
    // Cheapest possible exit for the many cart / checkout-interaction runs.
    if !matches!(
        input.buyer_journey().step(),
        Some(schema::BuyerJourneyStep::CheckoutCompletion)
    ) {
        return no_errors();
    }

    let meta = input.shop().settings();
    let loaded = rules::load(
        meta.is_some(),
        meta.and_then(|m| m.enabled()).and_then(|f| f.value()).map(|s| s.as_str()),
        meta.and_then(|m| m.config()).and_then(|f| f.value()).map(|s| s.as_str()),
    );

    let cart = input.cart();
    let buyer = cart.buyer_identity();
    let checkout = rules::Checkout {
        is_completion: true,
        email: buyer.and_then(|b| b.email()).cloned(),
        is_authenticated: buyer.map(|b| *b.is_authenticated()).unwrap_or(false),
        is_b2b: buyer.map(|b| b.purchasing_company().is_some()).unwrap_or(false),
        addresses: cart
            .delivery_groups()
            .iter()
            .filter_map(|g| g.delivery_address())
            .map(|a| rules::Address {
                first_name: a.first_name().cloned(),
                last_name: a.last_name().cloned(),
                name: a.name().cloned(),
                address1: a.address_1().cloned(),
                zip: a.zip().cloned(),
                country_code: a.country_code().cloned(),
            })
            .collect(),
        subtotal: Some(cart.cost().subtotal_amount().amount().to_string()),
        lines: cart.lines().len(),
        qty: cart.lines().iter().map(|l| *l.quantity() as i64).sum(),
        token: None,
    };
    let mut checkout = checkout;
    if rules::token_mode(&loaded) != rules::TokenMode::Off {
        let lines: Vec<(u64, i64)> = cart
            .lines()
            .iter()
            .filter_map(|l| match l.merchandise() {
                schema::run::input::cart::lines::Merchandise::ProductVariant(v) => {
                    token::variant_number(v.id()).map(|n| (n, *l.quantity() as i64))
                }
                _ => None,
            })
            .collect();
        let local = input.shop().local_time();
        checkout.token = Some(token::check(
            cart.token().and_then(|a| a.value()).map(|s| s.as_str()),
            input.shop().keys().map(|m| m.value().as_str()),
            Some(local.date().as_str()),
            *local.past_fresh(),
            &lines,
        ));
    }

    let decision = rules::decide(&checkout, &loaded);
    if let Some(line) = rules::log_line(&decision, &loaded, &checkout) {
        log!("{}", line);
    }

    match decision {
        rules::Decision::Block { message, .. } => Ok(schema::CartValidationsGenerateRunResult {
            operations: vec![schema::Operation::ValidationAdd(
                schema::ValidationAddOperation {
                    errors: vec![schema::ValidationError {
                        message,
                        target: "$.cart".to_owned(),
                    }],
                },
            )],
        }),
        _ => no_errors(),
    }
}

fn main() {
    log!("Please invoke a named export.");
    process::abort();
}

#[cfg(test)]
mod glue_tests {
    //! End-to-end through the generated Shopify types, using input shaped like the
    //! real dev-store runs (docs/phase-a-evidence/run-A-normal-checkout.md).
    use super::*;
    use shopify_function::run_function_with_input;

    const CONFIG_ENFORCE: &str = r#"{\"mode\":\"enforce\",\"support_phone\":\"(800) 548-6877\",\"blocked_names\":[\"james anderson\"],\"blocked_address1\":[\"428 st\",\"428 w 45th st|10036\",\"230 w 55th st|10019\",\"123 main st|10080\"],\"blocked_zips\":[\"10080\"],\"blocked_email_domains\":[]}"#;

    fn input(step: &str, settings: &str, address1: &str, authed: bool, company: &str) -> String {
        format!(
            r#"{{
  "buyerJourney": {{ "step": "{step}" }},
  "cart": {{
    "cost": {{ "subtotalAmount": {{ "amount": "1.96" }} }},
    "lines": [ {{ "quantity": 1 }} ],
    "buyerIdentity": {{ "email": "someone@gmail.com", "isAuthenticated": {authed}, "purchasingCompany": {company} }},
    "deliveryGroups": [
      {{ "deliveryAddress": {{ "firstName": "Mary", "lastName": "Smith", "name": "Mary Smith", "address1": "{address1}", "zip": "10019", "countryCode": "US" }} }},
      {{ "deliveryAddress": null }}
    ]
  }},
  "shop": {{ "settings": {settings} }}
}}"#
        )
    }

    fn settings(enabled: &str, config: &str) -> String {
        format!(r#"{{ "enabled": {{ "value": "{enabled}" }}, "config": {{ "value": "{config}" }} }}"#)
    }

    fn errors(json: &str) -> Vec<String> {
        let out = run_function_with_input(cart_validations_generate_run, json).unwrap();
        out.operations
            .into_iter()
            .flat_map(|op| match op {
                schema::Operation::ValidationAdd(v) => v.errors.into_iter().map(|e| format!("{}|{}", e.target, e.message)).collect::<Vec<_>>(),
            })
            .collect()
    }

    #[test]
    fn completion_enforce_blocklisted_address_blocks_with_cart_target() {
        let e = errors(&input("CHECKOUT_COMPLETION", &settings("true", CONFIG_ENFORCE), "230 West 55th Street", false, "null"));
        assert_eq!(e.len(), 1);
        assert!(e[0].starts_with("$.cart|We couldn't verify this checkout."), "{:?}", e);
        assert!(e[0].contains("(800) 548-6877"));
    }

    #[test]
    fn interaction_steps_never_block() {
        for step in ["CART_INTERACTION", "CHECKOUT_INTERACTION"] {
            assert!(errors(&input(step, &settings("true", CONFIG_ENFORCE), "428 st", false, "null")).is_empty(), "{}", step);
        }
        // step missing entirely
        let j = input("X", &settings("true", CONFIG_ENFORCE), "428 st", false, "null").replace(r#""step": "X""#, r#""step": null"#);
        assert!(errors(&j).is_empty());
    }

    #[test]
    fn clean_address_passes() {
        assert!(errors(&input("CHECKOUT_COMPLETION", &settings("true", CONFIG_ENFORCE), "12312 W Olympic Blvd", false, "null")).is_empty());
    }

    #[test]
    fn missing_disabled_or_broken_settings_fail_open() {
        assert!(errors(&input("CHECKOUT_COMPLETION", "null", "428 st", false, "null")).is_empty());
        assert!(errors(&input("CHECKOUT_COMPLETION", &settings("false", CONFIG_ENFORCE), "428 st", false, "null")).is_empty());
        assert!(errors(&input("CHECKOUT_COMPLETION", &settings("true", "{not json"), "428 st", false, "null")).is_empty());
        let no_fields = input("CHECKOUT_COMPLETION", r#"{ "enabled": null, "config": null }"#, "428 st", false, "null");
        assert!(errors(&no_fields).is_empty());
    }

    #[test]
    fn b2b_and_logged_in_exempt() {
        assert!(errors(&input("CHECKOUT_COMPLETION", &settings("true", CONFIG_ENFORCE), "428 st", false, r#"{ "company": { "id": "gid://shopify/Company/1" } }"#)).is_empty());
        assert!(errors(&input("CHECKOUT_COMPLETION", &settings("true", CONFIG_ENFORCE), "428 st", true, "null")).is_empty());
    }

    #[test]
    fn log_only_does_not_block() {
        let cfg = CONFIG_ENFORCE.replace(r#"\"enforce\""#, r#"\"log_only\""#);
        assert!(errors(&input("CHECKOUT_COMPLETION", &settings("true", &cfg), "428 st", false, "null")).is_empty());
    }

    // ---- Stage 2: token through the real input shape ----

    const KEY: &str = "51d5e2631986a6e7ddf19c718d6f8eba8ef5c1ab511299424ff77d454fba0c99";
    const HARD: &str = "1.995432.h.fe2646951b5fe6da175a3580ffebdd1f";

    fn token_input(token: &str, authed: bool, keys: &str, cfg_token_mode: &str) -> String {
        token_input_f(token, authed, keys, cfg_token_mode, false)
    }
    fn token_input_f(token: &str, authed: bool, keys: &str, cfg_token_mode: &str, past_fresh: bool) -> String {
        let cfg = format!(r#"{{\"mode\":\"log_only\",\"token_mode\":\"{cfg_token_mode}\",\"support_phone\":\"(800) 548-6877\"}}"#);
        format!(
            r#"{{
  "buyerJourney": {{ "step": "CHECKOUT_COMPLETION" }},
  "cart": {{
    "token": {token},
    "cost": {{ "subtotalAmount": {{ "amount": "21.95" }} }},
    "lines": [
      {{ "quantity": 1, "merchandise": {{ "__typename": "ProductVariant", "id": "gid://shopify/ProductVariant/41234567890" }} }},
      {{ "quantity": 1, "merchandise": {{ "__typename": "ProductVariant", "id": "gid://shopify/ProductVariant/49876543210" }} }},
      {{ "quantity": 1, "merchandise": {{ "__typename": "ProductVariant", "id": "gid://shopify/ProductVariant/41234567890" }} }}
    ],
    "buyerIdentity": {{ "email": "someone@gmail.com", "isAuthenticated": {authed}, "purchasingCompany": null }},
    "deliveryGroups": [ {{ "deliveryAddress": {{ "firstName": "Mary", "lastName": "Smith", "name": "Mary Smith", "address1": "1 Real Rd", "zip": "90210", "countryCode": "US" }} }} ]
  }},
  "shop": {{
    "localTime": {{ "date": "2026-09-30", "pastFresh": {past_fresh} }},
    "keys": {keys},
    "settings": {{ "enabled": {{ "value": "true" }}, "config": {{ "value": "{cfg}" }} }}
  }}
}}"#
        )
    }
    fn keys_json() -> String {
        format!(r#"{{ "value": "{{\"v\":1,\"d\":\"2026-09-30\",\"k\":{{\"995431\":\"{KEY}\",\"995432\":\"{KEY}\",\"995433\":\"{KEY}\"}}}}" }}"#)
    }
    fn attr(v: &str) -> String {
        format!(r#"{{ "value": "{v}" }}"#)
    }

    #[test]
    fn valid_token_on_split_lines_passes() {
        // same variant on two lines is summed: 41234567890:2,49876543210:1
        assert!(errors(&token_input(&attr(HARD), false, &keys_json(), "enforce")).is_empty());
    }

    #[test]
    fn spec_1_2_no_token_blocks_when_enforced() {
        let e = errors(&token_input("null", false, &keys_json(), "enforce"));
        assert_eq!(e.len(), 1);
        assert!(e[0].starts_with("$.cart|We couldn't verify this checkout."));
        // log_only: never blocks
        assert!(errors(&token_input("null", false, &keys_json(), "log_only")).is_empty());
        // off: never looks
        assert!(errors(&token_input("null", false, &keys_json(), "off")).is_empty());
    }

    #[test]
    fn token_for_other_cart_blocks() {
        let copied = attr("1.995432.h.00000000000000000000000000000000");
        assert_eq!(errors(&token_input(&copied, false, &keys_json(), "enforce")).len(), 1);
    }

    #[test]
    fn logged_in_skips_token_and_missing_keys_fail_open() {
        assert!(errors(&token_input("null", true, &keys_json(), "enforce")).is_empty());
        assert!(errors(&token_input("null", false, "null", "enforce")).is_empty());
    }

    #[test]
    fn soft_expired_and_stale_through_input() {
        let soft = attr("1.995432.s.48726d1a28a056f22f361509aff6f5b2");
        // soft token for the same cart content passes (spec 12)
        assert!(errors(&token_input(&soft, false, &keys_json(), "enforce")).is_empty());
        // expired: window no longer published
        let old = attr("1.995400.h.fe2646951b5fe6da175a3580ffebdd1f");
        assert_eq!(errors(&token_input(&old, false, &keys_json(), "enforce")).len(), 1);
        // server stopped publishing: pastFresh true -> let through
        assert!(errors(&token_input_f("null", false, &keys_json(), "enforce", true)).is_empty());
    }

    #[test]
    fn kill_switch_beats_token_enforce() {
        let j = token_input("null", false, &keys_json(), "enforce")
            .replace(r#""enabled": { "value": "true" }"#, r#""enabled": { "value": "false" }"#);
        assert!(errors(&j).is_empty());
    }

    #[test]
    fn blocklist_still_blocks_while_token_keys_are_stale_or_missing() {
        for (keys, fresh) in [(keys_json(), true), ("null".to_string(), false)] {
            let j = token_input_f("null", false, &keys, "enforce", fresh)
                .replace(r#"\"mode\":\"log_only\""#, r#"\"mode\":\"enforce\",\"blocked_address1\":[\"428 st\"]"#)
                .replace(r#""address1": "1 Real Rd""#, r#""address1": "428 st""#);
            assert_eq!(errors(&j).len(), 1, "blocklist must not depend on the token server");
        }
    }
}
