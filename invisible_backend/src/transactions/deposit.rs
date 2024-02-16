use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::transaction_batch::{LeafNodeType, CHAIN_IDS};
use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::errors::{
    send_deposit_error, DepositThreadExecutionError, TransactionExecutionError,
};

use crate::utils::crypto_utils::{hash_many, verify, Signature};
use crate::utils::storage::backup_storage::BackupStorage;
use crate::utils::storage::local_storage::MainStorage;
use num_bigint::BigUint;
use serde_json::Value;

use crossbeam::thread;
use error_stack::{Report, Result};

//
use crate::utils::notes::Note;

use super::swap::SwapResponse;
use super::transaction_helpers::db_updates::update_db_after_deposit;
use super::transaction_helpers::state_updates::update_state_after_deposit;
use super::Transaction;
//

#[derive(Debug, Clone)]
pub struct Deposit {
    pub transaction_type: String,
    pub deposit_id: u64,
    pub deposit_token: u32,
    pub deposit_amount: u64,
    pub stark_key: BigUint,
    pub notes: Vec<Note>,
    pub signature: Signature,
}

impl Deposit {
    pub fn execute_deposit(
        &mut self,
        tree_m: Arc<Mutex<SuperficialTree>>,
        updated_state_hashes_m: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
        swap_output_json_m: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        session: &Arc<Mutex<ServiceSession>>,
        main_storage: &Arc<Mutex<MainStorage>>,
        backup_storage: &Arc<Mutex<BackupStorage>>,
    ) -> Result<Vec<u64>, DepositThreadExecutionError> {
        //

        let deposit_id = self.deposit_id;
        let new_notes = self.notes.clone();

        let deposit_handle = thread::scope(move |_s| {
            let mut tree = tree_m.lock();

            let mut zero_idxs: Vec<u64> = Vec::new();
            for _ in 0..self.notes.len() {
                let idx = tree.first_zero_idx();
                zero_idxs.push(idx);
            }
            drop(tree);

            // ? Sum the notes and set the zero leaf indexes
            let mut amount_sum = 0u64;

            for i in 0..self.notes.len() {
                if self.notes[i].token != self.deposit_token {
                    return Err(send_deposit_error(
                        "deposit and note token missmatch".to_string(),
                        None,
                    ));
                }
                amount_sum += self.notes[i].amount;

                self.notes[i].index = zero_idxs[i];
            }

            if amount_sum != self.deposit_amount {
                return Err(send_deposit_error(
                    "deposit and note amount missmatch".to_string(),
                    None,
                ));
            }

            // ? verify Signature
            self.verify_deposit_signature()?;

            // ? Verify chain id
            let chain_id = deposit_id / 2u64.pow(32);
            if !CHAIN_IDS.contains(&(chain_id as u32)) {
                return Err(send_deposit_error(
                    "invalid chain id".to_string(),
                    Some(format!("invalid chain id: {}", chain_id)),
                ));
            }

            // // ? Verify the deposit has been registered
            // let data_commitment = self.get_action_commitment();
            // let main_storage_m = main_storage.lock();
            // if !main_storage_m.does_commitment_exists(
            //     OnchainActionType::Deposit,
            //     self.deposit_id % 2_u64.pow(32),
            //     &data_commitment,
            // ) {
            //     return Err(send_deposit_error(
            //         "deposit not registered".to_string(),
            //         Some(format!("deposit not registered: {}", self.deposit_id)),
            //     ));
            // }
            // main_storage_m.remove_onchain_action_commitment(self.deposit_id % 2_u64.pow(32));
            // drop(main_storage_m);

            // * After the deposit is verified to be valid update the state ================ //

            // ? Update the state
            let mut tree = tree_m.lock();
            update_state_after_deposit(&mut tree, &updated_state_hashes_m, &self.notes);
            drop(tree);

            let mut json_map = serde_json::map::Map::new();
            json_map.insert(
                String::from("transaction_type"),
                serde_json::to_value(&self.transaction_type).unwrap(),
            );
            json_map.insert(
                String::from("deposit"),
                serde_json::to_value(&self).unwrap(),
            );

            let mut swap_output_json = swap_output_json_m.lock();
            swap_output_json.push(json_map);
            drop(swap_output_json);

            return Ok(zero_idxs);
        });

        let zero_idxs = deposit_handle
            .or_else(|_| {
                // ? Some unknow error happened while executing the deposit
                Err(Report::new(DepositThreadExecutionError {
                    err_msg: format!("Unknown Deposit Error Occurred"),
                }))
            })?
            .or_else(|err: Report<DepositThreadExecutionError>| {
                // ? One of the known errors happened while executing the deposit
                Err(err)
            })?;

        // ? Update the datatbase
        update_db_after_deposit(&session, backup_storage, new_notes, &zero_idxs, deposit_id);

        return Ok(zero_idxs);
    }

    // * HELPER FUNCTIONS * //

    fn verify_deposit_signature(&self) -> Result<(), DepositThreadExecutionError> {
        let deposit_hash = self.hash_transaction();

        let valid = verify(&self.stark_key, &deposit_hash, &self.signature);

        if valid {
            return Ok(());
        } else {
            println!(
                "Invalid Signature: hash: {:?} pubKey: {:?} r:{:?} s:{:?}",
                deposit_hash, &self.stark_key, &self.signature.r, &self.signature.s
            );

            return Err(send_deposit_error(
                "Invalid Signature".to_string(),
                Some(format!(
                    "Invalid signature: r:{:?} s:{:?}",
                    &self.signature.r, &self.signature.s,
                )),
            ));
        }
    }

    fn get_action_commitment(&self) -> BigUint {
        // & h = H(depositId, starkKey, token, deposit_amount)

        let deposit_commitment = hash_many(&vec![
            &BigUint::from(self.deposit_id),
            &self.stark_key,
            &BigUint::from(self.deposit_token),
            &BigUint::from(self.deposit_amount),
        ]);

        return deposit_commitment;
    }

    fn hash_transaction(&self) -> BigUint {
        let mut note_hashes: Vec<&BigUint> = self.notes.iter().map(|note| &note.hash).collect();
        let deposit_id_bn =
            BigUint::from_str(self.deposit_id.to_string().as_str()).unwrap_or_default();
        note_hashes.push(&deposit_id_bn);

        return hash_many(&note_hashes);
    }
}

// * Trait Implementation * //
impl Transaction for Deposit {
    fn transaction_type(&self) -> &str {
        return self.transaction_type.as_str();
    }

    fn execute_transaction(
        &mut self,
        tree_m: Arc<Mutex<SuperficialTree>>,
        _partial_fill_tracker_m: Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
        updated_state_hashes_m: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
        swap_output_json_m: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        _blocked_order_ids_m: Arc<Mutex<HashMap<u64, bool>>>,
        session: &Arc<Mutex<ServiceSession>>,
        main_storage: &Arc<Mutex<MainStorage>>,
        backup_storage: &Arc<Mutex<BackupStorage>>,
    ) -> Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError> {
        let zero_idxs = self
            .execute_deposit(
                tree_m,
                updated_state_hashes_m,
                swap_output_json_m,
                session,
                main_storage,
                backup_storage,
            )
            .or_else(|err: Report<DepositThreadExecutionError>| {
                let error_context = err.current_context().clone();
                Err(
                    Report::new(TransactionExecutionError::Deposit(error_context.clone()))
                        .attach_printable(format!("Deposit transaction execution failed")),
                )
            })?;

        return Ok((None, Some(zero_idxs)));
    }
}

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for Deposit {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut note = serializer.serialize_struct("Deposit", 6)?;

        note.serialize_field("deposit_id", &self.deposit_id)?;
        note.serialize_field("deposit_token", &self.deposit_token)?;
        note.serialize_field("deposit_amount", &self.deposit_amount)?;
        note.serialize_field("stark_key", &self.stark_key.to_string())?;
        note.serialize_field("notes", &self.notes)?;
        note.serialize_field("signature", &self.signature)?;

        return note.end();
    }
}

// ================================================================================================= //
