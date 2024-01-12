use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use num_traits::{FromPrimitive, Num, One, Zero};
use parking_lot::Mutex;
use starknet::curve::AffinePoint;
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::perpetual::OrderSide;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::keccak256;
use crate::utils::storage::backup_storage::BackupStorage;
use crate::{
    perpetual::{
        get_collateral_amount, perp_helpers::perp_swap_helpers::_check_note_sums,
        perp_order::OpenOrderFields, perp_position::PerpPosition, COLLATERAL_TOKEN,
        LEVERAGE_DECIMALS, SYNTHETIC_ASSETS,
    },
    transaction_batch::{tx_batch_structs::SwapFundingInfo, LeafNodeType},
    utils::{
        crypto_utils::{verify, EcPoint, Signature},
        storage::firestore::{
            start_add_note_thread, start_add_position_thread, start_delete_note_thread,
            start_delete_position_thread,
        },
    },
};

use crate::utils::notes::Note;

use serde::Serialize;

use super::note_escapes::{find_invalid_note, hash_note_keccak};

#[derive(Serialize)]
pub struct PositionEscape {
    escape_id: u32,
    is_valid_a: bool,
    position_a: PerpPosition,
    valid_leaf_a: String, // valid leaf - if position does not exist, this is the leaf that was found
    close_price: u64,
    is_valid_b: bool,
    open_order_fields_b: Option<OpenOrderFields>,
    invalid_note: Option<(u64, String)>, // (idx, leaf) of one invalid note (if any)
    is_position_valid_b: bool,
    position_b: Option<PerpPosition>,
    valid_leaf_b: String, // valid leaf - if position does not exist, this is the leaf that was found
    recipient: String,
    signature_a: Signature,
    signature_b: Signature,
    new_funding_idx: u32,
    index_price: u64,
}

pub fn verify_position_escape(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    escape_id: u32,
    position_a: PerpPosition,
    close_price: u64,
    open_order_fields_b: Option<OpenOrderFields>,
    position_b: Option<PerpPosition>,
    recipient: String,
    signature_a: Signature,
    signature_b: Signature,
    swap_funding_info: &SwapFundingInfo,
    index_price: u64,
) -> (PositionEscape, Option<PerpPosition>) {
    let mut position_escape = PositionEscape {
        escape_id,
        is_valid_a: true,
        valid_leaf_a: "".to_string(),
        position_a: position_a.clone(),
        close_price,
        is_valid_b: true,
        open_order_fields_b: open_order_fields_b.clone(),
        invalid_note: None,
        is_position_valid_b: true,
        position_b: position_b.clone(),
        valid_leaf_b: "".to_string(),
        recipient: recipient.clone(),
        signature_a: signature_a.clone(),
        signature_b: signature_b.clone(),
        new_funding_idx: swap_funding_info.current_funding_idx,
        index_price,
    };

    // ? Verify the signatures
    let order_hash = hash_position_escape_message(
        &position_a,
        close_price,
        &open_order_fields_b,
        &position_b,
        recipient,
    );
    if !verify_signatures(
        &position_a,
        &open_order_fields_b,
        &position_b,
        signature_a,
        signature_b,
        order_hash,
    ) {
        return (position_escape, None);
    }

    // * Order_a ---------------------------------------------------------------
    // ? Verify position synthetic token is valid
    if !SYNTHETIC_ASSETS.contains(&position_a.position_header.synthetic_token) {
        return (position_escape, None);
    }

    // ? Verify position exists
    let (position_exists, leaf_node) = verify_position_exists(state_tree, &position_a);
    if !position_exists {
        position_escape.is_valid_a = false;
        position_escape.valid_leaf_a = leaf_node;
        return (position_escape, None);
    }

    // ? Verify position is not liquidatable
    if position_a
        .is_position_liquidatable(close_price, index_price)
        .0
    {
        return (position_escape, None);
    };

    // * Order_b ---------------------------------------------------------------

    let notes_in: Option<Vec<Note>>;
    let refund_note: Option<Note>;
    let new_position_b: PerpPosition;
    if let Some(open_order_fields_b) = open_order_fields_b {
        // ? Check if the notes spent are valid
        let invalid_note: Option<(u64, String)> =
            find_invalid_note(state_tree, &open_order_fields_b.notes_in);

        if let Some(invalid_note) = invalid_note {
            position_escape.is_valid_b = false;
            position_escape.invalid_note = Some(invalid_note);
            return (position_escape, None);
        }

        notes_in = Some(open_order_fields_b.notes_in.clone());
        refund_note = open_order_fields_b.refund_note.clone();

        // ? order_b
        let res = handle_counter_party_open_order(
            state_tree,
            &position_a,
            close_price,
            open_order_fields_b,
            swap_funding_info.current_funding_idx,
        );
        if let Err(_e) = res {
            return (position_escape, None);
        }
        new_position_b = res.unwrap();
    } else if let Some(position_b) = position_b {
        let (position_exists, leaf_node) = verify_position_exists(state_tree, &position_b);
        if !position_exists {
            position_escape.is_position_valid_b = false;
            position_escape.valid_leaf_b = leaf_node;
            return (position_escape, None);
        }

        let res = handle_counter_party_modify_order(
            &position_a,
            close_price,
            position_b,
            swap_funding_info,
            index_price,
        );
        if let Err(_e) = res {
            return (position_escape, None);
        }
        new_position_b = res.unwrap();

        notes_in = None;
        refund_note = None;
    } else {
        panic!("position_b and open_order_fields_b cannot both be None")
    }

    // * Update the state -----------------------
    update_state_after_escape(
        state_tree,
        updated_state_hashes,
        firebase_session,
        backup_storage,
        position_a,
        new_position_b.clone(),
        notes_in,
        refund_note,
    );

    println!("VALID POSITION ESCAPE: {}", escape_id);

    return (position_escape, Some(new_position_b));
}

fn handle_counter_party_open_order(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    position_a: &PerpPosition,
    close_price: u64,
    open_order_fields_b: OpenOrderFields,
    latest_funding_idx: u32,
) -> Result<PerpPosition, String> {
    let synthetic_token = position_a.position_header.synthetic_token;
    let synthetic_amount = position_a.position_size;

    // ? Counter party is opening a new position to close the old one

    if open_order_fields_b.collateral_token != COLLATERAL_TOKEN {
        return Err("collateral token not valid".to_string());
    }
    let mut state_tree_m = state_tree.lock();
    let perp_zero_idx = state_tree_m.first_zero_idx();
    drop(state_tree_m);

    if let Err(err) = _check_note_sums(&open_order_fields_b, 0) {
        return Err(err.to_string());
    }

    if open_order_fields_b.refund_note.is_some() {
        if open_order_fields_b.notes_in[0].index
            != open_order_fields_b.refund_note.as_ref().unwrap().index
        {
            return Err("refund note index is not the same as the first note index".to_string());
        }
    }

    // ? Check that leverage is valid relative to the notional position size
    let nominal_collateral_amount =
        get_collateral_amount(synthetic_token, synthetic_amount, close_price);
    let leverage = (nominal_collateral_amount as u128 * 10_u128.pow(LEVERAGE_DECIMALS as u32)
        / (open_order_fields_b.initial_margin) as u128) as u64;

    if leverage > 15 * 10_u64.pow(LEVERAGE_DECIMALS as u32) {
        return Err("Leverage is too high".to_string());
    }

    let position_b = PerpPosition::new(
        position_a.order_side.clone(),
        synthetic_amount,
        synthetic_token,
        COLLATERAL_TOKEN,
        open_order_fields_b.initial_margin,
        leverage,
        open_order_fields_b.allow_partial_liquidations,
        open_order_fields_b.position_address,
        latest_funding_idx,
        perp_zero_idx as u32,
        0,
    );

    Ok(position_b)
}

// * -----------------------

fn handle_counter_party_modify_order(
    position_a: &PerpPosition,
    close_price: u64,
    mut position_b: PerpPosition,
    swap_funding_info: &SwapFundingInfo,
    index_price: u64,
) -> Result<PerpPosition, String> {
    if position_b
        .is_position_liquidatable(close_price, index_price)
        .0
    {
        return Err("Position_b is liquidatable".to_string());
    };

    if position_a.position_header.synthetic_token != position_b.position_header.synthetic_token {
        return Err("Synthetic token is not the same".to_string());
    }

    let idx_diff = position_b.last_funding_idx - swap_funding_info.min_swap_funding_idx;
    let applicable_funding_rates = &swap_funding_info.swap_funding_rates[idx_diff as usize..];
    let applicable_funding_prices = &swap_funding_info.swap_funding_prices[idx_diff as usize..];

    if position_a.order_side == position_b.order_side {
        // ? Increase position_b size

        // & Increasing the position size
        position_b.increase_position_size(
            position_a.position_size,
            close_price,
            0,
            applicable_funding_rates.to_vec(),
            applicable_funding_prices.to_vec(),
            swap_funding_info.current_funding_idx,
        );

        let leverage = position_b.get_current_leverage(close_price).unwrap();

        // ? Check that leverage is valid relative to the notional position size after increasing size
        if leverage > 15 * 10_u64.pow(LEVERAGE_DECIMALS as u32) {
            return Err("Leverage would be too high".to_string());
        }

        return Ok(position_b);
    } else {
        if position_a.position_size <= position_b.position_size {
            // ? Decrease position_b size

            position_b.reduce_position_size(
                position_a.position_size,
                close_price,
                0,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                swap_funding_info.current_funding_idx,
            );

            return Ok(position_b);
        } else {
            // ? Flip side position_b
            position_b.flip_position_side(
                position_a.position_size,
                close_price,
                0,
                applicable_funding_rates.to_vec(),
                applicable_funding_prices.to_vec(),
                swap_funding_info.current_funding_idx,
            );

            let leverage = position_b.get_current_leverage(close_price).unwrap();

            // ? Check that leverage is valid relative to the notional position size after increasing size
            if leverage > 15 * 10_u64.pow(LEVERAGE_DECIMALS as u32) {
                return Err("Leverage would be too high".to_string());
            }

            return Ok(position_b);
        }
    }
}

// * -----------------------

fn update_state_after_escape(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    position_a: PerpPosition,
    new_position_b: PerpPosition,
    notes_in: Option<Vec<Note>>,
    refund_note: Option<Note>,
) {
    let mut state_tree_m = state_tree.lock();
    let mut updated_state_hashes_m = updated_state_hashes.lock();

    // * Remove notes_in and add refund note if order_b is an open order
    if let Some(notes_in) = notes_in {
        for note in notes_in.iter() {
            let z = BigUint::zero();
            state_tree_m.update_leaf_node(&z, note.index);
            updated_state_hashes_m.insert(note.index, (LeafNodeType::Note, z));

            let _h = start_delete_note_thread(
                firebase_session,
                backup_storage,
                note.address.x.to_string(),
                note.index.to_string(),
            );
        }

        if let Some(refund_note) = refund_note {
            state_tree_m.update_leaf_node(&refund_note.hash, refund_note.index);
            updated_state_hashes_m.insert(
                refund_note.index,
                (LeafNodeType::Note, refund_note.hash.clone()),
            );

            let _h = start_add_note_thread(refund_note, firebase_session, backup_storage);
        }
    }

    // * Remove position_a
    let z = BigUint::zero();
    state_tree_m.update_leaf_node(&z, position_a.index as u64);
    updated_state_hashes_m.insert(position_a.index as u64, (LeafNodeType::Position, z));

    let _h = start_delete_position_thread(
        firebase_session,
        backup_storage,
        position_a.position_header.position_address.to_string(),
        position_a.index.to_string(),
    );

    // * Add new_position_b
    state_tree_m.update_leaf_node(&new_position_b.hash, new_position_b.index as u64);
    updated_state_hashes_m.insert(
        new_position_b.index as u64,
        (LeafNodeType::Position, new_position_b.hash.clone()),
    );

    let _h = start_add_position_thread(new_position_b, firebase_session, backup_storage);

    drop(state_tree_m);
    drop(updated_state_hashes_m);
}

// * -----------------------

fn verify_position_exists(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    escape_position: &PerpPosition,
) -> (bool, String) {
    let state_tree_m = state_tree.lock();

    let leaf_node = state_tree_m.get_leaf_by_index(escape_position.index as u64);
    let position_exists = escape_position.hash == leaf_node;
    return (position_exists, leaf_node.to_string());
}

// * -----------------------

// * -----------------------

fn verify_signatures(
    position_a: &PerpPosition,
    open_order_fields: &Option<OpenOrderFields>,
    position_b: &Option<PerpPosition>,
    signature_a: Signature,
    signature_b: Signature,
    order_hash: BigUint,
) -> bool {
    // * Verify signature a -----
    let valid = verify(
        &position_a.position_header.position_address,
        &order_hash,
        &signature_a,
    );
    if !valid {
        return false;
    }

    // * Verify signature b -----
    if let Some(open_order_fields) = open_order_fields {
        let mut pub_key_sum: AffinePoint = AffinePoint::identity();

        for i in 0..open_order_fields.notes_in.len() {
            let ec_point = AffinePoint::from(&open_order_fields.notes_in[i].address);
            pub_key_sum = &pub_key_sum + &ec_point;
        }

        let pub_key: EcPoint = EcPoint::from(&pub_key_sum);

        let valid = verify(&pub_key.x.to_biguint().unwrap(), &order_hash, &signature_b);
        if !valid {
            return false;
        }
    } else {
        let public_key = &position_b
            .as_ref()
            .unwrap()
            .position_header
            .position_address;

        let valid = verify(public_key, &order_hash, &signature_b);
        if !valid {
            return false;
        }
    }

    true
}

//

fn hash_position_escape_message(
    position_a: &PerpPosition,
    close_price: u64,
    open_order_fields_b: &Option<OpenOrderFields>,
    position_b: &Option<PerpPosition>,
    recipient: String,
) -> BigUint {
    let mut hash_inputs: Vec<BigUint> = Vec::new();

    // & H = pedersen(position_a.hash, close_price, (open_order_fields_b.hash or position_b.hash), recipient)

    let position_a_hash = hash_position_keccak(position_a);
    hash_inputs.push(position_a_hash);
    let close_price = BigUint::from_u64(close_price).unwrap();
    hash_inputs.push(close_price);

    let hash_inp;
    if let Some(fields) = open_order_fields_b {
        hash_inp = hash_open_order_fields_keccak(fields)
    } else {
        hash_inp = hash_position_keccak(position_b.as_ref().unwrap())
    }
    hash_inputs.push(hash_inp);

    let recipient = recipient.replace("0x", "");
    let recipient_;
    if let Ok(dep) = BigUint::from_str(&recipient) {
        recipient_ = dep;
    } else if let Ok(dep) = BigUint::from_str_radix(&recipient, 16) {
        recipient_ = dep;
    } else {
        panic!("recipient is not a valid number");
    }
    hash_inputs.push(recipient_);

    let order_hash = keccak256(&hash_inputs);

    let p = BigUint::from_str(
        "3618502788666131213697322783095070105623107215331596699973092056135872020481",
    )
    .unwrap();
    let hash_on_curve = order_hash % &p;

    return hash_on_curve;
}

pub fn hash_position_keccak(position: &PerpPosition) -> BigUint {
    // & hash = H({allow_partial_liquidations, synthetic_token, position_address, vlp_token, max_vlp_supply, order_side, position_size, entry_price, liquidation_price, last_funding_idx, vlp_supply})

    let mut input_arr = Vec::new();

    let allow_partial_liquidations = if position.position_header.allow_partial_liquidations {
        BigUint::one()
    } else {
        BigUint::zero()
    };
    input_arr.push(allow_partial_liquidations);

    let synthetic_token = BigUint::from_u32(position.position_header.synthetic_token).unwrap();
    input_arr.push(synthetic_token);

    input_arr.push(position.position_header.position_address.clone());

    let vlp_token = BigUint::from_u32(position.position_header.vlp_token).unwrap();
    input_arr.push(vlp_token);

    let max_vlp_supply = BigUint::from_u64(position.position_header.max_vlp_supply).unwrap();
    input_arr.push(max_vlp_supply);

    let order_side = if position.order_side == OrderSide::Long {
        BigUint::one()
    } else {
        BigUint::zero()
    };
    input_arr.push(order_side);

    let position_size = BigUint::from_u64(position.position_size).unwrap();
    input_arr.push(position_size);

    let entry_price = BigUint::from_u64(position.entry_price).unwrap();
    input_arr.push(entry_price);

    let liquidation_price = BigUint::from_u64(position.liquidation_price).unwrap();
    input_arr.push(liquidation_price);

    let last_funding_idx = BigUint::from_u32(position.last_funding_idx).unwrap();
    input_arr.push(last_funding_idx);

    let vlp_supply = BigUint::from_u64(position.vlp_supply).unwrap();
    input_arr.push(vlp_supply);

    let position_hash = keccak256(&input_arr);

    let p = BigUint::from_str(
        "3618502788666131213697322783095070105623107215331596699973092056135872020481",
    )
    .unwrap();
    let hash_on_curve = position_hash % &p;

    return hash_on_curve;
}

pub fn hash_open_order_fields_keccak(open_order_fields_b: &OpenOrderFields) -> BigUint {
    // & H = (note_hashes, refund_note_hash, initial_margin, collateral_token, position_address, allow_partial_liquidations)

    let mut input_arr = Vec::new();

    for note in open_order_fields_b.notes_in.iter() {
        let note_hash = hash_note_keccak(note);

        input_arr.push(note_hash);
    }

    let refund_note_hash = hash_note_keccak(&open_order_fields_b.refund_note.clone().unwrap());
    input_arr.push(refund_note_hash);

    let initial_margin = BigUint::from_u64(open_order_fields_b.initial_margin).unwrap();
    input_arr.push(initial_margin);

    let collateral_token = BigUint::from_u32(open_order_fields_b.collateral_token).unwrap();
    input_arr.push(collateral_token);

    input_arr.push(open_order_fields_b.position_address.clone());

    let allow_partial_liquidations = if open_order_fields_b.allow_partial_liquidations {
        BigUint::one()
    } else {
        BigUint::zero()
    };
    input_arr.push(allow_partial_liquidations);

    let fields_hash = keccak256(&input_arr);

    let p = BigUint::from_str(
        "3618502788666131213697322783095070105623107215331596699973092056135872020481",
    )
    .unwrap();
    let hash_on_curve = fields_hash % &p;

    return hash_on_curve;
}
