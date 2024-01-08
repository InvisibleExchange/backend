use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::{to_vec, Map, Value};
use std::{collections::HashMap, path::Path, sync::Arc, time::SystemTime};

use error_stack::Result;

use crate::transaction_batch::tx_batch_helpers::store_da_output;
use crate::transaction_batch::TxOutputJson;
use crate::trees::{superficial_tree::SuperficialTree, Tree};
use crate::utils::storage::local_storage::MainStorage;
use crate::{
    transaction_batch::{
        tx_batch_helpers::{get_funding_info, split_hashmap},
        tx_batch_structs::{get_price_info, GlobalConfig},
        LeafNodeType,
    },
    utils::storage::firestore::upload_file_to_storage,
};

use crate::utils::errors::BatchFinalizationError;

use crate::transaction_batch::{
    tx_batch_helpers::{_init_empty_tokens_map, get_final_updated_counts, get_json_output},
    tx_batch_structs::{FundingInfo, GlobalDexState, OracleUpdate},
};

pub const TREE_DEPTH: u32 = 32;
const PARTITION_SIZE_EXPONENT: u32 = 12;

//

#[derive(Debug, Clone)]
pub struct BatchTransitionInfo {
    pub current_batch_index: u32,
    pub funding_info: FundingInfo,
    pub price_info_json: Value,
    pub data_commitment: BigUint,
    pub updated_state_hashes: HashMap<u64, (LeafNodeType, BigUint)>,
    pub exchange_state_storage: Map<String, Value>,
}

/// Gets all the relevant info for this batch and stores it in a struct
/// to be used by _transition_state. It also resets all the relevant state
/// variables so that the next batch can begin.
pub fn _finalize_batch_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    transaction_output_json: &Arc<Mutex<TxOutputJson>>,
    main_storage: &Arc<Mutex<MainStorage>>,
    insurance_fund: &Arc<Mutex<i64>>,
    funding_rates: &mut HashMap<u32, Vec<i64>>,
    funding_prices: &mut HashMap<u32, Vec<u64>>,
    min_funding_idxs: &Arc<Mutex<HashMap<u32, u32>>>,
    min_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
    max_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
) -> BatchTransitionInfo {
    let state_tree = state_tree.clone();
    let mut state_tree = state_tree.lock();
    state_tree.update_zero_idxs();

    println!("1");

    let main_storage = main_storage.clone();
    let mut main_storage = main_storage.lock();
    let latest_output_json = transaction_output_json.clone();
    let latest_output_json = latest_output_json.lock();

    let current_batch_index = main_storage.latest_batch;

    println!("2");

    // ? Store the latest output json
    main_storage.store_micro_batch(&latest_output_json.tx_micro_batch);
    main_storage.store_state_updates(&latest_output_json.state_updates);

    println!("3");

    let min_funding_idxs = &min_funding_idxs;
    let funding_rates = &funding_rates;
    let funding_prices = &funding_prices;
    let min_index_price_data_ = &min_index_price_data;
    let max_index_price_data_ = &max_index_price_data;

    let mut updated_state_hashes_c = updated_state_hashes.lock();
    let updated_state_hashes: HashMap<u64, (LeafNodeType, BigUint)> =
        updated_state_hashes_c.clone();

    // ?  Get the funding info
    let funding_info: FundingInfo =
        get_funding_info(min_funding_idxs, funding_rates, funding_prices);

    // ? Get the price info
    let price_info_json = get_price_info(min_index_price_data_, max_index_price_data_);

    println!("4");

    // ? Build and store the DA output (notes/positions/tabs/zero_indexes) to be posted to Celestia/EigenDA
    let data_commitment =
        store_da_output(&main_storage, &updated_state_hashes_c, current_batch_index);

    println!("5");

    updated_state_hashes_c.clear();

    // ? Drop the locks before updating the trees
    drop(state_tree);
    drop(main_storage);
    drop(updated_state_hashes_c);

    // ? Reset the batch
    reset_batch(min_funding_idxs, min_index_price_data, max_index_price_data);

    let mut exchange_state_storage = serde_json::Map::new(); // This should be stored in the database
    let insurance_fund_m = insurance_fund.lock();
    let insurance_fund_value = insurance_fund_m.clone();
    drop(insurance_fund_m);

    println!("6");

    exchange_state_storage.insert(
        String::from("funding_rates"),
        serde_json::to_value(&funding_rates).unwrap_or_default(),
    );
    exchange_state_storage.insert(
        String::from("funding_prices"),
        serde_json::to_value(&funding_prices).unwrap_or_default(),
    );
    exchange_state_storage.insert(
        String::from("insurance_fund"),
        serde_json::to_value(insurance_fund_value).unwrap_or_default(),
    );
    exchange_state_storage.insert(
        String::from("min_index_price_data"),
        serde_json::to_value(&min_index_price_data).unwrap_or_default(),
    );
    exchange_state_storage.insert(
        String::from("max_index_price_data"),
        serde_json::to_value(&max_index_price_data).unwrap_or_default(),
    );
    let rough_timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as u32;
    exchange_state_storage.insert(
        String::from("rough_timestamp"),
        serde_json::to_value(&rough_timestamp).unwrap_or_default(),
    );

    BatchTransitionInfo {
        current_batch_index,
        funding_info,
        price_info_json,
        data_commitment,
        updated_state_hashes,
        exchange_state_storage,
    }
}

/// This function updates the merkle trees and stores them to disk.
/// It also creates the json cairo program input for the prover.
pub fn _transition_state(
    main_storage_m: &Arc<Mutex<MainStorage>>,
    batch_transition_info: BatchTransitionInfo,
) -> Result<(), BatchFinalizationError> {
    // ? Get the json output of all the transactions
    let main_storage = main_storage_m.lock();
    let transaction_output_json = main_storage.read_storage(0);
    drop(main_storage);

    println!("7");

    // ? Get the final updated counts for the cairo program input
    let program_input_counts = get_final_updated_counts(
        &batch_transition_info.updated_state_hashes,
        &transaction_output_json,
    );

    println!("8");

    // ? Update the merkle trees and get the new roots and preimages
    let (prev_spot_root, new_spot_root, preimage_json) =
        update_trees(batch_transition_info.updated_state_hashes)?;

    println!("9");

    // ? Construct the global state and config
    let global_expiration_timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as u32;
    let global_dex_state: GlobalDexState = GlobalDexState::new(
        batch_transition_info.current_batch_index,
        &prev_spot_root,
        &new_spot_root,
        TREE_DEPTH,
        global_expiration_timestamp,
        program_input_counts,
    );

    println!("10");

    let global_config: GlobalConfig = GlobalConfig::new();

    let output_json: Map<String, Value> = get_json_output(
        &global_dex_state,
        &global_config,
        &batch_transition_info.data_commitment,
        &batch_transition_info.funding_info,
        batch_transition_info.price_info_json,
        &transaction_output_json,
        preimage_json,
    );

    println!("11");

    // & Write transaction batch json to database

    // Todo: This is for testing only ----------------------------
    let path =
        Path::new("../../prover_contracts/cairo_contracts/transaction_batch/tx_batch_input.json");
    std::fs::write(path, serde_json::to_string(&output_json).unwrap()).unwrap();
    // Todo: This is for testing only ----------------------------

    let mut main_storage = main_storage_m.lock();
    let future = main_storage.transition_to_new_batch();
    drop(main_storage);

    let _handle = tokio::spawn(async move {
        match future {
            None => {
                return;
            }
            Some(future) => {
                if let Err(e) = future.await {
                    println!("Error storing pending txs: {:?}", e);
                }
            }
        }

        // ? Store the transactions
        let serialized_data = to_vec(&output_json).expect("Serialization failed");
        if let Err(e) = upload_file_to_storage(
            "tx_batches/".to_string() + &batch_transition_info.current_batch_index.to_string(),
            serialized_data,
        )
        .await
        {
            println!("Error uploading file to storage: {:?}", e);
        }

        // ? store other relevant state info
        let serialized_data =
            to_vec(&batch_transition_info.exchange_state_storage).expect("Serialization failed");
        if let Err(e) = upload_file_to_storage(
            "tx_batches/".to_string()
                + &batch_transition_info.current_batch_index.to_string()
                + "_state_info",
            serialized_data,
        )
        .await
        {
            println!("Error uploading file to storage: {:?}", e);
        }
    });

    println!("Transaction batch finalized successfully!");

    return Ok(());
}

// * ======================================================================================

pub fn update_trees(
    updated_state_hashes: HashMap<u64, (LeafNodeType, BigUint)>,
) -> Result<(BigUint, BigUint, Map<String, Value>), BatchFinalizationError> {
    // * UPDATE SPOT TREES  -------------------------------------------------------------------------------------
    let mut updated_root_hashes: HashMap<u64, BigUint> = HashMap::new(); // the new roots of all tree partitions

    let mut preimage_json: Map<String, Value> = Map::new();

    let partitioned_hashes = split_hashmap(
        updated_state_hashes,
        2_usize.pow(PARTITION_SIZE_EXPONENT) as usize,
    );

    // ? Loop over all partitions and update the trees
    for (partition_index, partition) in partitioned_hashes {
        if partition.is_empty() {
            continue;
        }

        let (_, new_root) =
            tree_partition_update(partition, &mut preimage_json, partition_index as u32)?;

        updated_root_hashes.insert(partition_index as u64, new_root);
    }

    // ? use the newly generated roots to update the state tree
    let (prev_spot_root, new_spot_root) =
        tree_partition_update(updated_root_hashes, &mut preimage_json, u32::MAX)?;

    Ok((prev_spot_root, new_spot_root, preimage_json))
}

fn tree_partition_update(
    updated_state_hashes: HashMap<u64, BigUint>,
    preimage_json: &mut Map<String, Value>,
    tree_index: u32,
) -> Result<(BigUint, BigUint), BatchFinalizationError> {
    let shift = if tree_index == u32::MAX {
        PARTITION_SIZE_EXPONENT
    } else {
        0
    };
    let depth = if tree_index == u32::MAX {
        TREE_DEPTH - PARTITION_SIZE_EXPONENT
    } else {
        PARTITION_SIZE_EXPONENT
    };

    let mut batch_init_tree =
        Tree::from_disk(tree_index, depth, shift).map_err(|_| BatchFinalizationError {})?;

    let prev_root = batch_init_tree.root.clone();

    // ? Store the current tree to disk as a backup
    batch_init_tree
        .store_to_disk(tree_index, true)
        .map_err(|e| {
            println!("Error storing backup tree to disk: {:?}", e);
            BatchFinalizationError {}
        })?;

    batch_init_tree.batch_transition_updates(&updated_state_hashes, preimage_json);

    let new_root = batch_init_tree.root.clone();

    // ? Store the new tree to disk
    batch_init_tree
        .store_to_disk(tree_index, false)
        .map_err(|e| {
            println!("Error storing updated tree to disk: {:?}", e);
            BatchFinalizationError {}
        })?;

    Ok((prev_root, new_root))
}

// * RESET * //
fn reset_batch(
    min_funding_idxs: &Arc<Mutex<HashMap<u32, u32>>>,
    min_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
    max_index_price_data: &mut HashMap<u32, (u64, OracleUpdate)>,
) {
    _init_empty_tokens_map::<(u64, OracleUpdate)>(min_index_price_data);
    _init_empty_tokens_map::<(u64, OracleUpdate)>(max_index_price_data);
    // ? Funding is seperate from batch execution so it is not reset
    // ? min_funding_idxs is the exception since it's reletive to the batch
    let mut min_funding_idxs = min_funding_idxs.lock();
    min_funding_idxs.clear();
    _init_empty_tokens_map::<u32>(&mut min_funding_idxs);
}

//
