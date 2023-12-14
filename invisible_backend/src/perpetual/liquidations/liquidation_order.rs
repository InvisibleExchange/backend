use error_stack::Result;
use num_bigint::BigUint;
use num_traits::{FromPrimitive, One, Zero};
use starknet::curve::AffinePoint;

use crate::perpetual::perp_order::OpenOrderFields;
//
use crate::utils::errors::{send_perp_swap_error, PerpSwapExecutionError};

use super::super::perp_position::PerpPosition;
use crate::perpetual::OrderSide;
use crate::utils::crypto_utils::{hash_many, verify, EcPoint, Signature};

#[derive(Debug, Clone)]
pub struct LiquidationOrder {
    pub position: PerpPosition,
    pub order_side: OrderSide,
    pub synthetic_token: u32,
    pub synthetic_amount: u64,
    pub collateral_amount: u64,
    // You need to open a new position to liquidate the previous one
    pub open_order_fields: OpenOrderFields,
    //
    pub hash: BigUint,
}

impl LiquidationOrder {
    pub fn new(
        position: PerpPosition,
        order_side: OrderSide,
        synthetic_token: u32,
        synthetic_amount: u64,
        collateral_amount: u64,
        open_order_fields: OpenOrderFields,
    ) -> LiquidationOrder {
        let hash = hash_order(
            &order_side,
            synthetic_token,
            synthetic_amount,
            collateral_amount,
            &position,
            &open_order_fields,
        );

        return LiquidationOrder {
            position,
            order_side,
            synthetic_token,
            synthetic_amount,
            collateral_amount,
            open_order_fields,
            hash,
        };
    }

    pub fn verify_order_signature(
        &self,
        signature: &Signature,
    ) -> Result<Option<EcPoint>, PerpSwapExecutionError> {
        let order_hash = &self.hash;

        let mut pub_key_sum: AffinePoint = AffinePoint::identity();

        for i in 0..self.open_order_fields.notes_in.len() {
            let ec_point = AffinePoint::from(&self.open_order_fields.notes_in[i].address);
            pub_key_sum = &pub_key_sum + &ec_point;
        }

        let pub_key: EcPoint = EcPoint::from(&pub_key_sum);

        let valid = verify(&pub_key.x.to_biguint().unwrap(), &order_hash, &signature);

        if valid {
            return Ok(Some(pub_key));
        } else {
            return Err(send_perp_swap_error(
                "Invalid Signature".to_string(),
                None,
                Some(format!(
                    "Invalid signature: r:{:?} s:{:?} hash:{:?} pub_key:{:?}",
                    &signature.r, &signature.s, order_hash, pub_key
                )),
            ));
        }
    }
}

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for LiquidationOrder {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("PerpOrder", 13)?;

        note.serialize_field("position", &self.position)?;
        note.serialize_field("order_side", &self.order_side)?;
        note.serialize_field("synthetic_token", &self.synthetic_token)?;
        note.serialize_field("synthetic_amount", &self.synthetic_amount)?;
        note.serialize_field("collateral_amount", &self.collateral_amount)?;
        note.serialize_field("open_order_fields", &self.open_order_fields)?;
        let hash: &BigUint = &self.hash;
        note.serialize_field("hash", &hash.to_string())?;

        return note.end();
    }
}

//

//

//

fn hash_order(
    order_side: &OrderSide,
    synthetic_token: u32,
    synthetic_amount: u64,
    collateral_amount: u64,
    position: &PerpPosition,
    open_order_fields: &OpenOrderFields,
) -> BigUint {
    let mut hash_inputs: Vec<&BigUint> = Vec::new();

    let pos_addr_string = &position.position_header.position_address;
    hash_inputs.push(pos_addr_string);

    let order_side: BigUint = if *order_side == OrderSide::Long {
        BigUint::one()
    } else {
        BigUint::zero()
    };
    hash_inputs.push(&order_side);

    let synthetic_token = BigUint::from_u32(synthetic_token).unwrap();
    hash_inputs.push(&synthetic_token);
    let synthetic_amount = BigUint::from_u64(synthetic_amount).unwrap();
    hash_inputs.push(&synthetic_amount);
    let collateral_amount = BigUint::from_u64(collateral_amount).unwrap();
    hash_inputs.push(&collateral_amount);

    let fields_hash = open_order_fields.hash();
    hash_inputs.push(&fields_hash);

    let order_hash = hash_many(&hash_inputs);

    return order_hash;
}
