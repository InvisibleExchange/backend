use std::{collections::HashMap, fs, time::SystemTime};

use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use sled::Config;

use crate::transaction_batch::{
    batch_functions::batch_transition::BatchTransitionInfo, tx_batch_structs::OracleUpdate,
};

use super::firestore::upload_file_to_storage;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum OnchainActionType {
    Deposit,
    MMRegistration,
    MMAddLiquidity,
    MMRemoveLiquidity,
    MMClosePosition,
    NoteEscape,
    TabEscape,
    PositionEscape,
}

type StorageResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// The main storage struct that stores all the data on disk.
pub struct MainStorage {
    pub tx_db: sled::Db, // Stores the json ouput of the transactions executed this batch
    pub price_db: sled::Db, // Stores the price data of the current batch (min/max price with signatures)
    pub funding_db: sled::Db, // Stores the funding data since the begining(funding rates/prices)
    pub db_pending_updates: sled::Db, // small batches of txs that get pushed to the db periodically
    pub registerd_onchain_actions_db: sled::Db, // Onchain actions that were registered by the server
    pub latest_batch: u32,                      // every transaction batch stores data separately
    pub batch_transition_info_db: sled::Db, // stores the batch transition info after every batch
}

impl MainStorage {
    pub fn new() -> Self {
        let dir = fs::read_dir("storage/transaction_data");

        let batch_index = match dir {
            Ok(dir) => dir
                .filter(|entry| entry.as_ref().map(|e| e.path().is_dir()).unwrap_or(false))
                .count(),
            Err(_) => 1,
        };

        let config = Config::new()
            .path("./storage/transaction_data/".to_string() + &batch_index.to_string());
        let tx_db = config.open().unwrap();

        let config = Config::new().path("./storage/funding_info".to_string());
        let funding_db = config.open().unwrap();

        let config = Config::new().path("./storage/registered_actions".to_string());
        let registerd_onchain_actions_db = config.open().unwrap();

        let config = Config::new().path("./storage/batch_transition_info/".to_string());
        let batch_transition_info_db = config.open().unwrap();

        let config =
            Config::new().path("./storage/price_data/".to_string() + &batch_index.to_string());
        let price_db = config.open().unwrap();

        let config = Config::new().path("./storage/db_pending_updates".to_string());
        let db_pending_updates = config.open().unwrap();

        MainStorage {
            tx_db,
            funding_db,
            registerd_onchain_actions_db,
            latest_batch: batch_index as u32,
            price_db,
            db_pending_updates,
            batch_transition_info_db,
        }
    }

    pub fn revert_current_tx_batch(&self) {
        let dir = fs::read_dir("storage/transaction_data");

        let batch_index = match dir {
            Ok(dir) => {
                dir.filter(|entry| entry.as_ref().map(|e| e.path().is_dir()).unwrap_or(false))
                    .count()
                    - 1
            }
            Err(_) => 0,
        };

        // ? delete the current batch
        fs::remove_dir_all("storage/transaction_data/".to_string() + &batch_index.to_string())
            .unwrap();
    }

    /// Gets a batch of the latest K transactions that were executed
    /// and stores them on disk.
    ///
    /// # Arguments
    /// * swap_output_json - a vector of the latest 15-20 transactions as json maps
    ///
    pub fn store_micro_batch(&mut self, swap_output_json: &Vec<serde_json::Map<String, Value>>) {
        let index = self.tx_db.get("count").unwrap();
        let index = match index {
            Some(index) => {
                let index: u64 = serde_json::from_slice(&index.to_vec()).unwrap();
                index
            }
            None => 0,
        };

        let res = serde_json::to_vec(swap_output_json).unwrap();

        self.tx_db.insert(&index.to_string(), res).unwrap();
        self.tx_db
            .insert(
                "count".to_string(),
                serde_json::to_vec(&(index + 1)).unwrap(),
            )
            .unwrap();

        self.store_pending_batch_updates(swap_output_json);
    }

    /// Reads all the micro-batches from disk and returns them as a vector of json maps.
    ///
    /// # Arguments
    /// * shift_back - the number of micro-batches to shift back from the latest batch
    ///
    pub fn read_storage(&self, shift_back: u32) -> Vec<serde_json::Map<String, Value>> {
        let mut json_result = Vec::new();

        let tx_db;
        let db = if shift_back == 0 {
            &self.tx_db
        } else {
            let batch_index = self.latest_batch - shift_back;
            let config = Config::new()
                .path("./storage/transaction_data/".to_string() + &batch_index.to_string());
            tx_db = config.open().unwrap();
            &tx_db
        };

        let index = db.get("count").unwrap();
        let index = match index {
            Some(index) => {
                let index: u64 = serde_json::from_slice(&index.to_vec()).unwrap();
                index
            }
            None => 0,
        };

        for i in 0..index {
            let value = db.get(&i.to_string()).unwrap();
            let json_string = value.unwrap().to_vec();
            let res_vec: Vec<serde_json::Map<String, Value>> =
                serde_json::from_slice(&json_string).unwrap();

            json_result.extend(res_vec);
        }

        json_result
    }

    // * FUNDING INFO ————————————————————————————————————————————————————————————————————- //

    // pub funding_rates: HashMap<u64, Vec<i64>>, // maps asset id to an array of funding rates (not reset at new batch)
    // pub funding_prices: HashMap<u64, Vec<u64>>, // maps asset id to an array of funding prices (corresponding to the funding rates) (not reset at new batch)
    // pub min_funding_idxs: Arc<Mutex<HashMap<u64, u32>>>,
    pub fn store_funding_info(
        &self,
        funding_rates: &HashMap<u32, Vec<i64>>,
        funding_prices: &HashMap<u32, Vec<u64>>,
        min_funding_idx: &HashMap<u32, u32>,
    ) {
        self.funding_db
            .insert("funding_rates", serde_json::to_vec(&funding_rates).unwrap())
            .unwrap();
        self.funding_db
            .insert(
                "funding_prices",
                serde_json::to_vec(&funding_prices).unwrap(),
            )
            .unwrap();
        self.funding_db
            .insert(
                "min_funding_idx",
                serde_json::to_vec(&min_funding_idx).unwrap(),
            )
            .unwrap();
    }

    pub fn read_funding_info(
        &self,
    ) -> std::result::Result<
        (
            HashMap<u32, Vec<i64>>,
            HashMap<u32, Vec<u64>>,
            HashMap<u32, u32>,
        ),
        String,
    > {
        let funding_rates = self
            .funding_db
            .get("funding_rates")
            .unwrap()
            .ok_or("funding rates not found in storage")?;
        let funding_prices = self
            .funding_db
            .get("funding_prices")
            .unwrap()
            .ok_or("funding prices  not found in storage")?;
        let min_funding_idx = self
            .funding_db
            .get("min_funding_idx")
            .unwrap()
            .ok_or("min_funding_idx not found in storage")?;

        let funding_rates: HashMap<u32, Vec<i64>> =
            serde_json::from_slice(&funding_rates.to_vec()).unwrap();
        let funding_prices: HashMap<u32, Vec<u64>> =
            serde_json::from_slice(&funding_prices.to_vec()).unwrap();
        let min_funding_idx: HashMap<u32, u32> =
            serde_json::from_slice(&min_funding_idx.to_vec()).unwrap();

        Ok((funding_rates, funding_prices, min_funding_idx))
    }

    // * PRICE DATA ————————————————————————————————————————————————————————————————————- //

    pub fn store_price_data(
        &self,
        latest_index_price: &HashMap<u32, u64>,
        min_index_price_data: &HashMap<u32, (u64, OracleUpdate)>,
        max_index_price_data: &HashMap<u32, (u64, OracleUpdate)>,
    ) {
        self.price_db
            .insert(
                "latest_index_price",
                serde_json::to_vec(&latest_index_price).unwrap(),
            )
            .unwrap();
        self.price_db
            .insert(
                "min_index_price_data",
                serde_json::to_vec(&min_index_price_data).unwrap(),
            )
            .unwrap();
        self.price_db
            .insert(
                "max_index_price_data",
                serde_json::to_vec(&max_index_price_data).unwrap(),
            )
            .unwrap();
    }

    pub fn read_price_data(
        &self,
    ) -> Option<(
        HashMap<u32, u64>,
        HashMap<u32, (u64, OracleUpdate)>,
        HashMap<u32, (u64, OracleUpdate)>,
    )> {
        let latest_index_price = self.price_db.get("latest_index_price").unwrap();
        if let None = latest_index_price {
            return None;
        }

        let min_index_price_data = self.price_db.get("min_index_price_data").unwrap().unwrap();
        let max_index_price_data = self.price_db.get("max_index_price_data").unwrap().unwrap();

        let latest_index_price: HashMap<u32, u64> =
            serde_json::from_slice(&latest_index_price.unwrap().to_vec()).unwrap();
        let min_index_price_data: HashMap<u32, (u64, OracleUpdate)> =
            serde_json::from_slice(&min_index_price_data.to_vec()).unwrap();
        let max_index_price_data: HashMap<u32, (u64, OracleUpdate)> =
            serde_json::from_slice(&max_index_price_data.to_vec()).unwrap();

        Some((
            latest_index_price,
            min_index_price_data,
            max_index_price_data,
        ))
    }

    // * PENDING UPDATE DATA ———————————————————————————————————————————————————————————- //

    /// This stores the latest N Transactions that have not been pushed to the db yet.
    /// Every few minutes we push these transactions to the db.
    ///
    pub fn store_pending_batch_updates(
        &mut self,
        swap_output_json: &Vec<serde_json::Map<String, Value>>,
    ) {
        let index = self.db_pending_updates.get("count").unwrap();
        let index = match index {
            Some(index) => {
                let index: u64 = serde_json::from_slice(&index.to_vec()).unwrap();
                index
            }
            None => 0,
        };

        let res = serde_json::to_vec(swap_output_json).unwrap();

        self.db_pending_updates
            .insert(&index.to_string(), res)
            .unwrap();
        self.db_pending_updates
            .insert(
                "count".to_string(),
                serde_json::to_vec(&(index + 1)).unwrap(),
            )
            .unwrap();
    }

    pub fn process_pending_batch_updates(
        &mut self,
        finalizing_batch: bool,
    ) -> Option<impl std::future::Future<Output = StorageResult>>
//-> impl std::future::Future<Output = std::result::Result<(), Box<dyn std::error::Error>>>
    {
        let mut json_result = Vec::new();

        let count = self.db_pending_updates.get("count").unwrap();
        let count = match count {
            Some(count) => {
                let count: u64 = serde_json::from_slice(&count.to_vec()).unwrap();
                count
            }
            None => 0,
        };

        if count == 0 {
            return None;
        }

        let db_index = self.db_pending_updates.get("db_index").unwrap();
        let db_index = match db_index {
            Some(db_index) => {
                let db_index: u64 = serde_json::from_slice(&db_index.to_vec()).unwrap();
                db_index
            }
            None => 0,
        };

        let ts = SystemTime::now();
        let timestamp = ts
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        let timestamp = Map::from_iter(vec![("timestamp".to_string(), json!(timestamp))]);
        json_result.push(timestamp);

        for i in 0..count {
            let value = self.db_pending_updates.get(&i.to_string()).unwrap();
            let json_string = value.unwrap().to_vec();
            let res_vec: Vec<serde_json::Map<String, Value>> =
                serde_json::from_slice(&json_string).unwrap();

            json_result.extend(res_vec);
        }

        let serialized_data = serde_json::to_vec(&json_result).unwrap();

        self.db_pending_updates.clear().unwrap();

        if finalizing_batch {
            self.db_pending_updates
                .insert("db_index".to_string(), serde_json::to_vec(&0).unwrap())
                .unwrap();
        } else {
            self.db_pending_updates
                .insert(
                    "db_index".to_string(),
                    serde_json::to_vec(&(db_index + 1)).unwrap(),
                )
                .unwrap();
        }

        return Some(upload_file_to_storage(
            format!("tx_batches/pending/{}", db_index),
            serialized_data,
        ));
    }

    // * PROCESSED DEPOSITS ——————————————————————————————————————————————————————————————- //

    /// This is called by an external designated service that listens for onchain actions
    /// and stores the type of the onchain action and the hash commitment to the data.
    pub fn register_onchain_action(
        &self,
        action_type: OnchainActionType,
        data_id: u64,
        data_commitment: BigUint,
    ) {
        self.registerd_onchain_actions_db
            .insert(
                data_id.to_string(),
                serde_json::to_vec(&(action_type, data_commitment.to_string())).unwrap(),
            )
            .unwrap();
    }

    pub fn does_commitment_exists(
        &self,
        action_type: OnchainActionType,
        data_id: u64,
        data_commitment: &BigUint,
    ) -> bool {
        let doc_ref = self.registerd_onchain_actions_db.get(data_id.to_string());

        if let Err(_) = doc_ref {
            return false;
        } else if doc_ref.as_ref().unwrap().is_none() {
            return false;
        }
        let doc_ref = doc_ref.unwrap().unwrap();

        let (s_action_type, s_data_commitment): (OnchainActionType, String) =
            serde_json::from_slice(&doc_ref.to_vec()).unwrap();

        return s_action_type == action_type && s_data_commitment == data_commitment.to_string();
    }

    pub fn remove_onchain_action_commitment(&self, data_id: u64) {
        let _ = self
            .registerd_onchain_actions_db
            .remove(data_id.to_string());
    }

    // * BATCH TRANSITION ————————————————————————————————————————————————————————————————- //

    pub fn store_batch_transition_info(&self, batch_transition_info: &BatchTransitionInfo) {
        self.batch_transition_info_db
            .insert(
                &self.latest_batch.to_string(),
                serde_json::to_vec(batch_transition_info).unwrap(),
            )
            .unwrap();
    }

    pub fn read_batch_transition_info(&self, batch_index: u32) -> Option<BatchTransitionInfo> {
        let batch_transition_info = self
            .batch_transition_info_db
            .get(&batch_index.to_string())
            .unwrap();

        if let None = batch_transition_info {
            return None;
        }

        let batch_transition_info: BatchTransitionInfo =
            serde_json::from_slice(&batch_transition_info.unwrap().to_vec()).unwrap();

        Some(batch_transition_info)
    }

    // *

    pub fn store_accumulated_hashes(
        &self,
        accumulated_deposit_hashes: &HashMap<u32, BigUint>,
        accumulated_withdrawal_hashes: &HashMap<u32, BigUint>,
    ) {
        self.batch_transition_info_db
            .insert(
                self.latest_batch.to_string() + "-accumulated_deposit_hashes",
                serde_json::to_vec(accumulated_deposit_hashes).unwrap(),
            )
            .unwrap();

        self.batch_transition_info_db
            .insert(
                self.latest_batch.to_string() + "-accumulated_withdrawal_hashes",
                serde_json::to_vec(accumulated_withdrawal_hashes).unwrap(),
            )
            .unwrap();
    }

    pub fn read_accumulated_hashes(
        &self,
        batch_index: u32,
    ) -> (HashMap<u32, BigUint>, HashMap<u32, BigUint>) {
        let accumulated_deposit_hashes_info = self
            .batch_transition_info_db
            .get(batch_index.to_string() + "-accumulated_deposit_hashes")
            .unwrap();

        let accumulated_withdrawal_hashes_info = self
            .batch_transition_info_db
            .get(batch_index.to_string() + "-accumulated_withdrawal_hashes")
            .unwrap();

        let mut accumulated_deposit_hashes: HashMap<u32, BigUint> = HashMap::new();
        if let Some(acc_hashes) = accumulated_deposit_hashes_info {
            accumulated_deposit_hashes = serde_json::from_slice(&acc_hashes.to_vec()).unwrap();
        }

        let mut accumulated_withdrawal_hashes: HashMap<u32, BigUint> = HashMap::new();
        if let Some(acc_hashes) = accumulated_withdrawal_hashes_info {
            accumulated_withdrawal_hashes = serde_json::from_slice(&acc_hashes.to_vec()).unwrap();
        }

        (accumulated_deposit_hashes, accumulated_withdrawal_hashes)
    }

    // *

    /// Clears the storage to make room for the next batch.
    ///
    pub fn transition_to_new_batch(
        &mut self,
    ) -> Option<impl std::future::Future<Output = StorageResult>> {
        let new_batch_index = self.latest_batch + 1;

        if new_batch_index >= 5 {
            // ? delete the oldest batch
            let oldest_batch_index = new_batch_index - 5;

            let _ = fs::remove_dir_all(
                "storage/transaction_data/".to_string() + &oldest_batch_index.to_string(),
            );
            let _ = self
                .batch_transition_info_db
                .remove(&oldest_batch_index.to_string());
        }

        let config = Config::new()
            .path("./storage/transaction_data/".to_string() + &new_batch_index.to_string());
        let tx_db = config.open().unwrap();

        self.tx_db = tx_db;
        self.latest_batch = new_batch_index;

        return self.process_pending_batch_updates(true);
    }
}
