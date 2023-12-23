use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use num_bigint::BigUint;
use serde_json::Value;

use error_stack::{Report, Result};

use super::Transaction;
//
use super::limit_order::LimitOrder;
use super::swap_execution::{
    execute_swap_transaction, update_json_output, update_state_and_finalize,
};
use super::transaction_helpers::swap_helpers::{consistency_checks, NoteInfoExecutionOutput};
use super::transaction_helpers::transaction_output::TransactionOutptut;
use crate::transaction_batch::LeafNodeType;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::Signature;
use crate::utils::errors::{SwapThreadExecutionError, TransactionExecutionError};
use crate::utils::notes::Note;
use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::storage::local_storage::MainStorage;

#[derive(Debug)]
pub struct Swap {
    pub transaction_type: String,
    pub order_a: LimitOrder,
    pub order_b: LimitOrder,
    pub signature_a: Signature,
    pub signature_b: Signature,
    pub spent_amount_a: u64,
    pub spent_amount_b: u64,
    pub fee_taken_a: u64,
    pub fee_taken_b: u64,
}

impl Swap {
    pub fn new(
        order_a: LimitOrder,
        order_b: LimitOrder,
        signature_a: Signature,
        signature_b: Signature,
        spent_amount_a: u64,
        spent_amount_b: u64,
        fee_taken_a: u64,
        fee_taken_b: u64,
    ) -> Swap {
        Swap {
            transaction_type: "swap".to_string(),
            order_a,
            order_b,
            signature_a,
            signature_b,
            spent_amount_a,
            spent_amount_b,
            fee_taken_a,
            fee_taken_b,
        }
    }

    // & batch_init_tree is the state tree at the beginning of the batch
    // & tree is the current state tree
    // & partial_fill_tracker is a map of indexes to partial fill refund notes
    // & updatedNoteHashes is a map of {index: (leaf_hash, proof, proofPos)}
    fn execute_swap(
        &self,
        tree_m: Arc<Mutex<SuperficialTree>>,
        partial_fill_tracker_m: Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
        updated_state_hashes_m: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
        swap_output_json_m: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        blocked_order_ids_m: Arc<Mutex<HashMap<u64, bool>>>,
        session: &Arc<Mutex<ServiceSession>>,
        backup_storage: &Arc<Mutex<BackupStorage>>,
    ) -> Result<SwapResponse, SwapThreadExecutionError> {
        //

        // ? Verify swap consistencies
        consistency_checks(
            &self.order_a,
            &self.order_b,
            self.spent_amount_a,
            self.spent_amount_b,
            self.fee_taken_a,
            self.fee_taken_b,
        )?;

        // ? Lock the order tabs and get the order tab mutexes
        let mut order_tab_mutex_a = None;
        let mut order_tab_a = None;
        if let Some(tab_mutex) = self.order_a.order_tab.as_ref() {
            let tab_lock = tab_mutex.lock();

            order_tab_a = Some(tab_lock.clone());
            order_tab_mutex_a = Some(tab_lock);
        }
        let prev_order_tab_a = order_tab_a.clone();

        let mut order_tab_mutex_b = None;
        let mut order_tab_b = None;
        if let Some(tab_mutex) = self.order_b.order_tab.as_ref() {
            let tab_lock = tab_mutex.lock();

            order_tab_b = Some(tab_lock.clone());
            order_tab_mutex_b = Some(tab_lock);
        }
        let prev_order_tab_b = order_tab_b.clone();

        // * Execute the swap transaction =================================
        let execution_result = execute_swap_transaction(
            &tree_m,
            &partial_fill_tracker_m,
            &blocked_order_ids_m,
            &self.order_a,
            &self.order_b,
            order_tab_a,
            order_tab_b,
            &self.signature_a,
            &self.signature_b,
            self.spent_amount_a,
            self.spent_amount_b,
            self.fee_taken_a,
            self.fee_taken_b,
        )?;

        // ? Lock the json output before updating the state to prevent another transaction from
        // ? squeezing in between and updating the json output before this transaction is done
        let mut swap_output_json = swap_output_json_m.lock();

        // * Update the state if transaction was successful ===============
        update_state_and_finalize(
            &tree_m,
            &partial_fill_tracker_m,
            &updated_state_hashes_m,
            &blocked_order_ids_m,
            session,
            backup_storage,
            &execution_result,
            &self.order_a,
            &self.order_b,
            &prev_order_tab_a,
            &prev_order_tab_b,
        )?;

        // * Construct and store the JSON Output ===========================
        let swap_output = TransactionOutptut::new(&self);

        let execution_output_a = execution_result.0;
        let execution_output_b = execution_result.1;

        update_json_output(
            &mut swap_output_json,
            &swap_output,
            &execution_output_a,
            &execution_output_b,
            &prev_order_tab_a,
            &prev_order_tab_b,
        );

        drop(swap_output_json);

        // * Update the mutex order tabs and release the locks
        if let Some(mut order_tab_mutex) = order_tab_mutex_a {
            *order_tab_mutex = execution_output_a
                .updated_order_tab
                .as_ref()
                .unwrap()
                .clone();
        }
        if let Some(mut order_tab_mutex) = order_tab_mutex_b {
            *order_tab_mutex = execution_output_b
                .updated_order_tab
                .as_ref()
                .unwrap()
                .clone();
        }

        // * Return the swap result -----------------------------------

        return Ok(SwapResponse::new(
            &execution_output_a.note_info_output,
            execution_output_a.new_amount_filled,
            &execution_output_b.note_info_output,
            execution_output_b.new_amount_filled,
            self.spent_amount_a,
            self.spent_amount_b,
        ));
    }
}

// * IMPL TRANSACTION TRAIT * //

impl Transaction for Swap {
    fn transaction_type(&self) -> &str {
        return self.transaction_type.as_str();
    }

    fn execute_transaction(
        &mut self,
        tree_m: Arc<Mutex<SuperficialTree>>,
        partial_fill_tracker_m: Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
        updated_state_hashes_m: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
        swap_output_json_m: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        blocked_order_ids_m: Arc<Mutex<HashMap<u64, bool>>>,
        session: &Arc<Mutex<ServiceSession>>,
        _main_storage: &Arc<Mutex<MainStorage>>,
        backup_storage: &Arc<Mutex<BackupStorage>>,
    ) -> Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError> {
        let swap_response = self
            .execute_swap(
                tree_m,
                partial_fill_tracker_m,
                updated_state_hashes_m,
                swap_output_json_m,
                blocked_order_ids_m,
                session,
                backup_storage,
            )
            .or_else(|err: Report<SwapThreadExecutionError>| {
                let error_context = err.current_context().clone();
                Err(
                    Report::new(TransactionExecutionError::Swap(error_context.clone()))
                        .attach_printable(format!("Error executing swap: {}", error_context)),
                )
            })?;

        return Ok((Some(swap_response), None));
    }
}

// * SERIALIZE SWAP * //

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for Swap {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("PerpSwap", 9)?;

        note.serialize_field("order_a", &self.order_a)?;
        note.serialize_field("order_b", &self.order_b)?;
        note.serialize_field("signature_a", &self.signature_a)?;
        note.serialize_field("signature_b", &self.signature_b)?;
        note.serialize_field("spent_amount_a", &self.spent_amount_a)?;
        note.serialize_field("spent_amount_b", &self.spent_amount_b)?;
        note.serialize_field("fee_taken_a", &self.fee_taken_a)?;
        note.serialize_field("fee_taken_b", &self.fee_taken_b)?;

        return note.end();
    }
}

// * SWAP RESPONSE STRUCT * //

#[derive(Debug, Clone, serde::Serialize)]
pub struct SwapResponse {
    pub note_info_swap_response_a: Option<NoteInfoSwapResponse>,
    pub note_info_swap_response_b: Option<NoteInfoSwapResponse>,
    pub spent_amount_a: u64,
    pub spent_amount_b: u64,
}

impl SwapResponse {
    fn new(
        note_info_output_a: &Option<NoteInfoExecutionOutput>,
        new_amount_filled_a: u64,
        note_info_output_b: &Option<NoteInfoExecutionOutput>,
        new_amount_filled_b: u64,
        spent_amount_a: u64,
        spent_amount_b: u64,
    ) -> SwapResponse {
        // note info response a
        let mut note_info_swap_response_a = None;
        if let Some(output) = note_info_output_a {
            let mut new_pfr_note = None;
            if let Some((pfr_note_, _)) = output.new_partial_fill_info.as_ref() {
                new_pfr_note = Some(pfr_note_.as_ref().unwrap().clone());
            }

            note_info_swap_response_a = Some(NoteInfoSwapResponse {
                swap_note: output.swap_note.clone(),
                new_pfr_note,
                new_amount_filled: new_amount_filled_a,
            });
        }

        // note info response b
        let mut note_info_swap_response_b = None;
        if let Some(output) = note_info_output_b {
            let mut new_pfr_note = None;
            if let Some((pfr_note_, _)) = output.new_partial_fill_info.as_ref() {
                new_pfr_note = Some(pfr_note_.as_ref().unwrap().clone());
            }

            note_info_swap_response_b = Some(NoteInfoSwapResponse {
                swap_note: output.swap_note.clone(),
                new_pfr_note,
                new_amount_filled: new_amount_filled_b,
            });
        }

        SwapResponse {
            note_info_swap_response_a,
            note_info_swap_response_b,
            spent_amount_a,
            spent_amount_b,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NoteInfoSwapResponse {
    pub swap_note: Note,
    pub new_pfr_note: Option<Note>,
    pub new_amount_filled: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OrderFillResponse {
    pub note_info_swap_response: Option<NoteInfoSwapResponse>,
    pub fee_taken: u64,
}

impl OrderFillResponse {
    pub fn from_swap_response(req: &SwapResponse, fee_taken: u64, is_a: bool) -> Self {
        if is_a {
            return OrderFillResponse {
                note_info_swap_response: req.note_info_swap_response_a.clone(),
                fee_taken,
            };
        } else {
            return OrderFillResponse {
                note_info_swap_response: req.note_info_swap_response_b.clone(),
                fee_taken,
            };
        }
    }
}
