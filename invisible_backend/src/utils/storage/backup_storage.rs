use sled::{Config, Result};

use crate::{
    order_tab::OrderTab,
    perpetual::perp_position::PerpPosition,
    transactions::transaction_helpers::transaction_output::{FillInfo, PerpFillInfo},
};

use super::super::notes::Note;

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
