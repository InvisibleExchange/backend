use std::str::FromStr;
use std::sync::Arc;

use firestore_db_and_auth::{documents, errors::FirebaseError, ServiceSession};
use num_bigint::BigUint;
use num_traits::FromPrimitive;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use starknet::core::types::FieldElement;
use starknet::curve::AffinePoint;

use crate::perpetual::perp_position::{
    get_liquidation_price, PositionHeader, _get_bankruptcy_price, _hash_position,
};
use crate::utils::cairo_output::{NoteOutput, OrderTabOutput, PerpPositionOutput};
use crate::utils::crypto_utils::hash;
use crate::{order_tab::OrderTab, perpetual::perp_position::PerpPosition, utils::notes::Note};

use super::backup_storage::BackupStorage;

// * NOTE -------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug)]
pub struct FirebaseNoteObject {
    pub address: [String; 2],
    pub commitment: String,
    pub hidden_amount: String,
    pub index: String,
    pub token: String,
    pub hash: String,
}

impl FirebaseNoteObject {
    pub fn from_note(note: &Note) -> FirebaseNoteObject {
        let yt_digits = note.blinding.to_u64_digits();
        let yt_trimmed = if yt_digits.len() == 0 {
            0
        } else {
            yt_digits[0]
        };

        let hidden_amount = note.amount ^ yt_trimmed;

        return FirebaseNoteObject {
            address: [note.address.x.to_string(), note.address.y.to_string()],
            commitment: hash(&BigUint::from_u64(note.amount).unwrap(), &note.blinding).to_string(),
            hidden_amount: hidden_amount.to_string(),
            index: note.index.to_string(),
            token: note.token.to_string(),
            hash: note.hash.to_string(),
        };
    }

    pub fn from_note_object(note_output: NoteOutput) -> FirebaseNoteObject {
        let adddress_point = if note_output.address_x == "0".to_string() {
            AffinePoint::identity()
        } else {
            AffinePoint {
                x: FieldElement::from_dec_str(&note_output.address_x).unwrap(),
                y: FieldElement::from_dec_str(&note_output.address_y).unwrap(),
                infinity: false,
            }
        };

        return FirebaseNoteObject {
            address: [adddress_point.x.to_string(), adddress_point.y.to_string()],
            commitment: note_output.commitment,
            hidden_amount: note_output.hidden_amount.to_string(),
            index: note_output.index.to_string(),
            token: note_output.token.to_string(),
            hash: note_output.hash,
        };
    }
}

pub fn store_new_note(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    note: &Note,
) {
    let obj = FirebaseNoteObject::from_note(note);

    let res = _store_note_inner(session, obj);

    if let Err(e) = res {
        println!("Error storing note in backup storage. ERROR: {:?}", e);
        let s = backup_storage.lock();
        if let Err(_e) = s.store_note(note) {};
        drop(s);
    }
}

pub fn store_note_output(session: &ServiceSession, note: NoteOutput) {
    let obj = FirebaseNoteObject::from_note_object(note);

    let _res = _store_note_inner(session, obj);
}

fn _store_note_inner(
    session: &ServiceSession,
    obj: FirebaseNoteObject,
) -> Result<documents::WriteResult, FirebaseError> {
    let write_path = format!("notes");
    let res = documents::write(
        session,
        write_path.as_str(),
        Some(obj.index.to_string()),
        &obj,
        documents::WriteOptions::default(),
    );

    // ? ----------------------------------------
    let write_path = format!("addr2idx/addresses/{}", obj.address[0].to_string());
    let _res = documents::write(
        session,
        write_path.as_str(),
        Some(obj.index.to_string()),
        &json!({}),
        documents::WriteOptions::default(),
    );

    return res;
}

pub fn delete_note_at_address(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    address: &str,
    idx: &str,
) {
    // & address is the x coordinate in string format and idx is the index in string format

    let delete_path = format!("notes/{}", idx);
    let r = documents::delete(session, delete_path.as_str(), true);
    if let Err(e) = r {
        if let FirebaseError::APIError(numeric_code, string_code, _context) = e {
            if string_code.starts_with("No document to update") && numeric_code == 404 {
                return;
            }
        } else {
            println!("Error deleting note from backup storage. ERROR: {:?}", e);
        }

        let s = backup_storage.lock();
        if let Err(_e) = s.store_note_removal(u64::from_str_radix(idx, 10).unwrap(), address) {}
    }

    // ? ----------------------------------------

    let delete_path = format!("addr2idx/addresses/{}/{}", address, idx);
    let _r = documents::delete(session, delete_path.as_str(), true);
}

// * POSITIONS ---------------------------------------------------------------------------

pub fn delete_position_at_address(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    address: &str,
    idx: &str,
) {
    // & address is the x coordinate in string format and idx is the index in string format
    let delete_path = format!("positions/{}", idx);
    let r = documents::delete(session, delete_path.as_str(), true);
    if let Err(e) = r {
        if let FirebaseError::APIError(numeric_code, string_code, _context) = e {
            if string_code.starts_with("No document to update") && numeric_code == 404 {
                return;
            }
        } else {
            println!("Error deleting note from database: ERROR: {:?}", e);
        }

        let s = backup_storage.lock();
        if let Err(_e) = s.store_position_removal(u64::from_str_radix(idx, 10).unwrap(), address) {}
    }

    // ? ===================================================================
    // ? Delete the position's liquidation price in the database
    let delete_path = format!("liquidations/{}", address.to_string() + "-" + idx);

    let r = documents::delete(session, delete_path.as_str(), true);

    if let Err(e) = r {
        if let FirebaseError::APIError(numeric_code, string_code, _context) = e {
            if string_code.starts_with("No document to update") && numeric_code == 404 {
                return;
            }
        } else {
            println!("Error deleting liquidation from database: ERROR: {:?}", e);
        }
    }

    // ? ----------------------------------------

    let delete_path = format!("addr2idx/addresses/{}/{}", address, idx);
    let _r = documents::delete(session, delete_path.as_str(), true);
}

pub fn store_position_output(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    position_output: PerpPositionOutput,
) {
    let position_header = PositionHeader::new(
        position_output.synthetic_token,
        position_output.allow_partial_liquidations,
        BigUint::from_str("position_output").unwrap(),
        position_output.vlp_token,
    );

    let liquidation_price = get_liquidation_price(
        position_output.entry_price,
        position_output.margin,
        position_output.position_size,
        &position_output.order_side,
        position_output.synthetic_token,
        position_output.allow_partial_liquidations,
    );

    let bankruptcy_price = _get_bankruptcy_price(
        position_output.entry_price,
        position_output.margin,
        position_output.position_size,
        &position_output.order_side,
        position_output.synthetic_token,
    );

    let hash = _hash_position(
        &position_header.hash,
        &position_output.order_side,
        position_output.position_size,
        position_output.entry_price,
        liquidation_price,
        position_output.last_funding_idx,
        position_output.vlp_supply,
    );

    let position = PerpPosition {
        position_header,
        margin: position_output.margin,
        position_size: position_output.position_size,
        order_side: position_output.order_side.clone(),
        entry_price: position_output.entry_price,
        liquidation_price,
        bankruptcy_price,
        last_funding_idx: position_output.last_funding_idx,
        vlp_supply: position_output.vlp_supply,
        index: position_output.index,
        hash,
    };

    store_new_position(session, backup_storage, &position);
}

pub fn store_new_position(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    position: &PerpPosition,
) {
    // ? Store the position in the database
    let write_path = format!("positions");

    let _res = documents::write(
        session,
        write_path.as_str(),
        Some(position.index.to_string()),
        position,
        documents::WriteOptions::default(),
    );

    if let Err(e) = _res {
        println!("Error storing position to database. ERROR: {:?}", e);
        let s = backup_storage.lock();
        if let Err(_e) = s.store_position(position) {};
        drop(s);
    }

    // ? ===================================================================
    // ? Store the position's liquidation price in the database
    let write_path = format!(
        "{}",
        position.position_header.position_address.to_string()
            + "-"
            + position.index.to_string().as_str()
    );

    let _res = documents::write(
        session,
        "liquidations",
        Some(write_path),
        &json!({
            "liquidation_price": &position.liquidation_price,
            "synthetic_token": &position.position_header. synthetic_token,
            "order_side": &position.order_side,
        }),
        documents::WriteOptions::default(),
    );

    if let Err(e) = _res {
        println!(
            "Error storing liquidation price to database. ERROR: {:?}",
            e
        );
    }

    // ? ----------------------------------------

    let write_path = format!(
        "addr2idx/addresses/{}",
        position.position_header.position_address.to_string()
    );
    let _res = documents::write(
        session,
        write_path.as_str(),
        Some(position.index.to_string()),
        &json!({}),
        documents::WriteOptions::default(),
    );
}

// * ORDER TAB --------------------------------------------------------------------------

// ? Order Tab
#[derive(Serialize, Deserialize, Debug)]
pub struct OrderTabObject {
    pub index: u64,
    // header
    pub base_token: u32,
    pub quote_token: u32,
    pub pub_key: String,
    //
    pub base_commitment: String,
    pub base_hidden_amount: String,
    pub quote_commitment: String,
    pub quote_hidden_amount: String,
    pub hash: String,
}

impl OrderTabObject {
    pub fn from_order_tab(order_tab: &OrderTab) -> Self {
        // ? Hide base amount
        let base_yt_digits = order_tab.tab_header.base_blinding.to_u64_digits();
        let base_yt_trimmed = if base_yt_digits.len() == 0 {
            0
        } else {
            base_yt_digits[0]
        };
        let base_hidden_amount = order_tab.base_amount ^ base_yt_trimmed;

        // ? Hide quote amount
        let quote_yt_digits = order_tab.tab_header.quote_blinding.to_u64_digits();
        let quote_yt_trimmed = if quote_yt_digits.len() == 0 {
            0
        } else {
            quote_yt_digits[0]
        };
        let quote_hidden_amount = order_tab.quote_amount ^ quote_yt_trimmed;

        return OrderTabObject {
            index: order_tab.tab_idx,
            base_token: order_tab.tab_header.base_token,
            quote_token: order_tab.tab_header.quote_token,
            pub_key: order_tab.tab_header.pub_key.to_string(),
            base_commitment: hash(
                &BigUint::from_u64(order_tab.base_amount).unwrap(),
                &order_tab.tab_header.base_blinding,
            )
            .to_string(),
            base_hidden_amount: base_hidden_amount.to_string(),
            quote_commitment: hash(
                &BigUint::from_u64(order_tab.quote_amount).unwrap(),
                &order_tab.tab_header.quote_blinding,
            )
            .to_string(),
            quote_hidden_amount: quote_hidden_amount.to_string(),
            hash: order_tab.hash.to_string(),
        };
    }

    pub fn from_order_tab_output(order_tab_output: OrderTabOutput) -> Self {
        return OrderTabObject {
            index: order_tab_output.index,
            base_token: order_tab_output.base_token,
            quote_token: order_tab_output.quote_token,
            pub_key: order_tab_output.public_key,
            base_commitment: order_tab_output.base_commitment,
            base_hidden_amount: order_tab_output.base_hidden_amount.to_string(),
            quote_commitment: order_tab_output.quote_commitment,
            quote_hidden_amount: order_tab_output.quote_hidden_amount.to_string(),
            hash: order_tab_output.hash,
        };
    }
}

pub fn store_order_tab(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    order_tab: &OrderTab,
) {
    let obj = OrderTabObject::from_order_tab(order_tab);

    let res = store_order_tab_inner(session, obj);

    if let Err(e) = res {
        println!("Error storing note in backup storage. ERROR: {:?}", e);
        let s = backup_storage.lock();
        if let Err(_e) = s.store_order_tab(order_tab) {};
        drop(s);
    }
}

pub fn store_order_tab_output(session: &ServiceSession, order_tab_output: OrderTabOutput) {
    let obj = OrderTabObject::from_order_tab_output(order_tab_output);

    let _res = store_order_tab_inner(session, obj);
}

fn store_order_tab_inner(
    session: &ServiceSession,
    obj: OrderTabObject,
) -> Result<documents::WriteResult, FirebaseError> {
    let write_path = format!("order_tabs",);
    let res = documents::write(
        session,
        write_path.as_str(),
        Some(obj.index.to_string()),
        &obj,
        documents::WriteOptions::default(),
    );

    // ? ----------------------------------------

    let write_path = format!("addr2idx/addresses/{}", obj.pub_key.to_string());
    let _res = documents::write(
        session,
        write_path.as_str(),
        Some(obj.index.to_string()),
        &json!({}),
        documents::WriteOptions::default(),
    );

    return res;
}

pub fn delete_order_tab(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    pub_key: &str,
    idx: &str,
) {
    // & address is the x coordinate in string format and idx is the index in string format

    let delete_path = format!("order_tabs/{}", idx);
    let r = documents::delete(session, delete_path.as_str(), true);
    if let Err(e) = r {
        if let FirebaseError::APIError(numeric_code, string_code, _context) = e {
            if string_code.starts_with("No document to update") && numeric_code == 404 {
                return;
            }
        } else {
            println!("Error deleting note from backup storage. ERROR: {:?}", e);
        }

        let s = backup_storage.lock();
        if let Err(_e) = s.store_order_tab_removal(u64::from_str_radix(idx, 10).unwrap(), pub_key) {
        }
    }

    // ? ----------------------------------------

    let delete_path = format!("addr2idx/addresses/{}/{}", pub_key, idx);
    let _r = documents::delete(session, delete_path.as_str(), true);
}
