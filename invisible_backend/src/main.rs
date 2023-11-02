use std::{collections::HashMap, path::Path, str::FromStr};

use invisible_backend::utils::{
    cairo_output::{format_cairo_ouput, parse_cairo_output, preprocess_cairo_output},
    crypto_utils::{pedersen, pedersen_on_vec},
    storage::local_storage::MainStorage,
};

use invisible_backend::trees::Tree;

use num_bigint::BigUint;
use num_traits::{FromPrimitive, One, Zero};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let mut tree = Tree::new(32, 0);

    // let mut updated_hashes = HashMap::new();
    // for i in (0..10).into_iter().step_by(4) {
    //     updated_hashes.insert(i, BigUint::from_u64(i).unwrap());
    // }

    // let mut preimage = serde_json::Map::new();

    // let now = std::time::Instant::now();
    // tree.batch_transition_updates(&updated_hashes, &mut preimage);

    // // let x = updated_hashes.get(&0).unwrap().to_string();

    // println!("time to insert: {:?}", now.elapsed());

    // // println!("root: {:?}", tree.root);

    let path = Path::new("../../prover_contracts/cairo_contracts/transaction_batch/test123.json");
    std::fs::write(
        path,
        serde_json::to_string(&json!({"hello": "world"})).unwrap(),
    )
    .unwrap();

    Ok(())
}
