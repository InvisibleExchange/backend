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
    perpetual::{perp_position::PerpPosition, TOKENS},
    trees::superficial_tree::SuperficialTree,
    utils::notes::Note,
};

use super::tx_batch_structs::{FundingInfo, GlobalConfig, GlobalDexState};

// * HELPERS * //

/// Initialize a map with the default values for all tokens
pub fn _init_empty_tokens_map<T>(map: &mut HashMap<u64, T>)
where
    T: Default,
{
    for token in TOKENS {
        map.insert(token, T::default());
    }
}

// * BATCH FINALIZATION HELPERS ================================================================================

/// Gets the number of updated notes and positions in the batch and how many of them are empty/zero.\
/// This is usefull in the cairo program to know how many slots to allocate for the outputs
///
pub fn get_final_updated_counts(
    updated_note_hashes: &HashMap<u64, BigUint>,
    perpetual_updated_position_hashes: &HashMap<u64, BigUint>,
) -> [u32; 4] {
    let mut num_output_notes: u32 = 0; //= self.updated_note_hashes.len() as u32;
    let mut num_zero_notes: u32 = 0;
    let mut num_output_positions: u32 = 0; // = self.perpetual_updated_position_hashes.len() as u32;
    let mut num_empty_positions: u32 = 0;

    for (_, leaf_hash) in updated_note_hashes.iter() {
        if leaf_hash == &BigUint::zero() {
            num_zero_notes += 1;
        } else {
            num_output_notes += 1;
        }
    }

    for (_, leaf_hash) in perpetual_updated_position_hashes.iter() {
        if leaf_hash == &BigUint::zero() {
            num_empty_positions += 1;
        } else {
            num_output_positions += 1;
        }
    }

    return [
        num_output_notes,
        num_zero_notes,
        num_output_positions,
        num_empty_positions,
    ];
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
    perpetual_preimage: Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let dex_state_json = serde_json::to_value(&global_dex_state).unwrap();
    let global_config_json = serde_json::to_value(&global_config).unwrap();
    let funding_info_json = serde_json::to_value(&funding_info).unwrap();
    let swaps_json = serde_json::to_value(swap_output_json).unwrap();
    let preimage_json = serde_json::to_value(preimage).unwrap();
    let perpetual_preimage_json = serde_json::to_value(perpetual_preimage).unwrap();

    let mut output_json = serde_json::Map::new();
    output_json.insert(String::from("global_dex_state"), dex_state_json);
    output_json.insert(String::from("global_config"), global_config_json);
    output_json.insert(String::from("funding_info"), funding_info_json);
    output_json.insert(String::from("price_info"), price_info_json);
    output_json.insert(String::from("transactions"), swaps_json);
    output_json.insert(String::from("preimage"), preimage_json);
    output_json.insert(String::from("perpetual_preimage"), perpetual_preimage_json);

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
    hashmap: HashMap<u64, BigUint>,
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
                .map(|(key, value)| (key % chunk_size as u64, value.clone()))
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
/// * `updated_note_hashes` - The updated note hashes
/// * `updated_position_hashes` - The updated position hashes
/// * `notes_in` - The notes that are being added to the position
/// * `refund_note` - The refund note (if necessary)
/// * `position_index` - The index of the position
/// * `new_position_hash` - The new position hash
///
pub fn add_margin_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    perp_state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_note_hashes: &Arc<Mutex<HashMap<u64, BigUint>>>,
    updated_position_hashes: &Arc<Mutex<HashMap<u64, BigUint>>>,
    notes_in: &Vec<Note>,
    refund_note: Option<Note>,
    position_index: u64,
    new_position_hash: &BigUint,
) -> std::result::Result<(), String> {
    let mut tree = state_tree.lock();
    let mut updated_note_hashes = updated_note_hashes.lock();

    for note in notes_in.iter() {
        let leaf_hash = tree.get_leaf_by_index(note.index);
        if leaf_hash != note.hash {
            return Err("Note spent does not exist".to_string());
        }
    }

    if let Some(refund_note) = refund_note {
        tree.update_leaf_node(&refund_note.hash, notes_in[0].index);
        updated_note_hashes.insert(notes_in[0].index, refund_note.hash);
    } else {
        tree.update_leaf_node(&BigUint::zero(), notes_in[0].index);
        updated_note_hashes.insert(notes_in[0].index, BigUint::zero());
    }

    for note in notes_in.iter().skip(1) {
        tree.update_leaf_node(&BigUint::zero(), note.index);
        updated_note_hashes.insert(note.index, BigUint::zero());
    }
    drop(tree);
    drop(updated_note_hashes);

    let mut perp_tree = perp_state_tree.lock();
    let mut updated_position_hashes = updated_position_hashes.lock();

    perp_tree.update_leaf_node(&new_position_hash, position_index);
    updated_position_hashes.insert(position_index, new_position_hash.clone());

    drop(perp_tree);
    drop(updated_position_hashes);

    Ok(())
}

/// When removing(withdrawing) margin from a position, we need to update the state
/// by adding the return collateral note hash to the state tree, and updating the position hash
/// in the perp state tree
///
/// # Arguments
/// * `state_tree` - The state tree
/// * `perp_state_tree` - The perp state tree
/// * `updated_note_hashes` - The updated note hashes
/// * `updated_position_hashes` - The updated position hashes
/// * `return_collateral_note` - The return collateral note
/// * `position_index` - The index of the position
/// * `new_position_hash` - The new position hash
///
pub fn reduce_margin_state_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    perp_state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_note_hashes: &Arc<Mutex<HashMap<u64, BigUint>>>,
    updated_position_hashes: &Arc<Mutex<HashMap<u64, BigUint>>>,
    return_collateral_note: Note,
    position_index: u64,
    new_position_hash: &BigUint,
) {
    let mut tree = state_tree.lock();
    let mut updated_note_hashes = updated_note_hashes.lock();

    tree.update_leaf_node(&return_collateral_note.hash, return_collateral_note.index);
    updated_note_hashes.insert(return_collateral_note.index, return_collateral_note.hash);

    drop(tree);
    drop(updated_note_hashes);

    let mut perp_tree = perp_state_tree.lock();
    let mut updated_position_hashes = updated_position_hashes.lock();

    perp_tree.update_leaf_node(&new_position_hash, position_index);
    updated_position_hashes.insert(position_index, new_position_hash.clone());

    drop(perp_tree);
    drop(updated_position_hashes);
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
    let update = deviation * 100_000 / (index_price as i64 * 3); // accourate to 5 decimal places

    return sum + update;
}

/// Calculates the funding rate to apply to all positions
/// It is the twap of the per minute funding updates over the last 8 hours
///
/// # Returns
/// * `HashMap<u64, i64>` - The funding rates for each token
pub fn _calculate_funding_rates(
    running_funding_tick_sums: &mut HashMap<u64, i64>,
) -> HashMap<u64, i64> {
    // Should do once every 8 hours (480 minutes)

    let mut funding_rates: HashMap<u64, i64> = HashMap::new();
    for (token, twap_sum) in running_funding_tick_sums.drain() {
        let funding_rate = twap_sum / 480; // 480 minutes per 8 hours

        funding_rates.insert(token, funding_rate);
    }

    return funding_rates;
}

/// Builds the funding info struct
pub fn get_funding_info(
    min_funding_idxs: &Arc<Mutex<HashMap<u64, u32>>>,
    funding_rates: &HashMap<u64, Vec<i64>>,
    funding_prices: &HashMap<u64, Vec<u64>>,
) -> FundingInfo {
    let min_funding_idxs = min_funding_idxs.lock().clone();
    FundingInfo::new(funding_rates, funding_prices, &min_funding_idxs)
}

//
