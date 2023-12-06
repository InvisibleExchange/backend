use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

use crate::order_tab::OrderTab;
use crate::perpetual::perp_order::OpenOrderFields;
use crate::perpetual::perp_position::PerpPosition;
use crate::transaction_batch::tx_batch_structs::SwapFundingInfo;
use crate::utils::crypto_utils::Signature;
use crate::{server::grpc::engine_proto::EscapeMessage, transaction_batch::LeafNodeType};
use crate::{
    trees::superficial_tree::SuperficialTree, utils::storage::local_storage::BackupStorage,
};

use crate::utils::notes::Note;

use super::note_escapes::verify_note_escape;
use super::order_tab_escapes::verify_order_tab_escape;
use super::positon_escapes::verify_position_escape;

pub fn _execute_forced_escape_inner(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    updated_state_hashes: &Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
    firebase_session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    swap_output_json: &Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
    escape_message: EscapeMessage,
    swap_funding_info: &Option<SwapFundingInfo>,
    index_price: u64,
) {
    let escape_id = escape_message.escape_id;

    // ? Escape Notes
    if escape_message.escape_notes.len() > 0 {
        let escape_notes = escape_message
            .escape_notes
            .iter()
            .map(|n| Note::try_from(n.clone()).unwrap())
            .collect::<Vec<Note>>();

        let sig = escape_message.signature.unwrap();
        let signature = Signature { r: sig.r, s: sig.s };

        let note_escape = verify_note_escape(
            &state_tree,
            &updated_state_hashes,
            &firebase_session,
            &backup_storage,
            escape_id,
            escape_notes,
            signature,
        );

        let mut json_map = serde_json::map::Map::new();
        json_map.insert(
            String::from("transaction_type"),
            serde_json::to_value(&"forced_escape").unwrap(),
        );
        json_map.insert(
            String::from("escape_type"),
            serde_json::to_value(&"note_escape").unwrap(),
        );
        json_map.insert(
            String::from("note_escape"),
            serde_json::to_value(&note_escape).unwrap(),
        );

        let mut swap_output_json_m = swap_output_json.lock();
        swap_output_json_m.push(json_map);
        drop(swap_output_json_m);
    } else if let Some(close_order_tab_req) = escape_message.close_order_tab_req {
        let order_tab = OrderTab::try_from(close_order_tab_req).unwrap();

        let sig = escape_message.signature.unwrap();
        let signature = Signature { r: sig.r, s: sig.s };

        let tab_escape = verify_order_tab_escape(
            state_tree,
            updated_state_hashes,
            firebase_session,
            backup_storage,
            escape_id,
            order_tab,
            signature,
        );

        let mut json_map = serde_json::map::Map::new();
        json_map.insert(
            String::from("transaction_type"),
            serde_json::to_value(&"forced_escape").unwrap(),
        );
        json_map.insert(
            String::from("escape_type"),
            serde_json::to_value(&"order_tab_escape").unwrap(),
        );
        json_map.insert(
            String::from("tab_escape"),
            serde_json::to_value(&tab_escape).unwrap(),
        );

        let mut swap_output_json_m = swap_output_json.lock();
        swap_output_json_m.push(json_map);
        drop(swap_output_json_m);
    } else if let Some(close_position_message) = escape_message.close_position_message {
        let position_a =
            PerpPosition::try_from(close_position_message.position_a.unwrap()).unwrap();
        let close_price = close_position_message.close_price;

        let open_order_fields_b = match close_position_message.open_order_fields_b {
            Some(open_order_fields_b) => {
                Some(OpenOrderFields::try_from(open_order_fields_b).unwrap())
            }
            None => None,
        };
        let position_b = match close_position_message.position_b {
            Some(position_b) => Some(PerpPosition::try_from(position_b).unwrap()),
            None => None,
        };

        let sig_a = close_position_message.signature_a.unwrap();
        let sig_b = close_position_message.signature_b.unwrap();
        let signature_a = Signature {
            r: sig_a.r,
            s: sig_a.s,
        };
        let signature_b = Signature {
            r: sig_b.r,
            s: sig_b.s,
        };

        let swap_funding_info = swap_funding_info.as_ref().unwrap();

        let position_escape = verify_position_escape(
            state_tree,
            updated_state_hashes,
            firebase_session,
            backup_storage,
            escape_id,
            position_a,
            close_price,
            open_order_fields_b,
            position_b,
            signature_a,
            signature_b,
            swap_funding_info,
            index_price,
        );

        let mut json_map = serde_json::map::Map::new();
        json_map.insert(
            String::from("transaction_type"),
            serde_json::to_value(&"forced_escape").unwrap(),
        );
        json_map.insert(
            String::from("escape_type"),
            serde_json::to_value(&"position_escape").unwrap(),
        );
        json_map.insert(
            String::from("position_escape"),
            serde_json::to_value(&position_escape).unwrap(),
        );

        let mut swap_output_json_m = swap_output_json.lock();
        swap_output_json_m.push(json_map);
        drop(swap_output_json_m);
    }
}

pub fn _get_position_close_escape_info(
    funding_rates: &HashMap<u32, Vec<i64>>,
    funding_prices: &HashMap<u32, Vec<u64>>,
    latest_index_price: &HashMap<u32, u64>,
    escape_message: &EscapeMessage,
) -> (u64, Option<SwapFundingInfo>, u32) {
    let (index_price, swap_funding_info, synthetic_token) = match &escape_message
        .close_position_message
    {
        Some(close_position_message) => {
            let position_a =
                PerpPosition::try_from(close_position_message.position_a.clone().unwrap()).unwrap();
            let position_b = close_position_message.position_b.as_ref().map(|pos| {
                return PerpPosition::try_from(pos.clone()).unwrap();
            });

            let synthetic_token = position_a.position_header.synthetic_token;
            let index_price = latest_index_price.get(&synthetic_token).unwrap();

            let swap_funding_info = SwapFundingInfo::new(
                &funding_rates,
                &funding_prices,
                synthetic_token,
                &Some(position_a),
                &position_b,
            );

            (*index_price, Some(swap_funding_info), synthetic_token)
        }
        None => (0, None, 0),
    };

    return (index_price, swap_funding_info, synthetic_token);
}
