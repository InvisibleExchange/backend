use num_bigint::BigUint;
use num_traits::Zero;
use parking_lot::Mutex;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use serde_json::{Map, Value};
use std::{
    cmp::max,
    collections::HashMap,
    error::Error,
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::Arc,
};

use crate::{
    perpetual::{perp_position::PerpPosition, SYNTHETIC_ASSETS},
    trees::superficial_tree::SuperficialTree,
    utils::notes::Note,
};

use super::{
    tx_batch_structs::{FundingInfo, GlobalConfig, GlobalDexState, ProgramInputCounts},
    LeafNodeType,
};

// * HELPERS * //

/// Initialize a map with the default values for all tokens
pub fn _init_empty_tokens_map<T>(map: &mut HashMap<u32, T>)
where
    T: Default,
{
    for token in SYNTHETIC_ASSETS {
        map.insert(token, T::default());
    }
}

// * BATCH FINALIZATION HELPERS ================================================================================

/// Gets the number of updated notes and positions in the batch and how many of them are empty/zero.\
/// This is usefull in the cairo program to know how many slots to allocate for the outputs
///
pub fn get_final_updated_counts(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    swap_output_json: &Vec<Map<String, Value>>,
) -> ProgramInputCounts {
    let mut n_output_notes: u32 = 0; //= self.updated_state_hashes.len() as u32;
    let mut n_output_positions: u16 = 0; // = self.perpetual_updated_position_hashes.len() as u32;
    let mut n_output_tabs: u16 = 0;
    let mut n_zero_indexes: u32 = 0;

    for (_, (leaf_type, leaf_hash)) in updated_state_hashes.iter() {
        if leaf_hash == &BigUint::zero() {
            n_zero_indexes += 1;
        } else {
            match leaf_type {
                LeafNodeType::Note => {
                    n_output_notes += 1;
                }
                LeafNodeType::Position => {
                    n_output_positions += 1;
                }
                LeafNodeType::OrderTab => {
                    n_output_tabs += 1;
                }
            }
        }
    }

    let mut n_deposits: u16 = 0;
    let mut n_withdrawals: u16 = 0;
    let mut n_onchain_mm_actions: u16 = 0;
    let mut n_note_escapes: u16 = 0;
    let mut n_position_escapes: u16 = 0;
    let mut n_tab_escapes: u16 = 0;

    for transaction in swap_output_json {
        let transaction_type = transaction
            .get("transaction_type")
            .unwrap()
            .as_str()
            .unwrap();

        match transaction_type {
            "deposit" => {
                n_deposits += 1;
            }
            "withdrawal" => {
                n_withdrawals += 1;
            }
            "onchain_mm_action" => {
                n_onchain_mm_actions += 1;
            }
            "forced_escape" => match transaction.get("escape_type").unwrap().as_str().unwrap() {
                "note_escape" => {
                    n_note_escapes += 1;
                }
                "order_tab_escape" => {
                    n_tab_escapes += 1;
                }
                "position_escape" => {
                    n_position_escapes += 1;
                }
                _ => {
                    continue;
                }
            },
            _ => {
                continue;
            }
        }
    }

    ProgramInputCounts {
        n_output_notes,
        n_output_positions,
        n_output_tabs,
        n_zero_indexes,
        n_deposits,
        n_withdrawals,
        n_onchain_mm_actions,
        n_note_escapes,
        n_position_escapes,
        n_tab_escapes,
    }
}
//

/// Gets all the necessary information and generates the output json map that will
/// be used as the input to the cairo program, helping prove the entire batch
///
pub fn get_json_output(
    global_dex_state: &GlobalDexState,
    global_config: &GlobalConfig,
    funding_info: &FundingInfo,
    price_info_json: Value,
    swap_output_json: &Vec<Map<String, Value>>,
    preimage: Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let dex_state_json = serde_json::to_value(&global_dex_state).unwrap();
    let global_config_json = serde_json::to_value(&global_config).unwrap();
    let funding_info_json = serde_json::to_value(&funding_info).unwrap();
    let swaps_json = serde_json::to_value(swap_output_json).unwrap();
    let preimage_json = serde_json::to_value(preimage).unwrap();

    let mut output_json = serde_json::Map::new();
    output_json.insert(String::from("global_dex_state"), dex_state_json);
    output_json.insert(String::from("global_config"), global_config_json);
    output_json.insert(String::from("funding_info"), funding_info_json);
    output_json.insert(String::from("price_info"), price_info_json);
    output_json.insert(String::from("transactions"), swaps_json);
    output_json.insert(String::from("preimage"), preimage_json);

    return output_json;
}

pub fn store_snapshot_data(
    partial_fill_tracker: &HashMap<u64, (Note, u64)>,
    perpetual_partial_fill_tracker: &HashMap<u64, (Option<Note>, u64, u64)>,
    partialy_opened_positions: &HashMap<String, (PerpPosition, u64)>,
    funding_rates: &HashMap<u64, Vec<i64>>,
    funding_prices: &HashMap<u64, Vec<u64>>,
    current_funding_idx: u32,
) -> std::result::Result<(), Box<dyn Error>> {
    let path = Path::new("storage/batch_snapshot");

    let mut file: File = File::create(path)?;

    let encoded: Vec<u8> = bincode::serialize(&(
        partial_fill_tracker,
        perpetual_partial_fill_tracker,
        partialy_opened_positions,
        funding_rates,
        funding_prices,
        current_funding_idx,
    ))
    .unwrap();

    file.write_all(&encoded[..])?;

    Ok(())
}

pub fn fetch_snapshot_data() -> std::result::Result<
    (
        HashMap<u64, (Note, u64)>,
        HashMap<u64, (Option<Note>, u64, u64)>,
        HashMap<String, (PerpPosition, u64)>,
        HashMap<u64, Vec<i64>>,
        HashMap<u64, Vec<u64>>,
        u32,
    ),
    Box<dyn Error>,
> {
    let path = Path::new("storage/batch_snapshot");

    let mut file: File = File::open(path)?;

    let mut encoded: Vec<u8> = Vec::new();

    file.read_to_end(&mut encoded)?;

    let decoded: (
        HashMap<u64, (Note, u64)>,
        HashMap<u64, (Option<Note>, u64, u64)>,
        HashMap<String, (PerpPosition, u64)>,
        HashMap<u64, Vec<i64>>,
        HashMap<u64, Vec<u64>>,
        u32,
    ) = bincode::deserialize(&encoded[..]).unwrap();

    Ok(decoded)
}

pub fn split_hashmap(
    hashmap: HashMap<u64, (LeafNodeType, BigUint)>,
    chunk_size: usize,
) -> Vec<(usize, HashMap<u64, BigUint>)> {
    let max_key = *hashmap.keys().max().unwrap_or(&0);
    let num_submaps = (max_key as usize + chunk_size) / chunk_size;

    let submaps: Vec<(usize, HashMap<u64, BigUint>)> = (0..num_submaps)
        .into_par_iter()
        .map(|submap_index| {
            let submap: HashMap<u64, BigUint> = hashmap
                .iter()
                .filter(|(key, _)| {
                    let submap_start = if submap_index == 0 {
                        0
                    } else {
                        submap_index * chunk_size
                    };
                    let submap_end = (submap_index + 1) * chunk_size;
                    **key >= submap_start as u64 && **key < submap_end as u64
                })
                .map(|(key, (_type, value))| (key % chunk_size as u64, value.clone()))
                .collect();

            (submap_index, submap)
        })
        .collect();

    submaps
}

// * CHANGE MARGIN ================================================================================

/// When adding extra margin to a position (to prevent liquidation), we need to update the state
/// by removing the old note hashes from the state tree, adding the refund note hash(if necessary) and
/// updating the position hash in the perp state tree
///
/// # Arguments
/// * `state_tree` - The state tree
/// * `perp_state_tree` - The perp state tree
/// * `updated_state_hashes` - The updated note hashes
/// * `updated_position_hashes` - The updated position hashes
/// * `notes_in` - The notes that are being added to the position
/// * `refund_note` - The refund note (if necessary)
/// * `position_index` - The index of the position
/// * `new_position_hash` - The new position hash
///
pub fn add_margin_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    notes_in: &Vec<Note>,
    refund_note: Option<Note>,
    position_index: u64,
    new_position_hash: &BigUint,
) -> std::result::Result<(), String> {
    let mut tree = state_tree.lock();
    let mut updated_state_hashes = updated_state_hashes.lock();

    for note in notes_in.iter() {
        let leaf_hash = tree.get_leaf_by_index(note.index);
        if leaf_hash != note.hash {
            return Err("Note spent does not exist".to_string());
        }
    }

    if let Some(refund_note) = refund_note {
        tree.update_leaf_node(&refund_note.hash, notes_in[0].index);
        updated_state_hashes.insert(notes_in[0].index, (LeafNodeType::Note, refund_note.hash));
    } else {
        tree.update_leaf_node(&BigUint::zero(), notes_in[0].index);
        updated_state_hashes.insert(notes_in[0].index, (LeafNodeType::Note, BigUint::zero()));
    }

    for note in notes_in.iter().skip(1) {
        tree.update_leaf_node(&BigUint::zero(), note.index);
        updated_state_hashes.insert(note.index, (LeafNodeType::Note, BigUint::zero()));
    }

    tree.update_leaf_node(&new_position_hash, position_index);
    updated_state_hashes.insert(
        position_index,
        (LeafNodeType::Position, new_position_hash.clone()),
    );

    drop(tree);
    drop(updated_state_hashes);

    Ok(())
}

/// When removing(withdrawing) margin from a position, we need to update the state
/// by adding the return collateral note hash to the state tree, and updating the position hash
/// in the perp state tree
///
/// # Arguments
/// * `state_tree` - The state tree
/// * `perp_state_tree` - The perp state tree
/// * `updated_state_hashes` - The updated note hashes
/// * `updated_position_hashes` - The updated position hashes
/// * `return_collateral_note` - The return collateral note
/// * `position_index` - The index of the position
/// * `new_position_hash` - The new position hash
///
pub fn reduce_margin_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    return_collateral_note: Note,
    position_index: u64,
    new_position_hash: &BigUint,
) {
    let mut tree = state_tree.lock();
    let mut updated_state_hashes = updated_state_hashes.lock();

    tree.update_leaf_node(&return_collateral_note.hash, return_collateral_note.index);
    updated_state_hashes.insert(
        return_collateral_note.index,
        (LeafNodeType::Note, return_collateral_note.hash),
    );

    tree.update_leaf_node(&new_position_hash, position_index);
    updated_state_hashes.insert(
        position_index,
        (LeafNodeType::Position, new_position_hash.clone()),
    );

    drop(tree);
    drop(updated_state_hashes);
}

// * FUNDING FUNCTIONS ================================================================================

/// Calculates the per minute funding update
///
/// If index price is below market price (bid), then funding is positive and longs pay shorts\
/// If index price is above market price (ask), then funding is negative and shorts pay longs
///
///  # Arguments
/// * `impact_bid` - The impact bid price (from the orderbook)
/// * `impact_ask` - The impact ask price (from the orderbook)
/// * `sum` - The current sum of the per minute funding updates
/// * `index_price` - The index price (from the oracle)
///
///

///
/// # Returns
/// * `i64` - The new per minute funding update sum
pub fn _per_minute_funding_update_inner(
    impact_bid: u64,
    impact_ask: u64,
    sum: i64,
    index_price: u64,
) -> i64 {
    //& (Max(0, Impact Bid Price - Index Price) - Max(0, Index Price - Impact Ask Price))

    let deviation: i64 = max(0, impact_bid as i64 - index_price as i64) as i64
        - max(0, index_price as i64 - impact_ask as i64) as i64;
    let update = deviation * 100_000 / (index_price as i64); // accourate to 5 decimal places

    return sum + update;
}

/// Calculates the funding rate to apply to all positions
/// It is the twap of the per minute funding updates over the last 8 hours
///
/// # Returns
/// * `HashMap<u64, i64>` - The funding rates for each token
pub fn _calculate_funding_rates(
    running_funding_tick_sums: &mut HashMap<u32, i64>,
) -> HashMap<u32, i64> {
    // Should do once every hour (60 minutes)

    let mut funding_rates: HashMap<u32, i64> = HashMap::new();

    for t in SYNTHETIC_ASSETS {
        let twap_sum = running_funding_tick_sums.remove(&t).unwrap_or(0);

        let funding_premium = twap_sum / 60; // divide by 60 to get the average funding premium
        funding_rates.insert(t, funding_premium / 8); // scale to a realization period of 8 hours
    }

    return funding_rates;
}

/// Builds the funding info struct
pub fn get_funding_info(
    min_funding_idxs: &Arc<Mutex<HashMap<u32, u32>>>,
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
) -> FundingInfo {
    let min_funding_idxs = min_funding_idxs.lock().clone();
    FundingInfo::new(funding_rates, funding_prices, &min_funding_idxs)
}

//
