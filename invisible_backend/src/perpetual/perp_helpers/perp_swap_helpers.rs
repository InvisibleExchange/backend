use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use num_bigint::BigUint;

use crate::perpetual::perp_order::OpenOrderFields;
use crate::perpetual::perp_position::PerpPosition;
use crate::perpetual::{
    OrderSide, PositionEffectType, COLLATERAL_TOKEN, DECIMALS_PER_ASSET, DUST_AMOUNT_PER_ASSET,
    LEVERAGE_BOUNDS_PER_ASSET, LEVERAGE_DECIMALS, MAX_LEVERAGE, SYNTHETIC_ASSETS,
};
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::errors::{send_perp_swap_error, PerpSwapExecutionError};
use crate::utils::notes::Note;

use crate::utils::crypto_utils::EcPoint;
use error_stack::Result;

use super::super::perp_order::PerpOrder;

// * ==============================================================================
// * HELPER FUNCTIONS * //

/// Constructs the new pfr note
pub fn refund_partial_fill(
    collateral_token: u32,
    blinding: &BigUint,
    pub_key_sum: EcPoint,
    unspent_margin: u64,
    idx: u64,
) -> Option<Note> {
    // let prev_pfr_note_idx = order.partial_refund_note_idx.get();

    let new_partial_refund_note: Note = Note::new(
        idx,
        pub_key_sum,
        collateral_token,
        unspent_margin as u64,
        blinding.clone(),
    );

    return Some(new_partial_refund_note);
}

/// Gets the maximum leverage for a given token and amount
pub fn get_max_leverage(token: u32, amount: u64) -> u64 {
    let [min_bound, max_bound] = LEVERAGE_BOUNDS_PER_ASSET
        .get(token.to_string().as_str())
        .unwrap();

    let token_decimals = DECIMALS_PER_ASSET.get(token.to_string().as_str()).unwrap();
    let decimal_amount: f64 = (amount as f64) / 10_f64.powf(*token_decimals as f64);

    let max_leverage: f64;

    // TODO: FIGURE OUT CORRECT MAX LEVERAGE FOR PEPE

    if decimal_amount < *min_bound as f64 {
        max_leverage = MAX_LEVERAGE;
    } else if decimal_amount < *max_bound as f64 {
        max_leverage = MAX_LEVERAGE * (*min_bound as f64 / decimal_amount);
    } else {
        max_leverage = 1.0;
    }

    return (max_leverage * 10_f64.powf(LEVERAGE_DECIMALS as f64)) as u64;
}

// * ==============================================================================
// * CONSISTENCY CHECKS * //

/// Checks the tokens of all notes are the collateral token being spent \
/// and that the sum of inputs is at least equal to the initial margin + refund amount
pub fn _check_note_sums(
    open_order_fields: &OpenOrderFields,
    order_id: u64,
) -> Result<(), PerpSwapExecutionError> {
    // ? Sum all the notes and check if they all have the same collateral token

    let mut sum_notes: u64 = 0;
    for note in open_order_fields.notes_in.iter() {
        if note.token != open_order_fields.collateral_token {
            return Err(send_perp_swap_error(
                "note and collateral token mismatch".to_string(),
                Some(order_id),
                Some(format!(
                    "token mismatch: note token: {}, collateral token: {}",
                    note.token, open_order_fields.collateral_token
                )),
            ));
        }

        sum_notes += note.amount as u64
    }

    let refund_amount = if open_order_fields.refund_note.is_some() {
        open_order_fields.refund_note.as_ref().unwrap().amount
    } else {
        0
    };
    // ? Check that the sum of notes is at least equal to the initial margin
    if sum_notes != refund_amount + open_order_fields.initial_margin {
        return Err(send_perp_swap_error(
            "sum of inputs does not match amount spent".to_string(),
            Some(order_id),
            Some(format!(
                "note sum: {} < refund amount: {} + initial margin: {}",
                sum_notes, refund_amount, open_order_fields.initial_margin
            )),
        ));
    }

    Ok(())
}

/// Checks that the partial refund info is consistent with the order
pub fn _check_prev_fill_consistencies(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    partial_refund_info: &Option<(Option<Note>, u64, u64)>,
    order: &PerpOrder,
    initial_margin: u64,
) -> Result<Note, PerpSwapExecutionError> {
    // unwrap partial_refund_info or return an error
    if partial_refund_info.is_none() || partial_refund_info.as_ref().unwrap().clone().0.is_none() {
        return Err(send_perp_swap_error(
            "no partial refund info".to_string(),
            None,
            None,
        ));
    }
    let partial_refund_note = partial_refund_info.as_ref().unwrap().clone().0.unwrap();

    // ? Check that the partial refund note has the right token, amount, and address
    if partial_refund_note.token != order.open_order_fields.as_ref().unwrap().collateral_token {
        return Err(send_perp_swap_error(
            "spending wrong token".to_string(),
            None,
            Some(format!(
                "token mismatch: pfr_note token: {}, collateral token: {}",
                partial_refund_note.token,
                order.open_order_fields.as_ref().unwrap().collateral_token
            )),
        ));
    }

    if partial_refund_note.amount < initial_margin as u64 {
        return Err(send_perp_swap_error(
            "refund note amount is to small for this swap".to_string(),
            None,
            Some(format!(
                "refund note amount: {} < initial margin: {}",
                partial_refund_note.amount, initial_margin
            )),
        ));
    }

    // ? Assumption: If you know the sum of private keys you know the individual private keys
    if partial_refund_note.address.x
        != order.open_order_fields.as_ref().unwrap().notes_in[0]
            .address
            .x
    {
        return Err(send_perp_swap_error(
            "pfr note address invalid".to_string(),
            Some(order.order_id),
            None,
        ));
    }

    let state_tree = state_tree.lock();
    let leaf_hash = state_tree.get_leaf_by_index(partial_refund_note.index);
    if leaf_hash != partial_refund_note.hash {
        return Err(send_perp_swap_error(
            "prev partial refund note used in swap does not exist in the state".to_string(),
            None,
            None,
        ));
    }

    return Ok(partial_refund_note.clone());
}

// * ========================================================================================

/// This checks if another swap is already in progress for the same order. \
/// If so, it waits for the other swap to finish and rejects it, if it takes too long.
///
/// ## Returns:
/// * partial_fill_info - the partial fill info (if it's not the first fill)
///
pub fn block_until_prev_fill_finished(
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    blocked_perp_order_ids_m: &Arc<Mutex<HashMap<u64, bool>>>,
    order_id: u64,
) -> Result<Option<(Option<Note>, u64, u64)>, PerpSwapExecutionError> {
    let blocked_perp_order_ids = blocked_perp_order_ids_m.lock();
    let mut is_blocked = blocked_perp_order_ids
        .get(&order_id)
        .unwrap_or(&false)
        .clone();
    drop(blocked_perp_order_ids);

    let mut count = 0;
    while is_blocked {
        if count >= 10 {
            return Err(send_perp_swap_error(
                "previous fill is taking too long".to_string(),
                None,
                None,
            ));
        }

        sleep(Duration::from_millis(10));
        let blocked_perp_order_ids = blocked_perp_order_ids_m.lock();
        is_blocked = blocked_perp_order_ids
            .get(&order_id)
            .unwrap_or(&false)
            .clone();
        drop(blocked_perp_order_ids);

        count += 1;
    }

    let mut blocked_perp_order_ids = blocked_perp_order_ids_m.lock();
    blocked_perp_order_ids.insert(order_id, true);
    drop(blocked_perp_order_ids);

    // ?  Get the partial fill info for this order if it exists (if later fills)
    let mut perpetual_partial_fill_tracker = perpetual_partial_fill_tracker_m.lock();
    let partial_fill_info = perpetual_partial_fill_tracker.remove(&order_id);
    drop(perpetual_partial_fill_tracker);

    return Ok(partial_fill_info);
}

/// This finalizes the updates by inserting the new partial fill info and position into the tracker \
/// and allows the next swap filling the same order to continue executing
///
pub fn finalize_updates(
    order: &PerpOrder,
    perpetual_partial_fill_tracker_m: &Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>,
    partialy_filled_positions_m: &Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>,
    blocked_perp_order_ids_m: &Arc<Mutex<HashMap<u64, bool>>>,
    new_partial_fill_info: &(Option<Note>, u64, u64),
    new_position: &Option<PerpPosition>,
    new_filled_synthetic_amount: u64,
    is_fully_filled: bool,
) {
    // ? If order is partialy filled, we need to update the partial fill tracker
    if order.position_effect_type == PositionEffectType::Open {
        // ?  insert the partial fill info back into the tracker
        if !is_fully_filled {
            let mut perpetual_partial_fill_tracker = perpetual_partial_fill_tracker_m.lock();

            perpetual_partial_fill_tracker.insert(order.order_id, new_partial_fill_info.clone());
            drop(perpetual_partial_fill_tracker);

            // ? Store the partially filled position for the next fill
            let mut partialy_filled_positions = partialy_filled_positions_m.lock();
            partialy_filled_positions.insert(
                new_position
                    .as_ref()
                    .unwrap()
                    .position_header
                    .position_address
                    .to_string(),
                (
                    new_position.as_ref().unwrap().clone(),
                    new_filled_synthetic_amount,
                ),
            );
            drop(partialy_filled_positions);
        }
    } else {
        if new_filled_synthetic_amount
            < order.synthetic_amount - DUST_AMOUNT_PER_ASSET[&order.synthetic_token.to_string()]
            && new_position.is_some()
        {
            // ? Store the partially filled position for the next fill
            let mut partialy_filled_positions = partialy_filled_positions_m.lock();
            partialy_filled_positions.insert(
                new_position
                    .as_ref()
                    .unwrap()
                    .position_header
                    .position_address
                    .to_string(),
                (
                    new_position.as_ref().unwrap().clone(),
                    new_filled_synthetic_amount,
                ),
            );
            drop(partialy_filled_positions);
        }
    }

    // ? allow other threads with this order id to continue
    let mut blocked_perp_order_ids = blocked_perp_order_ids_m.lock();

    blocked_perp_order_ids.remove(&order.order_id);
    drop(blocked_perp_order_ids);
}

// * ========================================================================================

/// ## Checks:
/// * Checks that the order ids are set and different
/// * Checks that the collateral and synthetic tokens are valid and they match
/// * Checks that the order amounts are valid
/// * Checks that the order sides are different
/// * Checks for over spending
/// * Checks that the fees are not to large
/// * Checks that the amounts swapped are consistent with the order amounts
/// * Checks that the notes spent are all unique
/// * Chech that the positions are different
pub fn consistency_checks(
    order_a: &PerpOrder,
    order_b: &PerpOrder,
    spent_collateral: u64,
    spent_synthetic: u64,
    fee_taken_a: u64,
    fee_taken_b: u64,
) -> Result<(), PerpSwapExecutionError> {
    // ? Check that order ids are not 0
    if order_a.order_id == 0 || order_b.order_id == 0 {
        return Err(send_perp_swap_error(
            "Order id should not be 0".to_string(),
            None,
            None,
        ));
    }

    // ? Check that synthetic tokens are valid
    if !SYNTHETIC_ASSETS.contains(&order_a.synthetic_token) {
        return Err(send_perp_swap_error(
            "synthetic token not valid".to_string(),
            Some(order_a.order_id),
            Some(format!(
                "invalid synthetic token {:?}",
                order_a.synthetic_token
            )),
        ));
    } else if !SYNTHETIC_ASSETS.contains(&order_b.synthetic_token) {
        return Err(send_perp_swap_error(
            "synthetic token not valid".to_string(),
            Some(order_b.order_id),
            Some(format!(
                "invalid synthetic token {:?}",
                order_b.synthetic_token
            )),
        ));
    }

    // ! Collateral tokens are verified separately in open and close orders

    // ? Check that the synthetic and collateral tokens are the same for both orders
    if order_a.synthetic_token != order_b.synthetic_token {
        return Err(send_perp_swap_error(
            "synthetic token mismatch".to_string(),
            None,
            Some(format!(
                "synthetic token mismatch {:?} != {:?}",
                order_a.synthetic_token, order_b.synthetic_token
            )),
        ));
    }

    if order_a.position.is_some() {
        if order_a
            .position
            .as_ref()
            .unwrap()
            .position_header
            .synthetic_token
            != order_a.synthetic_token
        {
            return Err(send_perp_swap_error(
                "order and position token mismatch".to_string(),
                Some(order_a.order_id),
                Some(format!(
                    "synthetic token mismatch {:?} != {:?}",
                    order_a.synthetic_token, order_b.synthetic_token
                )),
            ));
        }

        if order_a.position_effect_type == PositionEffectType::Close
            && order_a.position.as_ref().unwrap().vlp_supply > 0
        {
            return Err(send_perp_swap_error(
                "cannot close position with vlp supply".to_string(),
                Some(order_a.order_id),
                Some(format!(
                    "cannot close position with vlp supply: {:?}",
                    order_a.position.as_ref().unwrap().vlp_supply
                )),
            ));
        }
    }

    if order_b.position.is_some() {
        if order_b
            .position
            .as_ref()
            .unwrap()
            .position_header
            .synthetic_token
            != order_b.synthetic_token
        {
            return Err(send_perp_swap_error(
                "order and position token mismatch".to_string(),
                Some(order_b.order_id),
                Some(format!(
                    "synthetic token mismatch {:?} != {:?}",
                    order_b.synthetic_token, order_b.synthetic_token
                )),
            ));
        }

        if order_b.position_effect_type == PositionEffectType::Close
            && order_b.position.as_ref().unwrap().vlp_supply > 0
        {
            return Err(send_perp_swap_error(
                "cannot close position with vlp supply".to_string(),
                Some(order_b.order_id),
                Some(format!(
                    "cannot close position with vlp supply: {:?}",
                    order_b.position.as_ref().unwrap().vlp_supply
                )),
            ));
        }
    }

    // ? Check that the orders are the opposite sides
    // ? for simplicity, we require order_a to be the "buyer" and order_b to be the "seller"
    if order_a.order_side != OrderSide::Long || order_b.order_side != OrderSide::Short {
        if order_a.order_side != OrderSide::Short || order_b.order_side != OrderSide::Long {
            return Err(send_perp_swap_error(
                "order sides are not opposite".to_string(),
                None,
                None,
            ));
        }
    }

    // ? Check that the amounts swapped don't exceed the order amounts
    let synthetic_dust_amount: u64 = DUST_AMOUNT_PER_ASSET[&order_a.synthetic_token.to_string()];
    let collateral_dust_amount: u64 = DUST_AMOUNT_PER_ASSET[&COLLATERAL_TOKEN.to_string()];
    if order_a.order_side == OrderSide::Long {
        if order_a.collateral_amount < spent_collateral - collateral_dust_amount
            || order_b.synthetic_amount < spent_synthetic - synthetic_dust_amount
        {
            return Err(send_perp_swap_error(
                "Amounts swapped exceed order amounts".to_string(),
                None,
                Some(format!(
                    "Amounts swapped exceed order amounts: {} < {} or {} < {}",
                    order_a.collateral_amount,
                    spent_collateral,
                    order_b.synthetic_amount,
                    spent_synthetic
                )),
            ));
        }
    } else {
        if order_b.collateral_amount < spent_collateral
            || order_a.synthetic_amount < spent_synthetic
        {
            return Err(send_perp_swap_error(
                "Amounts swapped exceed order amounts".to_string(),
                None,
                Some(format!(
                    "Amounts swapped exceed order amounts: {} < {} or {} < {}",
                    order_b.collateral_amount,
                    spent_collateral,
                    order_a.synthetic_amount,
                    spent_synthetic
                )),
            ));
        }
    }

    // & If the order is short than more collateral and less synthetic is good (higher price)
    // & If the order is long than more synthetic and less collateral is good (lower price)
    // ? Verify consistency of amounts swapped
    if order_a.order_side == OrderSide::Long {
        // ? Check the prices are consistent to 0.01% (1/10000)
        let a1 = spent_collateral as u128 * order_a.synthetic_amount as u128 * 9999;
        let a2 = spent_synthetic as u128 * order_a.collateral_amount as u128 * 10000;

        let b1 = spent_synthetic as u128 * order_b.collateral_amount as u128 * 10000;
        let b2 = spent_collateral as u128 * order_b.synthetic_amount as u128 * 10001;

        if a1 > a2 || b1 > b2 {
            return Err(send_perp_swap_error(
                "Amount swapped ratios are inconsistent".to_string(),
                None,
                None,
            ));
        }
    } else {
        return Err(send_perp_swap_error(
            "Order a should be Long and order b should be short".to_string(),
            None,
            None,
        ));
    }

    // ? Check that the fees taken don't exceed the order fees
    if ((fee_taken_a as u128 * order_a.collateral_amount as u128)
        > (order_a.fee_limit as u128 * spent_collateral as u128))
        || ((fee_taken_b as u128 * order_b.collateral_amount as u128)
            > (order_b.fee_limit as u128 * spent_collateral as u128))
    {
        return Err(send_perp_swap_error(
            "Fees taken exceed order fees".to_string(),
            None,
            None,
        ));
    }

    // ? Check that the order_ids are different
    if order_a.order_id == order_b.order_id {
        return Err(send_perp_swap_error(
            "Order ids are the same".to_string(),
            None,
            Some(format!("order ids are the same: {:?}", order_a.order_id)),
        ));
    }

    // ? Check that the positions being modified are different (different addresses)
    if order_a.position.is_some() && order_b.position.is_some() {
        let address_a = &order_a
            .position
            .as_ref()
            .unwrap()
            .position_header
            .position_address;
        let address_b = &order_b
            .position
            .as_ref()
            .unwrap()
            .position_header
            .position_address;

        if *address_a == *address_b {
            return Err(send_perp_swap_error(
                "Positions are the same".to_string(),
                None,
                Some(format!("positions are the same: {:?}", address_a)),
            ));
        }
    }

    // ? Check that the notes spent are all different for both orders (different indexes)
    let mut valid = true;
    let mut valid_a = true;
    let mut valid_b = true;

    if order_a.position.is_some() && order_b.position.is_some() {
        let address_a = &order_a
            .position
            .as_ref()
            .unwrap()
            .position_header
            .position_address;
        let address_b = &order_b
            .position
            .as_ref()
            .unwrap()
            .position_header
            .position_address;
        if *address_a == *address_b {
            return Err(send_perp_swap_error(
                "Positions are the same".to_string(),
                None,
                None,
            ));
        }
    }

    let mut spent_indexes_a: Vec<u64> = Vec::new();
    let mut hashes_a: HashMap<u64, BigUint> = HashMap::new();
    if order_a.open_order_fields.is_some() {
        order_a
            .open_order_fields
            .as_ref()
            .unwrap()
            .notes_in
            .iter()
            .for_each(|note| {
                if spent_indexes_a.contains(&note.index) {
                    valid_a = false;
                }
                spent_indexes_a.push(note.index);
                hashes_a.insert(note.index, note.hash.clone());
            });
    }

    let mut spent_indexes_b: Vec<u64> = Vec::new();
    if order_b.open_order_fields.is_some() {
        order_b
            .open_order_fields
            .as_ref()
            .unwrap()
            .notes_in
            .iter()
            .for_each(|note| {
                if spent_indexes_b.contains(&note.index) {
                    valid_b = false;
                }
                spent_indexes_b.push(note.index);

                if spent_indexes_a.contains(&note.index) {
                    if hashes_a.get(&note.index).unwrap() == &note.hash {
                        valid = false;
                    }
                }
            });
    }

    if !valid_a || !valid_b || !valid {
        let invalid_order_id = if !valid_a {
            Some(order_a.order_id)
        } else if !valid_b {
            Some(order_b.order_id)
        } else {
            None
        };

        return Err(send_perp_swap_error(
            "Notes spent are not unique".to_string(),
            invalid_order_id,
            None,
        ));
    }

    Ok(())
}

// * ========================================================================================

pub fn reverify_existances(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    order_a: &PerpOrder,
    prev_pfr_note_a: &Option<Note>,
    order_b: &PerpOrder,
    prev_pfr_note_b: &Option<Note>,
) -> Result<(), PerpSwapExecutionError> {
    let state_tree = state_tree.lock();

    if order_a.position_effect_type == PositionEffectType::Open {
        if let Some(pfr_note) = prev_pfr_note_a {
            let leaf_hash = state_tree.get_leaf_by_index(pfr_note.index);

            if leaf_hash != pfr_note.hash {
                return Err(send_perp_swap_error(
                    "prev partial refund note used in swap does not exist in the state".to_string(),
                    Some(order_a.order_id),
                    None,
                ));
            }
        } else {
            let notes_in = &order_a.open_order_fields.as_ref().unwrap().notes_in;
            for note in notes_in.iter() {
                let leaf_hash = state_tree.get_leaf_by_index(note.index);

                if leaf_hash != note.hash {
                    return Err(send_perp_swap_error(
                        "note spent for swap does not exist in the state".to_string(),
                        Some(order_a.order_id),
                        None,
                    ));
                }
            }
        }
    } else {
        let position = order_a.position.as_ref().unwrap();

        let leaf_hash = state_tree.get_leaf_by_index(position.index as u64);

        if position.hash != leaf_hash {
            return Err(send_perp_swap_error(
                "position to update does not exist in the state".to_string(),
                Some(order_a.order_id),
                None,
            ));
        }
    }

    if order_b.position_effect_type == PositionEffectType::Open {
        if let Some(pfr_note) = prev_pfr_note_b {
            let leaf_hash = state_tree.get_leaf_by_index(pfr_note.index);

            if leaf_hash != pfr_note.hash {
                return Err(send_perp_swap_error(
                    "prev partial refund note used in swap does not exist in the state".to_string(),
                    Some(order_b.order_id),
                    None,
                ));
            }
        } else {
            let notes_in = &order_b.open_order_fields.as_ref().unwrap().notes_in;
            for note in notes_in.iter() {
                let leaf_hash = state_tree.get_leaf_by_index(note.index);

                if leaf_hash != note.hash {
                    return Err(send_perp_swap_error(
                        "note spent for swap does not exist in the state".to_string(),
                        Some(order_b.order_id),
                        None,
                    ));
                }
            }
        }
    } else {
        let position = order_b.position.as_ref().unwrap();

        let leaf_hash = state_tree.get_leaf_by_index(position.index as u64);

        if position.hash != leaf_hash {
            return Err(send_perp_swap_error(
                "position to update does not exist in the state".to_string(),
                Some(order_b.order_id),
                None,
            ));
        }
    }

    return Ok(());
}
