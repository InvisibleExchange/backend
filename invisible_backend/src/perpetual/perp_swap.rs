use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use super::order_execution::{
    execute_perp_swap_transaction, update_json_output, update_state_and_finalize,
};

//
use super::perp_helpers::perp_swap_helpers::consistency_checks;
use super::perp_helpers::perp_swap_outptut::{PerpSwapOutput, PerpSwapResponse};
use super::{perp_order::PerpOrder, perp_position::PerpPosition, OrderSide};
use crate::transaction_batch::tx_batch_structs::SwapFundingInfo;
use crate::transaction_batch::LeafNodeType;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::Signature;
use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::{errors::PerpSwapExecutionError, notes::Note};

use error_stack::Result;
//

#[derive(Clone, Debug)]
pub struct PerpSwap {
    pub transaction_type: String,
    pub order_a: PerpOrder, // Should be a Long order
    pub order_b: PerpOrder, // Should be a Short order
    pub signature_a: Option<Signature>,
    pub signature_b: Option<Signature>,
    pub spent_collateral: u64, // amount spent in collateral token
    pub spent_synthetic: u64,  // amount spent in synthetic token
    pub fee_taken_a: u64,      // Fee taken in collateral token
    pub fee_taken_b: u64,      // Fee taken in collateral token
}

impl PerpSwap {
    pub fn new(
        order_a: PerpOrder,
        order_b: PerpOrder,
        signature_a: Option<Signature>,
        signature_b: Option<Signature>,
        spent_collateral: u64,
        spent_synthetic: u64,
        fee_taken_a: u64,
        fee_taken_b: u64,
    ) -> PerpSwap {
        PerpSwap {
            transaction_type: String::from("perpetual_swap"),
            order_a,
            order_b,
            signature_a,
            signature_b,
            spent_collateral,
            spent_synthetic,
            fee_taken_a,
            fee_taken_b,
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
        blocked_perp_order_ids: Arc<Mutex<HashMap<u64, bool>>>,
        //
        perpetual_partial_fill_tracker: Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>, // (pfr_note, amount_filled, spent_margin)
        partialy_filled_positions: Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>, // (position, synthetic filled)
        //
        index_price: u64,
        min_funding_idxs: Arc<Mutex<HashMap<u32, u32>>>,
        swap_funding_info: SwapFundingInfo,
        //
        session: Arc<Mutex<ServiceSession>>,
        backup_storage: Arc<Mutex<BackupStorage>>,
    ) -> Result<PerpSwapResponse, PerpSwapExecutionError> {
        //

        consistency_checks(
            &self.order_a,
            &self.order_b,
            self.spent_collateral,
            self.spent_synthetic,
            self.fee_taken_a,
            self.fee_taken_b,
        )?;

        let current_funding_idx = swap_funding_info.current_funding_idx;

        // * Execute the swap transaction =================================
        let mut execution_result = execute_perp_swap_transaction(
            &state_tree,
            &blocked_perp_order_ids,
            &perpetual_partial_fill_tracker,
            &partialy_filled_positions,
            index_price,
            swap_funding_info,
            &self.order_a,
            &self.order_b,
            &self.signature_a,
            &self.signature_b,
            self.spent_synthetic,
            self.spent_collateral,
            self.fee_taken_a,
            self.fee_taken_b,
        )?;

        // ? Lock the json output before updating the state to prevent another transaction from
        // ? squeezing in between and updating the json output before this transaction is done
        let mut swap_output_json_ = swap_output_json.lock();

        // * Update the state if transaction was successful ===============
        update_state_and_finalize(
            &state_tree,
            &updated_state_hashes,
            &blocked_perp_order_ids,
            &perpetual_partial_fill_tracker,
            &partialy_filled_positions,
            &session,
            &backup_storage,
            &mut execution_result,
            &self.order_a,
            &self.order_b,
        )?;

        let execution_output_a = execution_result.0;
        let execution_output_b = execution_result.1;

        let swap_output = if self.order_a.order_side == OrderSide::Long {
            PerpSwapOutput::new(&self, &self.order_a, &self.order_b)
        } else {
            PerpSwapOutput::new(&self, &self.order_b, &self.order_a)
        };

        update_json_output(
            swap_output,
            &execution_output_a,
            &execution_output_b,
            &mut swap_output_json_,
            current_funding_idx,
            &self.order_a.order_side,
        );

        drop(swap_output_json_);

        // * Update min funding index if necessary ===========================
        let mut min_funding_idxs_m = min_funding_idxs.lock();
        let prev_min_funding_idx = min_funding_idxs_m
            .get(&self.order_a.synthetic_token)
            .unwrap();
        if std::cmp::min(
            execution_output_a.prev_funding_idx,
            execution_output_b.prev_funding_idx,
        ) < *prev_min_funding_idx
        {
            min_funding_idxs_m.insert(
                self.order_a.synthetic_token,
                std::cmp::min(
                    execution_output_a.prev_funding_idx,
                    execution_output_b.prev_funding_idx,
                ),
            );
        }
        drop(min_funding_idxs_m);

        return Ok(PerpSwapResponse {
            position_a: execution_output_a.position,
            position_b: execution_output_b.position,
            new_pfr_info_a: execution_output_a.new_pfr_info,
            new_pfr_info_b: execution_output_b.new_pfr_info,
            return_collateral_note_a: execution_output_a.return_collateral_note,
            return_collateral_note_b: execution_output_b.return_collateral_note,
        });
    }

    //
}

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for PerpSwap {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut perp_swap = serializer.serialize_struct("PerpSwap", 9)?;

        if self.order_a.order_side == OrderSide::Long {
            perp_swap.serialize_field("signature_a", &self.signature_a)?;
            perp_swap.serialize_field("signature_b", &self.signature_b)?;
            perp_swap.serialize_field("spent_collateral", &self.spent_collateral)?;
            perp_swap.serialize_field("spent_synthetic", &self.spent_synthetic)?;
            perp_swap.serialize_field("fee_taken_a", &self.fee_taken_a)?;
            perp_swap.serialize_field("fee_taken_b", &self.fee_taken_b)?;
        } else {
            perp_swap.serialize_field("signature_b", &self.signature_a)?;
            perp_swap.serialize_field("signature_a", &self.signature_b)?;
            perp_swap.serialize_field("spent_collateral", &self.spent_collateral)?;
            perp_swap.serialize_field("spent_synthetic", &self.spent_synthetic)?;
            perp_swap.serialize_field("fee_taken_b", &self.fee_taken_a)?;
            perp_swap.serialize_field("fee_taken_a", &self.fee_taken_b)?;
        }

        return perp_swap.end();
    }
}
