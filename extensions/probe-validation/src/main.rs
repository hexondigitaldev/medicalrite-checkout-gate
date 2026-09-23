//! Phase A probe — Cart & Checkout Validation.
//!
//! NEVER BLOCKS. Returns zero errors on every run. Its only job is to make
//! Shopify record the full function input (visible in the Partner dashboard
//! function run logs) so we can answer the Phase A open questions.
use shopify_function::prelude::*;
use shopify_function::Result;
use std::process;

#[typegen("schema.graphql")]
pub mod schema {
    #[query("src/run.graphql")]
    pub mod run {}
}

#[shopify_function]
fn cart_validations_generate_run(
    input: schema::run::Input,
) -> Result<schema::CartValidationsGenerateRunResult> {
    // A short summary line in the run log makes scanning many runs easier.
    let cart = input.cart();
    log!(
        "probe step={:?} bg={} probe={} lines={} authed={} orders={:?}",
        input.buyer_journey().step(),
        cart.bg().is_some(),
        cart.probe().and_then(|a| a.value().map(|s| s.as_str())).unwrap_or(""),
        cart.lines().len(),
        cart.buyer_identity().map(|b| *b.is_authenticated()).unwrap_or(false),
        cart.buyer_identity()
            .and_then(|b| b.customer())
            .map(|c| *c.number_of_orders()),
    );

    Ok(schema::CartValidationsGenerateRunResult {
        operations: vec![schema::Operation::ValidationAdd(
            schema::ValidationAddOperation { errors: vec![] },
        )],
    })
}

fn main() {
    log!("Please invoke a named export.");
    process::abort();
}
