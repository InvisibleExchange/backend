use error_stack::Result;
use firestore_db_and_auth::ServiceSession;
use std::{collections::HashMap, sync::Arc, thread::ThreadId};

use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::Value;

use crate::{
    transaction_batch::LeafNodeType,
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::TransactionExecutionError, notes::Note, storage::local_storage::BackupStorage,
    },
};

use self::{swap::SwapResponse, transaction_helpers::rollbacks::RollbackInfo};

pub mod deposit;
pub mod limit_order;
pub mod swap;
mod swap_execution;
pub mod transaction_helpers;
pub mod withdrawal;

pub trait Transaction {
    fn transaction_type(&self) -> &str;

    fn execute_transaction(
        &mut self,
        state_tree: Arc<Mutex<SuperficialTree>>,
        partial_fill_tracker: Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>,
        updated_state_hashes: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>,
        swap_output_json: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>,
        blocked_order_ids: Arc<Mutex<HashMap<u64, bool>>>,
        rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
        session: &Arc<Mutex<ServiceSession>>,
        backup_storage: &Arc<Mutex<BackupStorage>>,
    ) -> Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError>;
}
