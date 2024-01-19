pub mod backup_storage;
pub mod firestore;
mod firestore_helpers;
pub mod local_storage;

use std::time::Instant;

use bincode::serialize;
use num_bigint::BigUint;
use num_traits::{ToPrimitive, Zero};
use serde_json::to_vec;

use sled::Config;

use crate::{perpetual::OrderSide, transaction_batch::LeafNodeType};

use super::cairo_output::{
    hash_note_output, hash_order_tab_output, hash_position_output, split_by_bytes, NoteOutput,
    OrderTabOutput, PerpPositionOutput,
};

/// The main storage struct that stores all the data on disk.
pub struct StateStorage {
    pub state_db: sled::Db, // Stores the state values (notes/tabs/positions)
}

impl StateStorage {
    pub fn new() -> Self {
        let config = Config::new().path("./storage/state".to_string());
        let state_db = config.open().unwrap();

        StateStorage { state_db }
    }
}

pub fn store_new_state_updates(
    note_outputs: &Vec<(u64, [BigUint; 3])>,
    position_outputs: &Vec<(u64, [BigUint; 3])>,
    tab_outputs: &Vec<(u64, [BigUint; 4])>,
    zero_indexes: &Vec<u64>,
) {
    let mut batch = sled::Batch::default();

    println!("Storing {} new notes", note_outputs.len());
    println!("Storing {} new positions", position_outputs.len());
    println!("Storing {} new tabs", tab_outputs.len());
    println!("Storing {} zero indexes", zero_indexes.len());

    let now = Instant::now();

    for (index, note_val) in note_outputs {
        batch.insert(to_vec(index).unwrap(), serialize(&note_val).unwrap());

        batch.insert(
            to_vec(&("leaf_type".to_string() + &index.to_string())).unwrap(),
            serialize(&LeafNodeType::Note).unwrap(),
        );
    }

    for (index, position_val) in position_outputs {
        batch.insert(to_vec(index).unwrap(), serialize(&position_val).unwrap());

        batch.insert(
            to_vec(&("leaf_type".to_string() + &index.to_string())).unwrap(),
            serialize(&LeafNodeType::Position).unwrap(),
        );
    }

    for (index, tab_val) in tab_outputs {
        batch.insert(to_vec(index).unwrap(), serialize(&tab_val).unwrap());

        batch.insert(
            to_vec(&("leaf_type".to_string() + &index.to_string())).unwrap(),
            serialize(&LeafNodeType::OrderTab).unwrap(),
        );
    }

    for index in zero_indexes {
        batch.remove(to_vec(index).unwrap());
        batch.remove(to_vec(&("leaf_type".to_string() + &index.to_string())).unwrap());
    }

    let config = Config::new().path("./storage/state".to_string());
    let state_db = config.open().unwrap();

    if let Err(err) = state_db.apply_batch(batch) {
        println!("Error storing state updates: {:?}", err.to_string())
    } else {
        println!("All state updates stored!");
    }

    println!("Time to store state updates: {:?}", now.elapsed());
}

pub enum StateValue {
    Note(NoteOutput),
    Position(PerpPositionOutput),
    OrderTab(OrderTabOutput),
}

pub fn get_state_at_index(index: u64) -> Option<(LeafNodeType, StateValue)> {
    let config = Config::new().path("./storage/state".to_string());
    let state_db = config.open().unwrap();

    let state_data = state_db
        .get(to_vec(&("leaf_type".to_string() + &index.to_string())).unwrap())
        .unwrap();

    if state_data.is_none() {
        return None;
    }

    let val: Result<LeafNodeType, Box<bincode::ErrorKind>> =
        bincode::deserialize(&state_data.unwrap());

    if let Err(_x) = val {
        return None;
    }

    let leaf_type: LeafNodeType = val.unwrap();

    match leaf_type {
        LeafNodeType::Note => {
            let note_data = state_db.get(index.to_string()).unwrap();
            let note_data: [BigUint; 3] = bincode::deserialize(&note_data.unwrap()).unwrap();

            let note = parse_note_data(note_data);

            return Some((LeafNodeType::Note, StateValue::Note(note)));
        }
        LeafNodeType::Position => {
            let position_data = state_db.get(index.to_string()).unwrap();

            let x: Result<[BigUint; 3], Box<bincode::ErrorKind>> =
                bincode::deserialize(&position_data.clone().unwrap());
            println!("x: {:?}", x);

            let position_data: [BigUint; 3] =
                bincode::deserialize(&position_data.unwrap()).unwrap();

            let position = parse_position_data(position_data);

            return Some((LeafNodeType::Position, StateValue::Position(position)));
        }
        LeafNodeType::OrderTab => {
            let tab_data = state_db.get(index.to_string()).unwrap();
            let tab_data: [BigUint; 4] = bincode::deserialize(&tab_data.unwrap()).unwrap();

            let tab = parse_tab_data(tab_data);

            return Some((LeafNodeType::OrderTab, StateValue::OrderTab(tab)));
        }
    }
}

pub fn parse_note_data(note_data: [BigUint; 3]) -> NoteOutput {
    let batched_note_info = note_data[0].clone();

    let split_vec = split_by_bytes(&batched_note_info, vec![32, 64, 64]);
    let token = split_vec[0].to_u32().unwrap();
    let hidden_amount = split_vec[1].to_u64().unwrap();
    let index = split_vec[2].to_u64().unwrap();

    let commitment = &note_data[1];
    let address = &note_data[2];

    let hash = hash_note_output(token, &commitment, &address).to_string();

    let note = NoteOutput {
        index,
        token,
        hidden_amount,
        commitment: commitment.to_string(),
        address: address.to_string(),
        hash,
    };

    return note;
}

pub fn parse_position_data(position_data: [BigUint; 3]) -> PerpPositionOutput {
    let batched_position_info_slot1 = position_data[0].clone();
    let batched_position_info_slot2 = position_data[1].clone();

    // & | index (64 bits) | synthetic_token (32 bits) | position_size (64 bits) | order_side (8 bits) | allow_partial_liquidations (8 bit)
    let split_vec_slot1 = split_by_bytes(&batched_position_info_slot1, vec![64, 32, 64, 8, 8]);
    let split_vec_slot2 = split_by_bytes(&batched_position_info_slot2, vec![64, 64, 32]);

    let index = split_vec_slot1[0].to_u64().unwrap();
    let synthetic_token = split_vec_slot1[1].to_u32().unwrap();
    let position_size = split_vec_slot1[2].to_u64().unwrap();
    let order_side = if split_vec_slot1[3] != BigUint::zero() {
        OrderSide::Long
    } else {
        OrderSide::Short
    };
    let allow_partial_liquidations = split_vec_slot1[4] != BigUint::zero();

    let entry_price = split_vec_slot2[0].to_u64().unwrap();
    let liquidation_price = split_vec_slot2[1].to_u64().unwrap();
    let last_funding_idx = split_vec_slot2[2].to_u32().unwrap();

    let public_key = &position_data[2];

    let hash = hash_position_output(
        synthetic_token,
        public_key,
        allow_partial_liquidations,
        //
        &order_side,
        position_size,
        entry_price,
        liquidation_price,
        last_funding_idx,
    )
    .to_string();

    let position = PerpPositionOutput {
        synthetic_token,
        position_size,
        order_side,
        entry_price,
        liquidation_price,
        last_funding_idx,
        allow_partial_liquidations,
        index,
        public_key: public_key.to_string(),
        hash,
    };

    return position;
}

pub fn parse_tab_data(tab_data: [BigUint; 4]) -> OrderTabOutput {
    let batched_tab_info = &tab_data[0];
    let split_vec = split_by_bytes(&batched_tab_info, vec![59, 32, 32, 64, 64]);

    let index = split_vec[0].to_u64().unwrap();
    let base_token = split_vec[1].to_u32().unwrap();
    let quote_token = split_vec[2].to_u32().unwrap();
    let base_hidden_amount = split_vec[3].to_u64().unwrap();
    let quote_hidden_amount = split_vec[4].to_u64().unwrap();

    let base_commitment = &tab_data[1];
    let quote_commitment = &tab_data[2];
    let public_key = &tab_data[3];

    let hash = hash_order_tab_output(
        base_token,
        quote_token,
        &public_key,
        &base_commitment,
        &quote_commitment,
    )
    .to_string();

    let order_tab = OrderTabOutput {
        index,
        base_token,
        quote_token,
        base_hidden_amount,
        quote_hidden_amount,
        base_commitment: base_commitment.to_string(),
        quote_commitment: quote_commitment.to_string(),
        public_key: public_key.to_string(),
        hash,
    };

    return order_tab;
}
