use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use num_bigint::BigUint;
use serde_json::Value;

use crossbeam::thread;
use error_stack::{Report, Result};

use super::Transaction;
//
use super::limit_order::LimitOrder;
use super::swap_execution::{execute_order, reverify_existances, update_state_after_order};
use super::transaction_helpers::db_updates::update_db_after_spot_swap;
use super::transaction_helpers::swap_helpers::{
    consistency_checks, finalize_updates, unblock_order, NoteInfoExecutionOutput,
    TxExecutionThreadOutput,
};
use super::transaction_helpers::transaction_output::TransactionOutptut;
use crate::transaction_batch::LeafNodeType;
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::Signature;
use crate::utils::errors::{send_swap_error, SwapThreadExecutionError, TransactionExecutionError};
use crate::utils::notes::Note;
use crate::utils::storage::local_storage::{BackupStorage, MainStorage};

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
        let prev_order_tab_a2 = order_tab_a.clone();

        let mut order_tab_mutex_b = None;
        let mut order_tab_b = None;
        if let Some(tab_mutex) = self.order_b.order_tab.as_ref() {
            let tab_lock = tab_mutex.lock();

            order_tab_b = Some(tab_lock.clone());
            order_tab_mutex_b = Some(tab_lock);
        }
        let prev_order_tab_b = order_tab_b.clone();
        let prev_order_tab_b2 = order_tab_b.clone();

        let blocked_order_ids_c = blocked_order_ids_m.clone();

        // * Execute the swap in a thread scope ===============================================================

        let swap_execution_handle = thread::scope(move |s| {
            let tree = tree_m.clone();
            let partial_fill_tracker = partial_fill_tracker_m.clone();
            let blocked_order_ids = blocked_order_ids_m.clone();

            let order_handle_a = s.spawn(move |_| {
                // ? Exececute order a -----------------------------------------------------

                let execution_output: TxExecutionThreadOutput;

                let (is_partially_filled, note_info_output, updated_order_tab, new_amount_filled) =
                    execute_order(
                        &tree,
                        &partial_fill_tracker,
                        &blocked_order_ids,
                        &self.order_a,
                        order_tab_a,
                        &self.signature_a,
                        self.spent_amount_a,
                        self.spent_amount_b,
                        self.fee_taken_a,
                    )?;

                execution_output = TxExecutionThreadOutput {
                    is_partially_filled,
                    note_info_output,
                    updated_order_tab,
                    new_amount_filled,
                };

                return Ok(execution_output);
            });

            let tree = tree_m.clone();
            let partial_fill_tracker = partial_fill_tracker_m.clone();
            let blocked_order_ids = blocked_order_ids_m.clone();

            let order_handle_b = s.spawn(move |_| {
                // ? Exececute order b -----------------------------------------------------

                let execution_output: TxExecutionThreadOutput;

                let (is_partially_filled, note_info_output, updated_order_tab, new_amount_filled) =
                    execute_order(
                        &tree,
                        &partial_fill_tracker,
                        &blocked_order_ids,
                        &self.order_b,
                        order_tab_b,
                        &self.signature_b,
                        self.spent_amount_b,
                        self.spent_amount_a,
                        self.fee_taken_b,
                    )?;

                execution_output = TxExecutionThreadOutput {
                    is_partially_filled,
                    note_info_output,
                    updated_order_tab,
                    new_amount_filled,
                };

                return Ok(execution_output);
            });

            // ? Get the result of thread_a execution or return an error
            let order_a_output = order_handle_a
                .join()
                .or_else(|_| {
                    // ? Un unknown error occured executing order a thread
                    Err(send_swap_error(
                        "Unknow Error Occured".to_string(),
                        None,
                        None,
                    ))
                })?
                .or_else(|err: Report<SwapThreadExecutionError>| {
                    // ? An error occured executing order a thread
                    Err(err)
                })?;

            // ? Get the result of thread_b execution or return an error
            let order_b_output = order_handle_b
                .join()
                .or_else(|_| {
                    // ? Un unknown error occured executing order a thread
                    Err(send_swap_error(
                        "Unknow Error Occured".to_string(),
                        None,
                        None,
                    ))
                })?
                .or_else(|err: Report<SwapThreadExecutionError>| {
                    // ? An error occured executing order a thread
                    Err(err)
                })?;

            // * AFTER BOTH orders have been verified successfully update the state —————————————————————————————————————
            reverify_existances(
                &tree_m,
                &self.order_a,
                &prev_order_tab_a,
                &order_a_output.note_info_output,
                &self.order_b,
                &prev_order_tab_b,
                &order_b_output.note_info_output,
            )?;

            // ? Order a ----------------------------------------
            let tree = tree_m.clone();
            let updated_state_hashes = updated_state_hashes_m.clone();
            let partial_fill_tracker = partial_fill_tracker_m.clone();
            let blocked_order_ids = blocked_order_ids_m.clone();

            let order_a_output_clone = order_a_output.clone();

            let update_state_handle_a = s.spawn(move |_| {
                update_state_after_order(
                    &tree,
                    &updated_state_hashes,
                    &self.order_a.spot_note_info,
                    &order_a_output_clone.note_info_output,
                    &order_a_output_clone.updated_order_tab,
                );

                update_db_after_spot_swap(
                    &session,
                    &backup_storage,
                    &self.order_a,
                    &order_a_output_clone.note_info_output,
                    &order_a_output_clone.updated_order_tab,
                );

                // ? update the  partial_fill_tracker map and allow other threads to continue filling the same order
                finalize_updates(
                    &partial_fill_tracker,
                    &blocked_order_ids,
                    self.order_a.order_id,
                    prev_order_tab_a.is_some(),
                    &order_a_output_clone,
                );

                Ok(())
            });

            // ? Order b ----------------------------------------
            let tree = tree_m.clone();
            let updated_state_hashes = updated_state_hashes_m.clone();
            let partial_fill_tracker = partial_fill_tracker_m.clone();
            let blocked_order_ids = blocked_order_ids_m.clone();
            let order_b_output_clone = order_b_output.clone();

            let update_state_handle_b = s.spawn(move |_| {
                update_state_after_order(
                    &tree,
                    &updated_state_hashes,
                    &self.order_b.spot_note_info,
                    &order_b_output_clone.note_info_output,
                    &order_b_output_clone.updated_order_tab,
                );

                update_db_after_spot_swap(
                    &session,
                    &backup_storage,
                    &self.order_b,
                    &order_b_output_clone.note_info_output,
                    &order_b_output_clone.updated_order_tab,
                );

                // ? update the  partial_fill_tracker map and allow other threads to continue filling the same order
                finalize_updates(
                    &partial_fill_tracker,
                    &blocked_order_ids,
                    self.order_b.order_id,
                    prev_order_tab_b.is_some(),
                    &order_b_output_clone,
                );

                Ok(())
            });

            // ? Run the update state thread_a or return an error
            update_state_handle_a
                .join()
                .or_else(|_| {
                    // ? Un unknown error occured executing order a thread
                    Err(send_swap_error(
                        "Unknow Error Occured".to_string(),
                        None,
                        None,
                    ))
                })?
                .or_else(|err: Report<SwapThreadExecutionError>| {
                    // ? An error occured executing order a thread
                    Err(err)
                })?;

            // ? Run the update state thread_b or return an error
            update_state_handle_b
                .join()
                .or_else(|e| {
                    // ? Un unknown error occured executing order a thread
                    Err(send_swap_error(
                        "Unknow Error Occured".to_string(),
                        None,
                        Some(format!("error occured executing spot swap:  {:?}", e)),
                    ))
                })?
                .or_else(|err: Report<SwapThreadExecutionError>| {
                    // ? An error occured executing order a thread
                    Err(err)
                })?;

            return Ok((order_a_output, order_b_output));
        });

        // ? Get the result or return the error
        let (execution_output_a, execution_output_b) = swap_execution_handle
            .or_else(|e| {
                println!("error occured executing spot swap2 :  {:?}", e);

                unblock_order(
                    &blocked_order_ids_c,
                    self.order_a.order_id,
                    self.order_b.order_id,
                );

                Err(send_swap_error(
                    "Unknow Error Occured".to_string(),
                    None,
                    Some(format!("error occured executing spot swap:  {:?}", e)),
                ))
            })?
            .or_else(|err: Report<SwapThreadExecutionError>| {
                println!("error occured executing spot swap1:  {:?}", err);

                unblock_order(
                    &blocked_order_ids_c,
                    self.order_a.order_id,
                    self.order_b.order_id,
                );

                Err(err)
            })?;

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

        // * JSON Output ========================================================================================
        let swap_output = TransactionOutptut::new(&self);

        let mut spot_note_info_res_a = None;
        let mut spot_note_info_res_b = None;
        let mut updated_tab_hash_a = None;
        let mut updated_tab_hash_b = None;
        if execution_output_a.updated_order_tab.is_some() {
            let updated_tab = execution_output_a.updated_order_tab.as_ref().unwrap();
            updated_tab_hash_a = Some(updated_tab.hash.clone());
        } else {
            // ? non-tab order
            let note_info_output = execution_output_a.note_info_output.as_ref().unwrap();

            let mut new_pfr_idx_a: u64 = 0;
            if let Some(new_pfr_note) = note_info_output.new_partial_fill_info.as_ref() {
                new_pfr_idx_a = new_pfr_note.0.as_ref().unwrap().index;
            }

            spot_note_info_res_a = Some((
                note_info_output.prev_partial_fill_refund_note.clone(),
                note_info_output.swap_note.index,
                new_pfr_idx_a,
            ));
        }
        if execution_output_b.updated_order_tab.is_some() {
            let updated_tab = execution_output_b.updated_order_tab.as_ref().unwrap();
            updated_tab_hash_b = Some(updated_tab.hash.clone());
        } else {
            // ? non-tab order
            let note_info_output = execution_output_b.note_info_output.as_ref().unwrap();

            let mut new_pfr_idx_b: u64 = 0;
            if let Some(new_pfr_note) = note_info_output.new_partial_fill_info.as_ref() {
                new_pfr_idx_b = new_pfr_note.0.as_ref().unwrap().index;
            }

            spot_note_info_res_b = Some((
                note_info_output.prev_partial_fill_refund_note.clone(),
                note_info_output.swap_note.index,
                new_pfr_idx_b,
            ));
        }

        let json_output = swap_output.wrap_output(
            &spot_note_info_res_a,
            &spot_note_info_res_b,
            &prev_order_tab_a2,
            &prev_order_tab_b2,
            &updated_tab_hash_a,
            &updated_tab_hash_b,
        );

        let mut swap_output_json = swap_output_json_m.lock();
        swap_output_json.push(json_output);
        drop(swap_output_json);

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
