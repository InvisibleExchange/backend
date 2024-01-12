use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crossbeam::thread;

use super::db_updates::update_db_after_liquidation_swap;
use super::execute_liquidations::{
    execute_liquidation, open_new_position_after_liquidation, verify_position_existence,
};
use super::liquidation_order::LiquidationOrder;
use super::liquidation_output::{wrap_liquidation_output, LiquidationResponse};
use super::state_updates::{
    update_perpetual_state_after_liquidation, update_state_after_liquidation,
};
use crate::perpetual::perp_position::PerpPosition;
use crate::transaction_batch::tx_batch_structs::SwapFundingInfo;
use crate::transaction_batch::LeafNodeType;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::Signature;
use crate::utils::errors::{send_perp_swap_error, PerpSwapExecutionError};
use crate::utils::storage::backup_storage::BackupStorage;

use error_stack::{Report, Result};
//

// TODO: DO SOMETHING WITH LEFTOVER MARGIN IN 000 SITUATIONS

#[derive(Clone, Debug)]
pub struct LiquidationSwap {
    pub transaction_type: String,
    pub liquidation_order: LiquidationOrder,
    pub signature: Signature,
    pub market_price: u64,
}

impl LiquidationSwap {
    pub fn new(
        liquidation_order: LiquidationOrder,
        signature: Signature,
        market_price: u64,
    ) -> LiquidationSwap {
        LiquidationSwap {
            transaction_type: String::from("liquidation_swap"),
            liquidation_order,
            signature,
            market_price,
        }
    }

    // & order a should be a Long order, & order b should be a Short order
    // & order a (Long) is swapping collateral for synthetic tokens
    // & order b (Short) is swapping synthetic tokens for collateral
    pub fn execute(
        &self,
        state_tree: Arc<Mutex<SuperficialTree>>,
        updated_state_hashes: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
        swap_output_json: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        //
        insurance_fund: Arc<Mutex<i64>>,
        //
        index_price: u64,
        min_funding_idxs: Arc<Mutex<HashMap<u32, u32>>>,
        swap_funding_info: SwapFundingInfo,
        //
        session: Arc<Mutex<ServiceSession>>,
        backup_storage: Arc<Mutex<BackupStorage>>,
    ) -> Result<LiquidationResponse, PerpSwapExecutionError> {
        //

        // TODO: This is only for testing purposes -- remove after
        let index_price = self.market_price;
        // TODO: This is only for testing purposes -- remove after

        // ? Execute orders in parallel ===========================================================

        let current_funding_idx = swap_funding_info.current_funding_idx;

        let (liquidated_position, new_position) = thread::scope(move |_| {
            let mut liquidated_position = self.liquidation_order.position.clone();

            // ? Verify the position hash is valid and exists in the state
            verify_position_existence(&state_tree, &liquidated_position)?;

            // ? Verify the signature
            self.liquidation_order
                .verify_order_signature(&self.signature)?;

            let (liquidated_size, liquidator_fee, leftover_collateral, is_partial_liquidation) =
                execute_liquidation(
                    self.market_price,
                    index_price,
                    &swap_funding_info,
                    &self.liquidation_order,
                    &mut liquidated_position,
                )?;

            println!("liquidated_size: {}", liquidated_size);
            println!("liquidator_fee: {}", liquidator_fee);
            println!("leftover_collateral: {}", leftover_collateral);
            println!("is_partial_liquidation: {}", is_partial_liquidation);

            let new_idx = state_tree.lock().first_zero_idx() as u32;


            let new_position = open_new_position_after_liquidation(
                &self.liquidation_order,
                liquidated_size,
                liquidator_fee,
                self.market_price,
                current_funding_idx,
                new_idx,
            )?;

            let liquidated_position: Option<PerpPosition> = if is_partial_liquidation {
                Some(liquidated_position)
            } else {
                None
            };

            // * UPDATE STATE AFTER SWAP ——————————————————————————————————————————

            let mut insurance_fund_m = insurance_fund.lock();
            let fund_amount: &mut i64 = &mut insurance_fund_m;
            *fund_amount += leftover_collateral;
            drop(insurance_fund_m);

            update_state_after_liquidation(
                &state_tree,
                &updated_state_hashes,
                &self.liquidation_order.open_order_fields.notes_in,
                &self.liquidation_order.open_order_fields.refund_note,
            )?;

            update_perpetual_state_after_liquidation(
                &state_tree,
                &updated_state_hashes,
                self.liquidation_order.position.index,
                &liquidated_position,
                &new_position,
            )?;

            Ok((liquidated_position, new_position))
        })
        .or_else(|e| {
            Err(send_perp_swap_error(
                "Unknown Error Occurred".to_string(),
                None,
                Some(format!("error occurred executing perp swap:  {:?}", e)),
            ))
        })?
        .or_else(|err: Report<PerpSwapExecutionError>| Err(err))?;

        //

        //

        // * set new min funding index if necessary (for cairo input ) -------------------------
        let mut min_funding_idxs_m = min_funding_idxs.lock();
        let prev_min_funding_idx = min_funding_idxs_m
            .get(&self.liquidation_order.synthetic_token)
            .unwrap();
        if current_funding_idx < *prev_min_funding_idx {
            min_funding_idxs_m.insert(self.liquidation_order.synthetic_token, current_funding_idx);
        }
        drop(min_funding_idxs_m);

        // * Write the swap output to json to be used as input to the cairo program ——————————————

        let new_liquidated_position_hash = if liquidated_position.is_some() {
            Some(liquidated_position.as_ref().unwrap().hash.to_string())
        } else {
            None
        };
        let new_position_hash = new_position.hash.to_string();

        let json_output = wrap_liquidation_output(
            &self.liquidation_order,
            &self.signature,
            &new_liquidated_position_hash,
            &new_position_hash,
            new_position.index,
            self.liquidation_order.position.last_funding_idx,
            current_funding_idx,
            self.market_price,
            index_price,
        );

        let mut swap_output_json_m = swap_output_json.lock();
        swap_output_json_m.push(json_output);
        drop(swap_output_json_m);

        // ? Update the database
        update_db_after_liquidation_swap(
            &session,
            &backup_storage,
            &self.liquidation_order,
            &liquidated_position,
            &new_position,
        );

        println!("liquidation swap executed successfully");

        return Ok(LiquidationResponse {
            liquidated_position_index: self.liquidation_order.position.index,
            liquidated_position_address: self
                .liquidation_order
                .position
                .position_header
                .position_address
                .clone(),
            liquidated_position,
            new_position,
        });
    }

    //
}
