use std::{collections::HashMap, fs, time::SystemTime};

use serde_json::{json, to_vec, Map, Value};

use sled::{Config, Result};

use crate::{
    order_tab::OrderTab,
    perpetual::perp_position::PerpPosition,
    transaction_batch::tx_batch_structs::OracleUpdate,
    transactions::transaction_helpers::transaction_output::{FillInfo, PerpFillInfo},
};

use super::{super::notes::Note, firestore::upload_file_to_storage};

type StorageResult = std::result::Result<(), Box<dyn std::error::Error>>;

/// The main storage struct that stores all the data on disk.
pub struct MainStorage {
    pub tx_db: sled::Db,
    pub price_db: sled::Db,
    pub funding_db: sled::Db,
    pub processed_deposits_db: sled::Db, // Deposit ids of all the deposits that were processed so far
    pub latest_batch: u32,               // every transaction batch stores data separately
    pub db_pending_updates: sled::Db, // small batches of txs that get pushed to the db periodically
}

impl MainStorage {
    pub fn new() -> Self {
        let dir = fs::read_dir("storage/transaction_data");

        let batch_index = match dir {
            Ok(dir) => {
                dir.filter(|entry| entry.as_ref().map(|e| e.path().is_dir()).unwrap_or(false))
                    .count()
                    - 1
            }
            Err(_) => 0,
        };

        let config = Config::new()
            .path("./storage/transaction_data/".to_string() + &batch_index.to_string());
        let tx_db = config.open().unwrap();

        let config =
            Config::new().path("./storage/price_data/".to_string() + &batch_index.to_string());
        let price_db = config.open().unwrap();

        let config = Config::new().path("./storage/funding_info".to_string());
        let funding_db = config.open().unwrap();

        let config = Config::new().path("./storage/processed_deposits".to_string());
        let processed_deposits_db = config.open().unwrap();

        let config = Config::new().path("./storage/db_pending_updates".to_string());
        let db_pending_updates = config.open().unwrap();

        MainStorage {
            tx_db,
            price_db,
            funding_db,
            processed_deposits_db,
            latest_batch: batch_index as u32,
            db_pending_updates,
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

    /// Gets a batch of the latest 15-20 transactions that were executed
    /// and stores them on disk.
    ///
    /// # Arguments
    /// * swap_output_json - a vector of the latest 15-20 transactions as json maps
    /// * index - the index of the micro batch in the current batch
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

    //
    //
    //
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

        let serialized_data = to_vec(&json_result).unwrap();

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

    // * PROCESSED DEPOSITS ——————————————————————————————————————————————————————————————- //

    /// Store a deposit id of a deposit that was processed to make sure it doesn't get executed twice
    pub fn store_processed_deposit_id(&self, deposit_id: u64) {
        self.processed_deposits_db
            .insert(deposit_id.to_string(), serde_json::to_vec(&true).unwrap())
            .unwrap();

        // println!(
        //     "stored_val new after deposit: {:?}",
        //     self.processed_deposits_db.iter().for_each(|val| {
        //         println!("{:?}", val);
        //     })
        // );
    }

    pub fn is_deposit_already_processed(&self, deposit_id: u64) -> bool {
        let is_processsed: bool = self
            .processed_deposits_db
            .get(deposit_id.to_string())
            .unwrap_or_default()
            .is_some();

        is_processsed
    }

    // * BATCH TRANSITION ————————————————————————————————————————————————————————————————- //

    /// Clears the storage to make room for the next batch.
    ///
    pub fn transition_to_new_batch(
        &mut self,
    ) -> Option<impl std::future::Future<Output = StorageResult>> {
        let new_batch_index = self.latest_batch + 1;

        let config = Config::new()
            .path("./storage/transaction_data/".to_string() + &new_batch_index.to_string());
        let tx_db = config.open().unwrap();

        // Todo: We could store funding and pricing info in the db
        // let price_data = self.read_price_data();
        // let funding_info = self.read_funding_info().ok();

        self.tx_db = tx_db;
        self.latest_batch = new_batch_index;

        return self.process_pending_batch_updates(true);
    }
}

/// This stores info about failed database updates
pub struct BackupStorage {
    note_db: sled::Db,                // For failed note updates
    removable_notes_db: sled::Db,     // For failed removable notes updates
    position_db: sled::Db,            // For failed position updates
    removable_positions_db: sled::Db, // For failed removable positions updates
    order_tab_db: sled::Db,           // For failed order tab updates
    removable_order_tab_db: sled::Db, // For failed removable order tab updates
    fills_db: sled::Db,               // For failed spot fills updates
    perp_fills_db: sled::Db,          // For failed perp fills updates
                                      // rollback_db: sled::Db,            // For rollback transactions
}

impl BackupStorage {
    pub fn new() -> Self {
        let config = Config::new().path("./storage/backups/notes");
        let note_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/removable_notes");
        let removable_notes_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/positions");
        let position_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/removable_positions");
        let removable_positions_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/order_tab");
        let order_tab_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/removable_order_tab");
        let removable_order_tab_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/fills");
        let fills_db = config.open().unwrap();

        let config = Config::new().path("./storage/backups/perp_fills");
        let perp_fills_db = config.open().unwrap();

        // let config = Config::new().path("./storage/rollback_info");
        // let rollback_db = config.open().unwrap();

        BackupStorage {
            note_db,
            removable_notes_db,
            position_db,
            removable_positions_db,
            fills_db,
            perp_fills_db,
            // rollback_db,
            order_tab_db,
            removable_order_tab_db,
        }
    }

    /// Stores a failed note update in the database.
    pub fn store_note(&self, note: &Note) -> Result<()> {
        // for x in self.note_db.iter() {}

        let idx = note.index;
        let note = serde_json::to_vec(note).unwrap();

        self.note_db.insert(idx.to_string(), note)?;

        Ok(())
    }

    pub fn store_note_removal(&self, idx: u64, address: &str) -> Result<()> {
        let info = serde_json::to_vec(&(idx, address)).unwrap();

        self.removable_notes_db.insert(idx.to_string(), info)?;

        Ok(())
    }

    /// Reads the notes that failed to be stored in the database.
    pub fn read_notes(&self) -> (Vec<Note>, Vec<(u64, String)>) {
        let mut notes = Vec::new();
        for x in self.note_db.iter() {
            let n = x.unwrap().1.to_vec();
            let note: Note = serde_json::from_slice(&n).unwrap();
            notes.push(note);
        }

        let mut removable_info = Vec::new();
        for x in self.removable_notes_db.iter() {
            let info: (u64, String) = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();

            removable_info.push(info);
        }

        (notes, removable_info)
    }

    pub fn store_position(&self, position: &PerpPosition) -> Result<()> {
        // for x in self.position_db.iter() {}

        let idx = position.index;
        let position = serde_json::to_vec(position).unwrap();

        self.position_db.insert(idx.to_string(), position)?;

        Ok(())
    }

    pub fn store_position_removal(&self, idx: u64, address: &str) -> Result<()> {
        let info = serde_json::to_vec(&(idx, address)).unwrap();

        self.removable_positions_db.insert(idx.to_string(), info)?;

        Ok(())
    }

    pub fn read_positions(&self) -> (Vec<PerpPosition>, Vec<(u64, String)>) {
        let mut positions = Vec::new();
        for x in self.position_db.iter() {
            let position: PerpPosition = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();
            positions.push(position);
        }

        let mut removable_info = Vec::new();
        for x in self.removable_positions_db.iter() {
            let info: (u64, String) = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();

            removable_info.push(info);
        }

        (positions, removable_info)
    }

    pub fn store_spot_fill(&self, fill: &FillInfo) -> Result<()> {
        // for x in self.fills_db.iter() {}

        let mut key = fill.user_id_a.clone();
        key.push_str(&fill.user_id_b);
        key.push_str(&fill.timestamp.to_string());
        let fill = serde_json::to_vec(fill).unwrap();

        self.fills_db.insert(key, fill)?;

        Ok(())
    }

    pub fn read_spot_fills(&self) -> Vec<FillInfo> {
        let mut fills = Vec::new();

        for x in self.fills_db.iter() {
            let fill: FillInfo = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();
            fills.push(fill);
        }

        fills
    }

    pub fn store_perp_fill(&self, fill: &PerpFillInfo) -> Result<()> {
        // for x in self.fills_db.iter() {}

        let mut key = fill.user_id_a.clone();
        key.push_str(&fill.user_id_b);
        key.push_str(&fill.timestamp.to_string());
        let fill = serde_json::to_vec(fill).unwrap();

        self.perp_fills_db.insert(key, fill)?;

        Ok(())
    }

    pub fn read_perp_fills(&self) -> Vec<PerpFillInfo> {
        let mut fills = Vec::new();

        for x in self.perp_fills_db.iter() {
            let fill: PerpFillInfo = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();
            fills.push(fill);
        }

        fills
    }

    // // TODO:
    // pub fn store_spot_rollback(&self, thread_id: u64, rollback: &RollbackInfo) -> Result<()> {
    //     // for x in self.fills_db.iter() {}
    //     // self.rollback_db.insert(key, fill)?;
    //     Ok(())
    // }

    pub fn store_order_tab(&self, order_tab: &OrderTab) -> Result<()> {
        let idx = order_tab.tab_idx;
        let tab = serde_json::to_vec(order_tab).unwrap();

        self.order_tab_db.insert(idx.to_string(), tab)?;

        Ok(())
    }

    pub fn store_order_tab_removal(&self, idx: u64, pub_key: &str) -> Result<()> {
        let info = serde_json::to_vec(&(idx, pub_key)).unwrap();

        self.removable_order_tab_db.insert(idx.to_string(), info)?;

        Ok(())
    }

    pub fn read_order_tabs(&self) -> (Vec<OrderTab>, Vec<(u64, String)>) {
        let mut order_tabs = Vec::new();
        for x in self.order_tab_db.iter() {
            let position: OrderTab = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();
            order_tabs.push(position);
        }

        let mut removable_info = Vec::new();
        for x in self.removable_order_tab_db.iter() {
            let info: (u64, String) = serde_json::from_slice(&x.unwrap().1.to_vec()).unwrap();

            removable_info.push(info);
        }

        (order_tabs, removable_info)
    }

    pub fn clear_db(&self) -> Result<()> {
        self.note_db.clear()?;
        self.position_db.clear()?;
        self.fills_db.clear()?;
        self.perp_fills_db.clear()?;

        Ok(())
    }
}
