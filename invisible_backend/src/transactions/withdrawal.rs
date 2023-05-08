use std::collections::HashMap;

use firestore_db_and_auth::ServiceSession;
use parking_lot::Mutex;
use starknet::curve::AffinePoint;
use std::sync::Arc;
use std::thread::ThreadId;

use crate::trees::superficial_tree::SuperficialTree;
use crate::utils::crypto_utils::{pedersen_on_vec, verify, EcPoint, Signature};
use crate::utils::errors::{
    send_withdrawal_error, TransactionExecutionError, WithdrawalThreadExecutionError,
};

use crate::utils::storage::BackupStorage;
use crossbeam::thread;
use error_stack::{Report, Result};
use num_bigint::BigUint;
use num_traits::Zero;
use serde_json::Value;

use super::transaction_helpers::db_updates::update_db_after_withdrawal;
use super::transaction_helpers::rollbacks::RollbackInfo;
use super::transaction_helpers::state_updates::update_state_after_withdrawal;
//
use super::swap::{SwapResponse, Transaction};
use crate::utils::notes::Note;
//

pub struct Withdrawal {
    pub transaction_type: String,
    pub withdrawal_token: u64,
    pub withdrawal_amount: u64,
    pub stark_key: BigUint,
    pub notes_in: Vec<Note>,
    pub refund_note: Option<Note>,
    pub signature: Signature,
}

impl Withdrawal {
    pub fn execute_withdrawal(
        &self,
        tree_m: Arc<Mutex<SuperficialTree>>,
        updated_note_hashes_m: Arc<Mutex<HashMap<u64, BigUint>>>,
        swap_output_json_m: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
        session: Arc<Mutex<ServiceSession>>,
        backup_storage: Arc<Mutex<BackupStorage>>,
    ) -> Result<(), WithdrawalThreadExecutionError> {
        let withdrawal_handle = thread::scope(move |_s| {
            let mut valid: bool = true;
            let amount_sum = self.notes_in.iter().fold(0u64, |acc, note| {
                if note.token != self.withdrawal_token {
                    valid = false;
                }
                return acc + note.amount;
            });

            if !valid {
                return Err(send_withdrawal_error(
                    "Notes do not match withdrawal token".to_string(),
                    None,
                ));
            }

            let refund_amount = if self.refund_note.is_some() {
                self.refund_note.as_ref().unwrap().amount
            } else {
                0
            };
            if amount_sum != self.withdrawal_amount + refund_amount {
                return Err(send_withdrawal_error(
                    "Notes do not match withdrawal and refund amount".to_string(),
                    None,
                ));
            }

            // ? Verify signature
            self.verify_withdrawal_signatures()?;

            // ? Update state
            let mut tree = tree_m.lock();
            let mut updated_note_hashes = updated_note_hashes_m.lock();
            update_state_after_withdrawal(
                &mut tree,
                &mut updated_note_hashes,
                &rollback_safeguard,
                &self.notes_in,
                &self.refund_note,
            )?;
            drop(tree);
            drop(updated_note_hashes);

            let mut json_map = serde_json::map::Map::new();
            json_map.insert(
                String::from("transaction_type"),
                serde_json::to_value(&self.transaction_type).unwrap(),
            );
            json_map.insert(
                String::from("withdrawal"),
                serde_json::to_value(&self).unwrap(),
            );

            let mut swap_output_json = swap_output_json_m.lock();
            swap_output_json.push(json_map);
            drop(swap_output_json);

            Ok(())
        });

        withdrawal_handle
            .or_else(|_| {
                Err(send_withdrawal_error(
                    "Unknown error occured in withdrawal".to_string(),
                    None,
                ))
            })?
            .or_else(|err| Err(err))?;

        // ? Update the database
        update_db_after_withdrawal(
            &session,
            backup_storage,
            &self.notes_in,
            self.refund_note.clone(),
        );

        Ok(())
    }

    // * UPDATE STATE * //

    // * HELPER FUNCTIONS * //

    fn verify_withdrawal_signatures(&self) -> Result<(), WithdrawalThreadExecutionError> {
        let withdrawal_hash = self.hash_transaction();

        let mut pub_key_sum: AffinePoint = AffinePoint::identity();

        for i in 0..self.notes_in.len() {
            let ec_point = AffinePoint::from(&self.notes_in[i].address);
            pub_key_sum = &pub_key_sum + &ec_point;
        }

        let pub_key: EcPoint = EcPoint::from(&pub_key_sum);

        let valid = verify(
            &pub_key.x.to_biguint().unwrap(),
            &withdrawal_hash,
            &self.signature,
        );

        if valid {
            return Ok(());
        } else {
            return Err(send_withdrawal_error(
                "Invalid Signature".to_string(),
                Some(format!(
                    "Invalid signature: r:{:?} s:{:?} hash:{:?} pub_key:{:?}",
                    &self.signature.r, &self.signature.s, withdrawal_hash, pub_key
                )),
            ));
        }
    }

    fn hash_transaction(&self) -> BigUint {
        let z = BigUint::zero();
        let mut note_hashes: Vec<&BigUint> = self.notes_in.iter().map(|note| &note.hash).collect();
        let refund_note_hash = if self.refund_note.is_some() {
            &self.refund_note.as_ref().unwrap().hash
        } else {
            &z
        };

        note_hashes.insert(0, &refund_note_hash);
        note_hashes.insert(0, &self.stark_key);

        let withdrawal_hash = pedersen_on_vec(&note_hashes);

        return withdrawal_hash;
    }
}

// * Transaction Trait * //
impl Transaction for Withdrawal {
    fn transaction_type(&self) -> &str {
        return self.transaction_type.as_str();
    }

    fn execute_transaction(
        &mut self,
        tree: Arc<Mutex<SuperficialTree>>,
        _: Arc<Mutex<HashMap<u64, (Note, u64)>>>,
        updated_note_hashes: Arc<Mutex<HashMap<u64, BigUint>>>,
        swap_output_json: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        _: Arc<Mutex<HashMap<u64, bool>>>,
        rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
        session: Arc<Mutex<ServiceSession>>,
        backup_storage: Arc<Mutex<BackupStorage>>,
    ) -> Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError> {
        self.execute_withdrawal(
            tree,
            updated_note_hashes,
            swap_output_json,
            rollback_safeguard,
            session,
            backup_storage,
        )
        .or_else(|err: Report<WithdrawalThreadExecutionError>| {
            let error_context = err.current_context().clone();
            Err(
                Report::new(TransactionExecutionError::Withdrawal(error_context.clone()))
                    .attach_printable(format!(
                        "Withdrawal transaction execution failed with error {:?}",
                        error_context
                    )),
            )
        })?;

        return Ok((None, None));
    }
}

use serde::ser::{Serialize, SerializeStruct, Serializer};

impl Serialize for Withdrawal {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut withdrawal = serializer.serialize_struct("Withdrawal", 9)?;

        withdrawal.serialize_field("transaction_type", &self.transaction_type)?;
        withdrawal.serialize_field("withdrawal_token", &self.withdrawal_token)?;
        withdrawal.serialize_field("withdrawal_amount", &self.withdrawal_amount)?;
        withdrawal.serialize_field("stark_key", &self.stark_key.to_string())?;
        withdrawal.serialize_field("notes_in", &self.notes_in)?;
        withdrawal.serialize_field("refund_note", &self.refund_note)?;
        withdrawal.serialize_field("signature", &self.signature)?;

        return withdrawal.end();
    }
}
