use firestore_db_and_auth::ServiceSession;
use num_bigint::BigUint;
use parking_lot::Mutex;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    fs,
    sync::Arc,
    thread::{self, JoinHandle, ThreadId},
};

use error_stack::Result;

use crate::{
    perpetual::{
        liquidations::{
            liquidation_engine::LiquidationSwap, liquidation_output::LiquidationResponse,
        },
        perp_helpers::{perp_rollback::PerpRollbackInfo, perp_swap_outptut::PerpSwapResponse},
        perp_position::PerpPosition,
        perp_swap::PerpSwap,
    },
    server::grpc::{OrderTabActionMessage, OrderTabActionResponse},
    transactions::Transaction,
};
use crate::{server::grpc::RollbackMessage, utils::storage::MainStorage};
use crate::{trees::superficial_tree::SuperficialTree, utils::storage::BackupStorage};

use crate::utils::{
    errors::{
        BatchFinalizationError, OracleUpdateError, PerpSwapExecutionError,
        TransactionExecutionError,
    },
    firestore::create_session,
    notes::Note,
};

use crate::transactions::{swap::SwapResponse, transaction_helpers::rollbacks::RollbackInfo};

use crate::server::grpc::{ChangeMarginMessage, FundingUpdateMessage};

use crate::transaction_batch::{
    tx_batch_helpers::_init_empty_tokens_map,
    tx_batch_structs::{OracleUpdate, SwapFundingInfo},
};

use self::{
    batch_functions::{
        admin_functions::{_init_inner, _per_minute_funding_updates, _update_index_prices_inner},
        batch_transition::{_finalize_batch_inner, _transition_state},
        state_modifications::{
            _change_position_margin_inner, _execute_order_tab_modification_inner,
            _split_notes_inner,
        },
    },
    restore_state_helpers::_restore_state_inner,
};

// TODO: This could be weighted sum of different transactions (e.g. 5 for swaps, 1 for deposits, 1 for withdrawals)
// const TRANSACTIONS_PER_BATCH: u16 = 10; // Number of transaction per batch (until batch finalization)

// TODO: Make fields in all classes private where they should be

// TODO: If you get a note doesn't exist error, there should  be a function where you can check the existence of all your notes

pub mod batch_functions;
pub mod restore_state_helpers;
pub mod tx_batch_helpers;
pub mod tx_batch_structs;

// { ETH Mainnet: 9090909, Starknet: 7878787, ZkSync: 5656565 }
pub const CHAIN_IDS: [u32; 3] = [9090909, 7878787, 5656565];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LeafNodeType {
    Note,
    Position,
    OrderTab,
    MMSpotRegistration,
    MMPerpRegistration,
}
pub struct TransactionBatch {
    pub state_tree: Arc<Mutex<SuperficialTree>>, // current state tree (superficial tree only stores the leaves)
    pub partial_fill_tracker: Arc<Mutex<HashMap<u64, (Option<Note>, u64)>>>, // maps orderIds to partial fill refund notes and filled mounts
    pub updated_state_hashes: Arc<Mutex<HashMap<u64, (LeafNodeType, BigUint)>>>, // info to get merkle proofs at the end of the batch
    pub swap_output_json: Arc<Mutex<Vec<serde_json::Map<String, Value>>>>, // json output map for cairo input
    pub blocked_order_ids: Arc<Mutex<HashMap<u64, bool>>>, // maps orderIds to whether they are blocked while another thread is processing the same order (in case of partial fills)
    //
    // pub perpetual_state_tree: Arc<Mutex<SuperficialTree>>, // current perpetual state tree (superficial tree only stores the leaves)
    pub perpetual_partial_fill_tracker: Arc<Mutex<HashMap<u64, (Option<Note>, u64, u64)>>>, // (pfr_note, amount_filled, spent_margin)
    pub partialy_opened_positions: Arc<Mutex<HashMap<String, (PerpPosition, u64)>>>, // positions that were partially opened in an order that was partially filled
    pub blocked_perp_order_ids: Arc<Mutex<HashMap<u64, bool>>>, // maps orderIds to whether they are blocked while another thread is processing the same order (in case of partial fills)
    pub insurance_fund: Arc<Mutex<i64>>, // insurance fund used to pay for liquidations
    //
    pub latest_index_price: HashMap<u32, u64>,
    pub min_index_price_data: HashMap<u32, (u64, OracleUpdate)>, // maps asset id to the min price, OracleUpdate info of this batch
    pub max_index_price_data: HashMap<u32, (u64, OracleUpdate)>, // maps asset id to the max price, OracleUpdate info of this batch
    //
    pub running_funding_tick_sums: HashMap<u32, i64>, // maps asset id to the sum of all funding ticks in this batch (used for TWAP)
    pub current_funding_count: u16, // maps asset id to the number of funding ticks applied already (used for TWAP, goes up to 480)

    pub funding_rates: HashMap<u32, Vec<i64>>, // maps asset id to an array of funding rates (not reset at new batch)
    pub funding_prices: HashMap<u32, Vec<u64>>, // maps asset id to an array of funding prices (corresponding to the funding rates) (not reset at new batch)
    pub current_funding_idx: u32, // the current index of the funding rates and prices arrays
    pub funding_idx_shift: HashMap<u32, u32>, // maps asset id to an funding idx shift
    pub min_funding_idxs: Arc<Mutex<HashMap<u32, u32>>>, // the min funding index of a position being updated in this batch for each asset
    //
    pub rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>, // used to rollback the state in case of errors
    pub perp_rollback_safeguard: Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>>, // used to rollback the perp_state in case of errors
    //
    pub firebase_session: Arc<Mutex<ServiceSession>>, // Firebase session for updating the database in the cloud
    pub main_storage: Arc<Mutex<MainStorage>>,        // Storage Connection to store data on disk
    pub backup_storage: Arc<Mutex<BackupStorage>>,    // Storage for failed database updates
    //
    pub running_index_price_count: u16, // number of index price updates in the current micro batch
}

impl TransactionBatch {
    pub fn new(
        tree_depth: u32,
        rollback_safeguard: Arc<Mutex<HashMap<ThreadId, RollbackInfo>>>,
        perp_rollback_safeguard: Arc<Mutex<HashMap<ThreadId, PerpRollbackInfo>>>,
    ) -> TransactionBatch {
        let state_tree = SuperficialTree::new(tree_depth);
        let partial_fill_tracker: HashMap<u64, (Option<Note>, u64)> = HashMap::new();
        let updated_state_hashes: HashMap<u64, (LeafNodeType, BigUint)> = HashMap::new();
        let swap_output_json: Vec<serde_json::Map<String, Value>> = Vec::new();
        let blocked_order_ids: HashMap<u64, bool> = HashMap::new();

        // let perpetual_state_tree = SuperficialTree::new(perp_tree_depth);
        let perpetual_partial_fill_tracker: HashMap<u64, (Option<Note>, u64, u64)> = HashMap::new();
        let partialy_opened_positions: HashMap<String, (PerpPosition, u64)> = HashMap::new();
        let blocked_perp_order_ids: HashMap<u64, bool> = HashMap::new();

        // let order_tabs_state_tree = SuperficialTree::new(16);

        let mut latest_index_price: HashMap<u32, u64> = HashMap::new();
        let mut min_index_price_data: HashMap<u32, (u64, OracleUpdate)> = HashMap::new();
        let mut max_index_price_data: HashMap<u32, (u64, OracleUpdate)> = HashMap::new();

        let mut running_funding_tick_sums: HashMap<u32, i64> = HashMap::new();
        let mut funding_rates: HashMap<u32, Vec<i64>> = HashMap::new();
        let mut funding_prices: HashMap<u32, Vec<u64>> = HashMap::new();
        let mut min_funding_idxs: HashMap<u32, u32> = HashMap::new();
        let mut funding_idx_shift: HashMap<u32, u32> = HashMap::new();

        let session = create_session();
        let session = Arc::new(Mutex::new(session));

        // Init empty maps
        _init_empty_tokens_map::<u64>(&mut latest_index_price);
        _init_empty_tokens_map::<(u64, OracleUpdate)>(&mut min_index_price_data);
        _init_empty_tokens_map::<(u64, OracleUpdate)>(&mut max_index_price_data);
        _init_empty_tokens_map::<i64>(&mut running_funding_tick_sums);
        _init_empty_tokens_map::<Vec<i64>>(&mut funding_rates);
        _init_empty_tokens_map::<Vec<u64>>(&mut funding_prices);
        _init_empty_tokens_map::<u32>(&mut funding_idx_shift);
        _init_empty_tokens_map::<u32>(&mut min_funding_idxs);

        // TODO: For testing only =================================================
        latest_index_price.insert(54321, 2000 * 10u64.pow(6));
        latest_index_price.insert(12345, 30000 * 10u64.pow(6));
        latest_index_price.insert(66666, 10800);
        // TODO: For testing only =================================================

        let tx_batch = TransactionBatch {
            state_tree: Arc::new(Mutex::new(state_tree)),
            partial_fill_tracker: Arc::new(Mutex::new(partial_fill_tracker)),
            updated_state_hashes: Arc::new(Mutex::new(updated_state_hashes)),
            swap_output_json: Arc::new(Mutex::new(swap_output_json)),
            blocked_order_ids: Arc::new(Mutex::new(blocked_order_ids)),
            //
            perpetual_partial_fill_tracker: Arc::new(Mutex::new(perpetual_partial_fill_tracker)),
            partialy_opened_positions: Arc::new(Mutex::new(partialy_opened_positions)),
            blocked_perp_order_ids: Arc::new(Mutex::new(blocked_perp_order_ids)),
            insurance_fund: Arc::new(Mutex::new(0)),
            //
            latest_index_price,
            min_index_price_data,
            max_index_price_data,
            //
            running_funding_tick_sums,
            current_funding_count: 0,
            funding_rates,
            funding_prices,
            current_funding_idx: 0,
            funding_idx_shift,
            min_funding_idxs: Arc::new(Mutex::new(min_funding_idxs)),
            //
            rollback_safeguard,
            perp_rollback_safeguard,
            //
            firebase_session: session,
            main_storage: Arc::new(Mutex::new(MainStorage::new())),
            backup_storage: Arc::new(Mutex::new(BackupStorage::new())),
            //
            running_index_price_count: 0,
        };

        return tx_batch;
    }

    /// This initializes the transaction batch from a previous state
    pub fn init(&mut self) {
        _init_inner(
            &mut self.main_storage,
            &mut self.funding_rates,
            &mut self.funding_prices,
            &mut self.current_funding_idx,
            &mut self.funding_idx_shift,
            &mut self.min_funding_idxs,
            &mut self.latest_index_price,
            &mut self.min_index_price_data,
            &mut self.max_index_price_data,
            &mut self.state_tree,
        );

        let storage = self.main_storage.lock();
        if !storage.tx_db.is_empty() {
            let swap_output_json = storage.read_storage(0);
            drop(storage);
            self.restore_state(swap_output_json);
        }
    }

    pub fn revert_current_tx_batch(&mut self) {
        // TODO: Copy the state_tree_backup file to the current state_tree file

        // ? Attempt to delete the file
        let latest_batch_index = self.main_storage.lock().latest_batch;
        match fs::remove_file(
            "./storage/transaction_data/".to_string() + latest_batch_index.to_string().as_str(),
        ) {
            Ok(()) => println!("File deleted successfully"),
            Err(err) => eprintln!("Error deleting file: {}", err),
        }
    }

    pub fn execute_transaction<T: Transaction + std::marker::Send + 'static>(
        &mut self,
        mut transaction: T,
    ) -> JoinHandle<Result<(Option<SwapResponse>, Option<Vec<u64>>), TransactionExecutionError>>
    {
        //

        let state_tree = self.state_tree.clone();
        let partial_fill_tracker = self.partial_fill_tracker.clone();
        let updated_state_hashes = self.updated_state_hashes.clone();
        let swap_output_json = self.swap_output_json.clone();
        let blocked_order_ids = self.blocked_order_ids.clone();
        let rollback_safeguard = self.rollback_safeguard.clone();
        let session = self.firebase_session.clone();
        let backup_storage = self.backup_storage.clone();

        let handle = thread::spawn(move || {
            let res = transaction.execute_transaction(
                state_tree,
                partial_fill_tracker,
                updated_state_hashes,
                swap_output_json,
                blocked_order_ids,
                rollback_safeguard,
                &session,
                &backup_storage,
            );
            return res;
        });

        return handle;
    }

    pub fn execute_perpetual_transaction(
        &mut self,
        transaction: PerpSwap,
    ) -> JoinHandle<Result<PerpSwapResponse, PerpSwapExecutionError>> {
        let state_tree = self.state_tree.clone();
        let updated_state_hashes = self.updated_state_hashes.clone();
        let swap_output_json = self.swap_output_json.clone();

        let perpetual_partial_fill_tracker = self.perpetual_partial_fill_tracker.clone();
        let partialy_opened_positions = self.partialy_opened_positions.clone();
        let blocked_perp_order_ids = self.blocked_perp_order_ids.clone();

        let session = self.firebase_session.clone();
        let backup_storage = self.backup_storage.clone();

        let current_index_price = *self
            .latest_index_price
            .get(&transaction.order_a.synthetic_token)
            .unwrap();
        let min_funding_idxs = self.min_funding_idxs.clone();

        let perp_rollback_safeguard = self.perp_rollback_safeguard.clone();

        let swap_funding_info = SwapFundingInfo::new(
            &self.funding_rates,
            &self.funding_prices,
            self.current_funding_idx,
            &self.funding_idx_shift,
            transaction.order_a.synthetic_token,
            &transaction.order_a.position,
            &transaction.order_b.position,
        );

        let handle = thread::spawn(move || {
            return transaction.execute(
                state_tree,
                updated_state_hashes,
                swap_output_json,
                blocked_perp_order_ids,
                perpetual_partial_fill_tracker,
                partialy_opened_positions,
                current_index_price,
                min_funding_idxs,
                swap_funding_info,
                perp_rollback_safeguard,
                session,
                backup_storage,
            );
        });

        return handle;
    }

    pub fn execute_liquidation_transaction(
        &mut self,
        liquidation_transaction: LiquidationSwap,
    ) -> JoinHandle<Result<LiquidationResponse, PerpSwapExecutionError>> {
        let state_tree = self.state_tree.clone();
        let updated_state_hashes = self.updated_state_hashes.clone();
        let swap_output_json = self.swap_output_json.clone();

        let session = self.firebase_session.clone();
        let backup_storage = self.backup_storage.clone();

        let insurance_fund = self.insurance_fund.clone();

        let current_index_price = *self
            .latest_index_price
            .get(&liquidation_transaction.liquidation_order.synthetic_token)
            .unwrap();
        let min_funding_idxs = self.min_funding_idxs.clone();

        let swap_funding_info = SwapFundingInfo::new(
            &self.funding_rates,
            &self.funding_prices,
            self.current_funding_idx,
            &self.funding_idx_shift,
            liquidation_transaction.liquidation_order.synthetic_token,
            &Some(liquidation_transaction.liquidation_order.position.clone()),
            &None,
        );

        let handle = thread::spawn(move || {
            return liquidation_transaction.execute(
                state_tree,
                updated_state_hashes,
                swap_output_json,
                insurance_fund,
                current_index_price,
                min_funding_idxs,
                swap_funding_info,
                session,
                backup_storage,
            );
        });

        return handle;
    }

    // * Rollback the transaction execution state updates
    pub fn rollback_transaction(&mut self, _rollback_info_message: (ThreadId, RollbackMessage)) {
        // let thread_id = rollback_info_message.0;
        // let rollback_message = rollback_info_message.1;

        println!(
            "Rolling back transaction: {:?}",
            _rollback_info_message.1.tx_type
        );

        // if rollback_message.tx_type == "deposit" {
        //     // ? rollback the deposit execution state updates

        //     let rollback_info = self.rollback_safeguard.lock().remove(&thread_id).unwrap();

        //     rollback_deposit_updates(&self.state_tree, &self.updated_state_hashes, rollback_info);
        // } else if rollback_message.tx_type == "swap" {
        //     // ? rollback the swap execution state updates

        //     let rollback_info = self.rollback_safeguard.lock().remove(&thread_id).unwrap();

        //     rollback_swap_updates(
        //         &self.state_tree,
        //         &self.updated_state_hashes,
        //         rollback_message,
        //         rollback_info,
        //     );
        // } else if rollback_message.tx_type == "withdrawal" {
        //     // ? rollback the withdrawal execution state updates

        //     rollback_withdrawal_updates(
        //         &self.state_tree,
        //         &self.updated_state_hashes,
        //         rollback_message,
        //     );
        // } else if rollback_message.tx_type == "perp_swap" {
        //     // ? rollback the perp swap execution state updates

        //     let rollback_info = self
        //         .perp_rollback_safeguard
        //         .lock()
        //         .remove(&thread_id)
        //         .unwrap();

        //     rollback_perp_swap(
        //         &self.state_tree,
        //         &self.updated_state_hashes,
        //         &self.perpetual_state_tree,
        //         &self.perpetual_updated_position_hashes,
        //         rollback_message,
        //         rollback_info,
        //     );
        // }
    }

    // * =================================================================
    // TODO: These two functions should take a constant fee to ensure not being DOSed
    pub fn split_notes(
        &mut self,
        notes_in: Vec<Note>,
        new_note: Note,
        refund_note: Option<Note>,
    ) -> std::result::Result<Vec<u64>, String> {
        return _split_notes_inner(
            &self.state_tree,
            &self.updated_state_hashes,
            &self.firebase_session,
            &self.backup_storage,
            &self.swap_output_json,
            notes_in,
            new_note,
            refund_note,
        );
    }

    pub fn change_position_margin(
        &self,
        margin_change: ChangeMarginMessage,
    ) -> std::result::Result<(u64, PerpPosition), String> {
        return _change_position_margin_inner(
            &self.state_tree,
            &self.updated_state_hashes,
            &self.firebase_session,
            &self.backup_storage,
            &self.swap_output_json,
            &self.latest_index_price,
            margin_change,
        );
    }

    pub fn execute_order_tab_modification(
        &mut self,
        tab_action_message: OrderTabActionMessage,
    ) -> JoinHandle<OrderTabActionResponse> {
        return _execute_order_tab_modification_inner(
            &self.state_tree,
            &self.updated_state_hashes,
            &self.firebase_session,
            &self.backup_storage,
            &self.swap_output_json,
            &self.latest_index_price,
            tab_action_message,
        );
    }

    // * =================================================================
    // * FINALIZE BATCH

    pub fn finalize_batch(&mut self) -> Result<(), BatchFinalizationError> {
        // TODO: This can only be executed if the previous batch was already executed

        let batch_transition_info = _finalize_batch_inner(
            &self.state_tree,
            &self.updated_state_hashes,
            &self.swap_output_json,
            &self.main_storage,
            &mut self.funding_rates,
            &mut self.funding_prices,
            &mut self.min_funding_idxs,
            &mut self.min_index_price_data,
            &mut self.max_index_price_data,
        );

        // * =================================================================

        // TODO: This requires spinning up a spot instances on aws to handle the load
        _transition_state(&self.main_storage, batch_transition_info)?;

        Ok(())
    }

    // * =================================================================
    // * RESTORE STATE

    pub fn restore_state(&mut self, transactions: Vec<Map<String, Value>>) {
        _restore_state_inner(
            &self.state_tree,
            &self.updated_state_hashes,
            &self.perpetual_partial_fill_tracker,
            &self.main_storage,
            transactions,
        )
    }

    // * FUNDING CALCULATIONS * //

    pub fn per_minute_funding_updates(&mut self, funding_update: FundingUpdateMessage) {
        _per_minute_funding_updates(
            &mut self.running_funding_tick_sums,
            &mut self.latest_index_price,
            &mut self.current_funding_count,
            &mut self.funding_rates,
            &mut self.funding_prices,
            &mut self.current_funding_idx,
            &self.min_funding_idxs,
            &self.main_storage,
            funding_update,
        )
    }

    // * PRICE FUNCTIONS * //

    pub fn update_index_prices(
        &mut self,
        oracle_updates: Vec<OracleUpdate>,
    ) -> Result<(), OracleUpdateError> {
        return _update_index_prices_inner(
            &mut self.latest_index_price,
            &mut self.min_index_price_data,
            &mut self.max_index_price_data,
            &mut self.running_index_price_count,
            &self.main_storage,
            oracle_updates,
        );
    }
}

//

//

//

//
