use std::str::FromStr;
use std::sync::Arc;

use crate::order_tab::OrderTab;
use crate::utils::crypto_utils::{hash_many, verify, EcPoint, Signature};
use crate::utils::errors::{send_swap_error, SwapThreadExecutionError};

use error_stack::Result;
use num_bigint::{BigInt, BigUint};
use num_traits::{FromPrimitive, Zero};
use parking_lot::Mutex;
use starknet::curve::AffinePoint;

//
use crate::utils::notes::Note;
//

#[derive(Debug, Clone)]
pub struct LimitOrder {
    pub order_id: u64,
    pub expiration_timestamp: u64,
    pub token_spent: u32,
    pub token_received: u32,
    pub amount_spent: u64,
    pub amount_received: u64,
    pub fee_limit: u64,
    //
    pub spot_note_info: Option<SpotNotesInfo>,
    pub order_tab: Option<Arc<Mutex<OrderTab>>>,
    //
    pub hash: BigUint,
}

impl LimitOrder {
    pub fn new(
        order_id: u64,
        expiration_timestamp: u64,
        token_spent: u32,
        token_received: u32,
        amount_spent: u64,
        amount_received: u64,
        fee_limit: u64,
        spot_note_info: Option<SpotNotesInfo>,
        order_tab: Option<Arc<Mutex<OrderTab>>>,
    ) -> LimitOrder {
        let hash = hash_order(
            expiration_timestamp,
            token_spent,
            token_received,
            amount_spent,
            amount_received,
            fee_limit,
            &spot_note_info,
            &order_tab,
        );

        LimitOrder {
            order_id,
            expiration_timestamp,
            token_spent,
            token_received,
            amount_spent,
            amount_received,
            fee_limit,
            spot_note_info,
            order_tab,
            hash,
        }
    }

    pub fn set_hash(&mut self) {
        let hash = hash_order(
            self.expiration_timestamp,
            self.token_spent,
            self.token_received,
            self.amount_spent,
            self.amount_received,
            self.fee_limit,
            &self.spot_note_info,
            &self.order_tab,
        );

        self.hash = hash;
    }

    pub fn verify_order_signature(
        &self,
        signature: &Signature,
        order_tab: &Option<OrderTab>,
    ) -> Result<(), SwapThreadExecutionError> {
        let order_hash = &self.hash;
        let pub_key: BigUint;

        if order_tab.is_some() {
            let order_tab = order_tab.as_ref().unwrap();
            pub_key = order_tab.tab_header.pub_key.clone();
        } else if self.spot_note_info.is_some() {
            let mut pub_key_sum: AffinePoint = AffinePoint::identity();
            for note in self.spot_note_info.as_ref().unwrap().notes_in.iter() {
                let ec_point = AffinePoint::from(&note.address);

                pub_key_sum = &pub_key_sum + &ec_point;
            }

            pub_key = EcPoint::from(&pub_key_sum).x.to_biguint().unwrap();
        } else {
            return Err(send_swap_error(
                "Limit Order not defined properly".to_string(),
                Some(self.order_id),
                None,
            ));
        }

        let valid = verify(&pub_key, &order_hash, &signature);

        if valid {
            return Ok(());
        } else {
            return Err(send_swap_error(
                "Invalid Signature".to_string(),
                Some(self.order_id),
                Some(format!(
                    "Invalid signature: r:{:?} s:{:?} hash:{:?} pub_key:{:?}",
                    &signature.r, &signature.s, order_hash, pub_key
                )),
            ));
        }
    }
}

fn hash_order(
    expiration_timestamp: u64,
    token_spent: u32,
    token_received: u32,
    amount_spent: u64,
    amount_received: u64,
    fee_limit: u64,
    spot_note_info: &Option<SpotNotesInfo>,
    order_tab: &Option<Arc<Mutex<OrderTab>>>,
) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    // & H({expiration_timestamp, token_spent, token_received, amount_spent, amount_received, fee_limit, note_info_hash, order_tab_pub_key})

    let expiration_timestamp = BigUint::from_u64(expiration_timestamp).unwrap();
    hash_inputs.push(&expiration_timestamp);
    let token_spent = BigUint::from_u32(token_spent).unwrap();
    hash_inputs.push(&token_spent);
    let token_received = BigUint::from_u32(token_received).unwrap();
    hash_inputs.push(&token_received);
    let amount_spent = BigUint::from_u64(amount_spent).unwrap();
    hash_inputs.push(&amount_spent);
    let amount_received = BigUint::from_u64(amount_received).unwrap();
    hash_inputs.push(&amount_received);
    let fee_limit = BigUint::from_u64(fee_limit).unwrap();
    hash_inputs.push(&fee_limit);

    if spot_note_info.is_some() {
        let note_info_hash = spot_note_info.as_ref().unwrap().hash();
        hash_inputs.push(&note_info_hash);

        let order_hash = hash_many(&hash_inputs);

        return order_hash;
    } else {
        let tab = order_tab.as_ref().unwrap().lock();
        let tab_pub_key = tab.tab_header.pub_key.clone();

        hash_inputs.push(&tab_pub_key);

        let order_hash = hash_many(&hash_inputs);

        return order_hash;
    }
}

#[derive(Debug, Clone)]
/// This struct is used for normal limit orders, market makers use OrderTabs
pub struct SpotNotesInfo {
    pub dest_received_address: EcPoint,
    pub dest_received_blinding: BigUint,
    pub notes_in: Vec<Note>,
    pub refund_note: Option<Note>,
}

impl SpotNotesInfo {
    fn hash(&self) -> BigUint {
        let note_hashes: Vec<&BigUint> = self
            .notes_in
            .iter()
            .map(|note| &note.hash)
            .collect::<Vec<&BigUint>>();

        let z: BigUint = BigUint::zero();
        let refund_hash: &BigUint;
        if self.refund_note.is_some() {
            refund_hash = &self.refund_note.as_ref().unwrap().hash
        } else {
            refund_hash = &z;
        };

        let mut hash_inputs: Vec<&BigUint> = Vec::new();
        for n_hash in note_hashes {
            hash_inputs.push(n_hash);
        }
        hash_inputs.push(refund_hash);

        let dra_x = self.dest_received_address.x.to_biguint().unwrap();
        hash_inputs.push(&dra_x);
        hash_inputs.push(&self.dest_received_blinding);

        let order_hash = hash_many(&hash_inputs);

        return order_hash;
    }
}

// * SERIALIZE * //
use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for SpotNotesInfo {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("LimitOrder", 13)?;

        // note.serialize_field("dest_spent_address", &self.dest_spent_address)?;
        note.serialize_field("dest_received_address", &self.dest_received_address)?;
        note.serialize_field(
            "dest_received_blinding",
            &self.dest_received_blinding.to_string(),
        )?;
        note.serialize_field("notes_in", &self.notes_in)?;
        note.serialize_field("refund_note", &self.refund_note)?;

        return note.end();
    }
}

// * DESERIALIZE * //
use serde::de::{Deserialize, Deserializer};
use serde::Deserialize as DeserializeTrait;

impl<'de> Deserialize<'de> for SpotNotesInfo {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Addr {
            x: String,
            y: String,
        }

        #[derive(DeserializeTrait)]
        struct Helper {
            dest_received_address: Addr,
            dest_received_blinding: String,
            notes_in: Vec<Note>,
            refund_note: Option<Note>,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(SpotNotesInfo {
            dest_received_address: EcPoint {
                x: BigInt::from_str(&helper.dest_received_address.x).unwrap(),
                y: BigInt::from_str(&helper.dest_received_address.y).unwrap(),
            },
            dest_received_blinding: BigUint::from_str(&helper.dest_received_blinding).unwrap(),
            notes_in: helper.notes_in,
            refund_note: helper.refund_note,
        })
    }
}

// ====================================================================================================

impl Serialize for LimitOrder {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("LimitOrder", 13)?;

        note.serialize_field("order_id", &self.order_id)?;
        note.serialize_field("expiration_timestamp", &self.expiration_timestamp)?;
        note.serialize_field("token_spent", &self.token_spent)?;
        note.serialize_field("token_received", &self.token_received)?;
        note.serialize_field("amount_spent", &self.amount_spent)?;
        note.serialize_field("amount_received", &self.amount_received)?;
        note.serialize_field("fee_limit", &self.fee_limit)?;
        //
        note.serialize_field("spot_note_info", &self.spot_note_info)?;
        //
        let hash: &BigUint = &self.hash;
        note.serialize_field("hash", &hash.to_string())?;

        return note.end();
    }
}

impl<'de> Deserialize<'de> for LimitOrder {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(DeserializeTrait)]
        struct Helper {
            order_id: u64,
            expiration_timestamp: u64,
            token_spent: u32,
            token_received: u32,
            amount_spent: u64,
            amount_received: u64,
            fee_limit: u64,
            spot_note_info: Option<SpotNotesInfo>,
            order_tab: Option<OrderTab>,
            hash: String,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(LimitOrder {
            order_id: helper.order_id,
            expiration_timestamp: helper.expiration_timestamp,
            token_spent: helper.token_spent,
            token_received: helper.token_received,
            amount_spent: helper.amount_spent,
            amount_received: helper.amount_received,
            fee_limit: helper.fee_limit,
            spot_note_info: helper.spot_note_info,
            order_tab: helper.order_tab.map(|x| Arc::new(Mutex::new(x))),
            hash: BigUint::from_str(&helper.hash).unwrap(),
        })
    }
}
