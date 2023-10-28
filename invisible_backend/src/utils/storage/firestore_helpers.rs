use std::sync::Arc;

use firestore_db_and_auth::{documents, errors::FirebaseError, ServiceSession};
use num_bigint::BigUint;
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{order_tab::OrderTab, perpetual::perp_position::PerpPosition, utils::notes::Note};

use crate::utils::crypto_utils::pedersen;

use super::local_storage::BackupStorage;

// * NOTE -------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug)]
pub struct FirebaseNoteObject {
    pub address: [String; 2],
    pub commitment: String,
    pub hidden_amount: String,
    pub index: String,
    pub token: String,
}

impl FirebaseNoteObject {
    pub fn from_note(note: &Note) -> FirebaseNoteObject {
        // let hash8 = trimHash(yt, 64);
        // let hiddentAmount = bigInt(amount).xor(hash8).value;

        let yt_digits = note.blinding.to_u64_digits();
        let yt_trimmed = if yt_digits.len() == 0 {
            0
        } else {
            yt_digits[0]
        };

        let hidden_amount = note.amount ^ yt_trimmed;

        return FirebaseNoteObject {
            address: [note.address.x.to_string(), note.address.y.to_string()],
            commitment: pedersen(&BigUint::from_u64(note.amount).unwrap(), &note.blinding)
                .to_string(),
            hidden_amount: hidden_amount.to_string(),
            index: note.index.to_string(),
            token: note.token.to_string(),
        };
    }
}

pub fn store_new_note(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    note: &Note,
) {
    let obj = FirebaseNoteObject::from_note(note);

    let write_path = format!("notes");
    let res = documents::write(
        session,
        write_path.as_str(),
        Some(note.index.to_string()),
        &obj,
        documents::WriteOptions::default(),
    );

    if let Err(e) = res {
        println!("Error storing note in backup storage. ERROR: {:?}", e);
        let s = backup_storage.lock();
        if let Err(_e) = s.store_note(note) {};
        drop(s);
    }

    // ? ----------------------------------------

    let write_path = format!("addr2idx/addresses/{}", note.address.x.to_string());
    let _res = documents::write(
        session,
        write_path.as_str(),
        Some(note.index.to_string()),
        &json!({}),
        documents::WriteOptions::default(),
    );
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
    pub index: u32,
    // header
    pub is_smart_contract: bool,
    pub base_token: u32,
    pub quote_token: u32,
    pub vlp_token: u32,
    pub max_vlp_supply: u64,
    pub pub_key: String,
    //
    pub base_commitment: String,
    pub base_hidden_amount: String,
    pub quote_commitment: String,
    pub quote_hidden_amount: String,
    pub vlp_supply_commitment: String,
    pub vlp_supply_hidden_amount: String,
    pub hash: String,
}

impl OrderTabObject {
    pub fn from_order_tab(order_tab: &OrderTab) -> Self {
        // let hash8 = trimHash(yt, 64);
        // let hiddentAmount = bigInt(amount).xor(hash8).value;

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

        let vlp_supply_hidden_amount;
        let vlp_supply_commitment;
        if order_tab.vlp_supply > 0 {
            // ? Hide vlp supply

            let b1 = &order_tab.tab_header.base_blinding % BigUint::from(2_u32).pow(128);
            let b2 = &order_tab.tab_header.quote_blinding % BigUint::from(2_u32).pow(128);

            let blindings_sum = &b1 + &b2;
            vlp_supply_commitment = pedersen(&BigUint::from(order_tab.vlp_supply), &blindings_sum);

            let vlp_supply_yt_digits = blindings_sum.to_u64_digits();
            let vlp_supply_yt_trimmed = if vlp_supply_yt_digits.len() == 0 {
                0
            } else {
                vlp_supply_yt_digits[0]
            };
            vlp_supply_hidden_amount = order_tab.vlp_supply ^ vlp_supply_yt_trimmed;
        } else {
            vlp_supply_hidden_amount = 0;
            vlp_supply_commitment = BigUint::zero();
        }

        return OrderTabObject {
            index: order_tab.tab_idx,
            vlp_token: order_tab.tab_header.vlp_token,
            max_vlp_supply: order_tab.tab_header.max_vlp_supply,
            is_smart_contract: order_tab.tab_header.is_smart_contract,
            base_token: order_tab.tab_header.base_token,
            quote_token: order_tab.tab_header.quote_token,
            pub_key: order_tab.tab_header.pub_key.to_string(),
            base_commitment: pedersen(
                &BigUint::from_u64(order_tab.base_amount).unwrap(),
                &order_tab.tab_header.base_blinding,
            )
            .to_string(),
            base_hidden_amount: base_hidden_amount.to_string(),
            quote_commitment: pedersen(
                &BigUint::from_u64(order_tab.quote_amount).unwrap(),
                &order_tab.tab_header.quote_blinding,
            )
            .to_string(),
            quote_hidden_amount: quote_hidden_amount.to_string(),
            vlp_supply_commitment: vlp_supply_commitment.to_string(),
            vlp_supply_hidden_amount: vlp_supply_hidden_amount.to_string(),
            hash: order_tab.hash.to_string(),
        };
    }
}

pub fn store_order_tab(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    order_tab: &OrderTab,
) {
    let obj = OrderTabObject::from_order_tab(order_tab);

    let write_path = format!("order_tabs",);
    let res = documents::write(
        session,
        write_path.as_str(),
        Some(order_tab.tab_idx.to_string()),
        &obj,
        documents::WriteOptions::default(),
    );

    if let Err(e) = res {
        println!("Error storing note in backup storage. ERROR: {:?}", e);
        let s = backup_storage.lock();
        if let Err(_e) = s.store_order_tab(order_tab) {};
        drop(s);
    }

    // ? ----------------------------------------

    let write_path = format!(
        "addr2idx/addresses/{}",
        order_tab.tab_header.pub_key.to_string()
    );
    let _res = documents::write(
        session,
        write_path.as_str(),
        Some(order_tab.tab_idx.to_string()),
        &json!({}),
        documents::WriteOptions::default(),
    );
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
