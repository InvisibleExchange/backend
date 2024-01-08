use error_stack::Result;
use firestore_db_and_auth::ServiceSession;
use std::{collections::HashMap, sync::Arc};

use num_bigint::BigUint;
use parking_lot::Mutex;

use crate::{
    transaction_batch::{LeafNodeType, TxOutputJson},
    trees::superficial_tree::SuperficialTree,
    utils::{
        errors::TransactionExecutionError, notes::Note, storage::backup_storage::BackupStorage,
        storage::local_storage::MainStorage,
    },
};

use self::swap::SwapResponse;

pub mod deposit;
pub mod limit_order;
mod order_execution;
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
        transaction_output_json: Arc<Mutex<TxOutputJson>>,
        blocked_order_ids: Arc<Mutex<HashMap<u64, bool>>>,
        session: &Arc<Mutex<ServiceSession>>,
        main_storage: &Arc<Mutex<MainStorage>>,
        backup_storage: &Arc<Mutex<BackupStorage>>,
    ) -> Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError>;
}
