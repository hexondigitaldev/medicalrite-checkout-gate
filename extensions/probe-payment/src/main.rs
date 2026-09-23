//! Phase A probe — Payment Customization.
//!
//! By default HIDES NOTHING (safe to install on the live store for Q1).
//! On the dev store only, you may set the payment customization metafield
//! `$app:bot-gate-probe` / `config` to e.g.
//!   {"hideNames": ["(for testing) Bogus Gateway"], "placements": ["PAYMENT_METHOD"]}
//! to prove a named method can be hidden. Names match case-insensitively.
use shopify_function::prelude::*;
use shopify_function::Result;
use std::process;

#[derive(Deserialize, Default, PartialEq, Debug, Clone)]
#[shopify_function(rename_all = "camelCase")]
pub struct Configuration {
    #[shopify_function(default)]
    pub hide_names: Vec<String>,
    /// "PAYMENT_METHOD" and/or "ACCELERATED_CHECKOUT". Empty = all placements.
    #[shopify_function(default)]
    pub placements: Vec<String>,
}

#[typegen("schema.graphql")]
pub mod schema {
    #[query("src/run.graphql", custom_scalar_overrides = {
        "Input.paymentCustomization.metafield.jsonValue" => super::Configuration,
    })]
    pub mod run {}
}

fn placement(p: &str) -> Option<schema::PaymentCustomizationPaymentMethodPlacement> {
    match p {
        "PAYMENT_METHOD" => Some(schema::PaymentCustomizationPaymentMethodPlacement::PaymentMethod),
        "ACCELERATED_CHECKOUT" => {
            Some(schema::PaymentCustomizationPaymentMethodPlacement::AcceleratedCheckout)
        }
        _ => None,
    }
}

#[shopify_function]
fn cart_payment_methods_transform_run(
    input: schema::run::Input,
) -> Result<schema::CartPaymentMethodsTransformRunResult> {
    for m in input.payment_methods() {
        log!("method name={:?} id={} placements={:?}", m.name(), m.id(), m.placements());
    }

    let config = input
        .payment_customization()
        .metafield()
        .map(|mf| mf.json_value())
        .cloned()
        .unwrap_or_default();

    let placements: Vec<_> = config.placements.iter().filter_map(|p| placement(p)).collect();
    let wanted: Vec<String> = config.hide_names.iter().map(|n| n.to_lowercase()).collect();

    let operations = input
        .payment_methods()
        .iter()
        .filter(|m| wanted.contains(&m.name().to_lowercase()))
        .map(|m| {
            log!("probe hiding {:?}", m.name());
            schema::Operation::PaymentMethodHide(schema::PaymentMethodHideOperation {
                payment_method_id: m.id().clone(),
                placements: if placements.is_empty() { None } else { Some(placements.clone()) },
            })
        })
        .collect();

    Ok(schema::CartPaymentMethodsTransformRunResult { operations })
}

fn main() {
    log!("Please invoke a named export.");
    process::abort();
}
