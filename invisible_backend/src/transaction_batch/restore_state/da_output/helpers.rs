use std::{collections::HashMap, str::FromStr};

use num_bigint::BigUint;
use num_traits::{FromPrimitive, One, Zero};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    order_tab::OrderTab,
    perpetual::{perp_position::PerpPosition, OrderSide},
    transaction_batch::LeafNodeType,
    utils::{
        crypto_utils::{hash, keccak256},
        notes::Note,
    },
};

// & ==================================================================================================================
// & HELPERS ==========================================================================================================

/// Check if the note is part of updated_state_hashes and if
/// it is, then parse and append it to note_outputs.
pub fn append_note_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    note_outputs: &mut Vec<(u64, [BigUint; 4])>,
    note: &Note,
) {
    let (leaf_type, leaf_hash) = updated_state_hashes.get(&note.index).unwrap();

    if leaf_type != &LeafNodeType::Note || leaf_hash != &note.hash {
        return;
    }

    let (index, output) = _get_note_output(note);
    note_outputs.push((index, output));
}

pub fn _get_note_output(note: &Note) -> (u64, [BigUint; 4]) {
    let hidden_amount = BigUint::from_u64(note.amount).unwrap()
        ^ &note.blinding % BigUint::from_u64(2).unwrap().pow(64);

    // & batched_note_info format: | token (32 bits) | hidden amount (64 bits) | idx (64 bits) |
    let batched_note_info = BigUint::from_u32(note.token).unwrap() << 128
        | hidden_amount << 64
        | BigUint::from_u64(note.index).unwrap();

    let commitment = hash(&BigUint::from_u64(note.amount).unwrap(), &note.blinding);

    return (
        note.index,
        [
            batched_note_info,
            commitment,
            note.address.x.to_biguint().unwrap(),
            note.address.y.to_biguint().unwrap(),
        ],
    );
}

// * ———————————————————————————————————————————————————————————————————————————————————— * //

/// Check if the position is part of updated_state_hashes and if
/// it is, then parse and append it to position_outputs.
pub fn append_position_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    position_outputs: &mut Vec<(u64, [BigUint; 3])>,
    position: &PerpPosition,
) {
    let (leaf_type, leaf_hash) = updated_state_hashes.get(&(position.index as u64)).unwrap();
    if leaf_type != &LeafNodeType::Position || leaf_hash != &position.hash {
        return;
    }

    let (index, output) = _get_position_output(position);
    position_outputs.push((index, output));
}

pub fn _get_position_output(position: &PerpPosition) -> (u64, [BigUint; 3]) {
    // & | index (64 bits) | synthetic_token (32 bits) | position_size (64 bits) | vlp_token (32 bits) |
    let batched_position_info_slot1 = BigUint::from_u64(position.index).unwrap() << 128
        | BigUint::from_u32(position.position_header.synthetic_token).unwrap() << 96
        | BigUint::from_u64(position.position_size).unwrap() << 32
        | BigUint::from_u32(position.position_header.vlp_token).unwrap();

    // & format: | entry_price (64 bits) | margin (64 bits) | vlp_supply (64 bits) | last_funding_idx (32 bits) | order_side (1 bits) | allow_partial_liquidations (1 bits) |
    let batched_position_info_slot2 = BigUint::from_u64(position.entry_price).unwrap() << 162
        | BigUint::from_u64(position.margin).unwrap() << 98
        | BigUint::from_u64(position.vlp_supply).unwrap() << 34
        | BigUint::from_u32(position.last_funding_idx).unwrap() << 2
        | if position.order_side == OrderSide::Long {
            BigUint::one()
        } else {
            BigUint::zero()
        } << 1
        | if position.position_header.allow_partial_liquidations {
            BigUint::one()
        } else {
            BigUint::zero()
        };

    let public_key = position.position_header.position_address.clone();

    return (
        position.index as u64,
        [
            batched_position_info_slot1,
            batched_position_info_slot2,
            public_key,
        ],
    );
}

// * ———————————————————————————————————————————————————————————————————————————————————— * //

/// Check if the order_tab is part of updated_state_hashes and if
/// it is, then parse and append it to tab_outputs.
pub fn append_tab_output(
    updated_state_hashes: &HashMap<u64, (LeafNodeType, BigUint)>,
    tab_outputs: &mut Vec<(u64, [BigUint; 4])>,
    order_tab: &OrderTab,
) {
    let (leaf_type, leaf_hash) = updated_state_hashes
        .get(&(order_tab.tab_idx as u64))
        .unwrap();

    if leaf_type != &LeafNodeType::OrderTab || leaf_hash != &order_tab.hash {
        return;
    }

    let (index, output) = _get_tab_output(order_tab);
    tab_outputs.push((index, output));
}

pub fn _get_tab_output(order_tab: &OrderTab) -> (u64, [BigUint; 4]) {
    let base_hidden_amount = BigUint::from_u64(order_tab.base_amount).unwrap()
        ^ &order_tab.tab_header.base_blinding % BigUint::from_u64(2).unwrap().pow(64);
    let quote_hidden_amount = BigUint::from_u64(order_tab.quote_amount).unwrap()
        ^ &order_tab.tab_header.quote_blinding % BigUint::from_u64(2).unwrap().pow(64);

    // & batched_tab_info_slot format: | index (59 bits) | base_token (32 bits) | quote_token (32 bits) | base_hidden_amount (64 bits) | quote_hidden_amount (64 bits)
    let batched_tab_info = BigUint::from_u64(order_tab.tab_idx).unwrap() << 192
        | BigUint::from_u32(order_tab.tab_header.base_token).unwrap() << 160
        | BigUint::from_u32(order_tab.tab_header.quote_token).unwrap() << 128
        | base_hidden_amount << 64
        | quote_hidden_amount;

    let base_commitment = hash(
        &BigUint::from_u64(order_tab.base_amount).unwrap(),
        &order_tab.tab_header.base_blinding,
    );
    let quote_commitment = hash(
        &BigUint::from_u64(order_tab.quote_amount).unwrap(),
        &order_tab.tab_header.quote_blinding,
    );

    let public_key = order_tab.tab_header.pub_key.clone();

    return (
        order_tab.tab_idx as u64,
        [
            batched_tab_info,
            base_commitment,
            quote_commitment,
            public_key,
        ],
    );
}

// * ———————————————————————————————————————————————————————————————————————————————————— * //

#[derive(Debug, Serialize, Deserialize)]
pub struct DepositRequest {
    pub deposit_id: u64,
    pub token_id: u32,
    pub amount: u64,
    pub stark_key: String,
}
pub fn _update_output_deposits(
    deposit: &Value,
    deposit_outputs: &mut HashMap<u32, Vec<DepositRequest>>,
    accumulated_deposit_hashes: &mut HashMap<u32, BigUint>,
) {
    let deposit_id = deposit.get("deposit_id").unwrap().as_u64().unwrap();
    let token_id = deposit.get("deposit_token").unwrap().as_u64().unwrap() as u32;
    let amount = deposit.get("deposit_amount").unwrap().as_u64().unwrap();
    let stark_key = deposit.get("stark_key").unwrap().as_str().unwrap();

    let deposit_output = DepositRequest {
        deposit_id,
        token_id,
        amount,
        stark_key: stark_key.to_string(),
    };

    let chain_id = (deposit_id / 2u64.pow(32)) as u32;

    // * Update deposit outputs ==================================== * //
    let dep_outputs = deposit_outputs.get_mut(&chain_id);
    if let Some(dep_outputs) = dep_outputs {
        dep_outputs.push(deposit_output);
    } else {
        deposit_outputs.insert(chain_id, vec![deposit_output]);
    }

    // * Update accumulated deposit hashes ========================== * //
    let batched_deposit_info = BigUint::from_u64(deposit_id).unwrap() << 96
        | BigUint::from_u32(token_id).unwrap() << 64
        | BigUint::from_u64(amount).unwrap();

    let deposit_hash = keccak256(&vec![
        batched_deposit_info,
        BigUint::from_str(stark_key).unwrap(),
    ]);

    let z = BigUint::zero();
    let prev_deposit_hash = accumulated_deposit_hashes.get(&chain_id).unwrap_or(&z);
    let new_deposit_hash = keccak256(&vec![prev_deposit_hash.clone(), deposit_hash]);

    accumulated_deposit_hashes.insert(chain_id, new_deposit_hash);
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithdrawalRequest {
    pub chain_id: u32,
    pub token_id: u32,
    pub amount: u64,
    pub recipient: String,
}
pub fn _update_output_withdrawals(
    withdrawal: &Value,
    withdrawal_outputs: &mut HashMap<u32, Vec<WithdrawalRequest>>,
    accumulated_withdrawal_hashes: &mut HashMap<u32, BigUint>,
) {
    let chain_id = withdrawal
        .get("withdrawal_chain")
        .unwrap()
        .as_u64()
        .unwrap() as u32;
    let token_id = withdrawal
        .get("withdrawal_token")
        .unwrap()
        .as_u64()
        .unwrap() as u32;
    let amount = withdrawal
        .get("withdrawal_amount")
        .unwrap()
        .as_u64()
        .unwrap();

    let recipient = withdrawal.get("recipient").unwrap().as_str().unwrap();

    // * Update withdrawal outputs ==================================== * //
    let withdrawal_output = WithdrawalRequest {
        chain_id,
        token_id,
        amount,
        recipient: recipient.to_string(),
    };

    let with_outputs = withdrawal_outputs.get_mut(&chain_id);
    if let Some(with_outputs) = with_outputs {
        with_outputs.push(withdrawal_output);
    } else {
        withdrawal_outputs.insert(chain_id, vec![withdrawal_output]);
    }

    // * Update accumulated withdrawal hashes ========================== * //
    let batched_withdrawal_info = BigUint::from_u32(chain_id).unwrap() << 64
        | BigUint::from_u32(token_id).unwrap() << 64
        | BigUint::from_u64(amount).unwrap();

    let withdrawal_hash = keccak256(&vec![
        batched_withdrawal_info,
        BigUint::from_str(recipient).unwrap(),
    ]);

    let z: BigUint = BigUint::zero();
    let prev_withdrawal_hash = accumulated_withdrawal_hashes.get(&chain_id).unwrap_or(&z);
    let new_withdrawal_hash = keccak256(&vec![prev_withdrawal_hash.clone(), withdrawal_hash]);

    accumulated_withdrawal_hashes.insert(chain_id, new_withdrawal_hash);
}
