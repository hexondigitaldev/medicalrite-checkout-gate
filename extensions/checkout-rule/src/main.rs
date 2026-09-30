//! Stage 1 checkout rule (Cart & Checkout Validation). Logic lives in rules.rs.
use shopify_function::prelude::*;
use shopify_function::Result;
use std::process;

pub mod rules;

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
        total: Some(cart.cost().total_amount().amount().to_string()),
        lines: cart.lines().len(),
        qty: cart.lines().iter().map(|l| *l.quantity() as i64).sum(),
    };

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
    "cost": {{ "totalAmount": {{ "amount": "11.91" }} }},
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
}
