const path = require("path");
const { restoreOrderbooks } = require("./restoreOrderBooks");
function storeSpotOrder(db, order_id, orderObject) {
  let command = `
    INSERT OR REPLACE INTO spotOrders
      (order_id, expiration_timestamp, token_spent, token_received, amount_spent, amount_received,
      fee_limit, spot_note_info, order_tab, signature, user_id) 
    VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
    `;

  try {
    db.run(command, [
      order_id,
      orderObject.expiration_timestamp,
      orderObject.token_spent,
      orderObject.token_received,
      orderObject.amount_spent,
      orderObject.amount_received,
      orderObject.fee_limit,
      // spot_note_info
      orderObject.spot_note_info
        ? JSON.stringify(orderObject.spot_note_info)
        : null,
      // order_tab
      orderObject.order_tab ? JSON.stringify(orderObject.order_tab) : null,
      //
      JSON.stringify(orderObject.signature),
      orderObject.user_id,
    ]);
  } catch (error) {
    console.log("error: ", error);
  }
}

function storePerpOrder(db, order_id, orderObject) {
  let command = `
    INSERT OR REPLACE INTO perpOrders 
      (order_id, expiration_timestamp, position, position_effect_type, order_side, synthetic_token, synthetic_amount, 
      collateral_amount, fee_limit, open_order_fields, close_order_fields, signature, user_id) 
    VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
    `;

  try {
    db.run(command, [
      order_id,
      orderObject.expiration_timestamp,
      JSON.stringify(orderObject.position),
      orderObject.position_effect_type,
      orderObject.order_side,
      orderObject.synthetic_token,
      orderObject.synthetic_amount,
      orderObject.collateral_amount,
      orderObject.fee_limit,
      JSON.stringify(orderObject.open_order_fields),
      JSON.stringify(orderObject.close_order_fields),
      JSON.stringify(orderObject.signature),
      orderObject.user_id,
    ]);
  } catch (error) {
    console.log("error: ", error);
  }
}

const sqlite3 = require("sqlite3").verbose();
function initDb() {
  const createPerpTableCommand = `
  CREATE TABLE IF NOT EXISTS perpOrders 
    (order_id INTEGER PRIMARY KEY NOT NULL, 
    expiration_timestamp INTEGER NOT NULL, 
    position TEXT, 
    position_effect_type INTEGER NOT NULL,
     order_side INTEGER NOT NULL, 
    synthetic_token INTEGER NOT NULL,
    synthetic_amount INTEGER NOT NULL, 
    collateral_amount INTEGER NOT NULL, 
    fee_limit INTEGER NOT NULL, 
    open_order_fields TEXT, 
    close_order_fields TEXT,
    signature TEXT NOT NULL, 
    user_id INTEGER )`;

  // spot_note_info = {dest_received_address, dest_received_blinding, notes_in, refund_note}
  const createSpotTableCommand = `
  CREATE TABLE IF NOT EXISTS spotOrders
  (order_id INTEGER PRIMARY KEY NOT NULL, 
  expiration_timestamp INTEGER NOT NULL,  
  token_spent INTEGER NOT NULL, 
  token_received INTEGER NOT NULL, 
  amount_spent INTEGER NOT NULL,  
  amount_received INTEGER NOT NULL,  
  fee_limit INTEGER NOT NULL, 
  spot_note_info TEXT,
  order_tab TEXT,
  signature TEXT NOT NULL, 
  user_id INTEGER )  `;

  let db = new sqlite3.Database(
    path.join(__dirname, "../orderBooks.db"),
    (err) => {
      if (err) {
        console.error(err.message);
      }
      console.log("Connected to the relay server database.");
    }
  );

  db.run(createSpotTableCommand);

  db.run(createPerpTableCommand);

  const createSpotLiquidityTableCommand =
    "CREATE TABLE IF NOT EXISTS spotLiquidity (market_id INTEGER PRIMARY KEY UNIQUE NOT NULL, bidQueue TEXT NOT NULL, askQueue TEXT NOT NULL)";
  const createPerpLiquidityTableCommand =
    "CREATE TABLE IF NOT EXISTS perpLiquidity (market_id INTEGER PRIMARY KEY UNIQUE NOT NULL, bidQueue TEXT NOT NULL, askQueue TEXT NOT NULL)";

  db.run(createSpotLiquidityTableCommand, (res, err) => {
    if (err) {
      console.log(err);
    }

    db.run(createPerpLiquidityTableCommand, (res, err) => {
      if (err) {
        console.log(err);
      }
    });
  });

  const createLiquidationTable =
    "CREATE TABLE IF NOT EXISTS liquidations (position_index INTEGER PRIMARY KEY NOT NULL, position_address TEXT NOT NULL, synthetic_token INTEGER NOT NULL, order_side BIT NOT NULL, liquidation_price INTEGER NOT NULL)";

  db.run(createLiquidationTable, (res, err) => {
    if (err) {
      console.log(err);
    }
  });

  db.run(
    `CREATE TABLE IF NOT EXISTS processedDeposits (id INTEGER PRIMARY KEY, pending TEXT, processed TEXT)`,
    (res, err) => {
      if (err) {
        console.log(err);
      }
    }
  );

  return db;
}

function initLiquidity(db) {
  const SPOT_MARKET_IDS = {
    BTCUSD: 11,
    ETHUSD: 12,
  };

  const PERP_MARKET_IDS = {
    BTCUSD: 21,
    ETHUSD: 22,
    PEPEUSD: 23,
  };

  // & Restore liquidity from database
  restoreOrderbooks(db);

  // & Create liquidity if it does not exist
  for (let marketId of Object.values(SPOT_MARKET_IDS)) {
    // Check if liquidity already exists
    const query = `SELECT * FROM spotLiquidity WHERE market_id = ${marketId}`;
    db.all(query, [], (err, rows) => {
      if (err) {
        console.error(err.message);
        return;
      }

      if (rows && rows.length == 0) {
        // Liquidity does not exist, so create it
        db.run(
          "INSERT INTO spotLiquidity (market_id, bidQueue, askQueue) VALUES($1, $2, $3)",
          [marketId, JSON.stringify([]), JSON.stringify([])]
        );
      }
    });
  }

  for (let marketId of Object.values(PERP_MARKET_IDS)) {
    // Check if liquidity already exists
    const query = `SELECT * FROM perpLiquidity WHERE market_id = ${marketId}`;
    db.all(query, [], (err, rows) => {
      if (err) {
        console.error(err.message);
        return;
      }

      if (rows && rows.length == 0) {
        // Liquidity does not exist, so create it
        db.run(
          "INSERT INTO perpLiquidity (market_id, bidQueue, askQueue) VALUES($1, $2, $3)",
          [marketId, JSON.stringify([]), JSON.stringify([])]
        );
      }
    });
  }
}

function updateStoredDepositIds(db, pendingDepositIds, processedDepositIds) {
  const stmt = db.prepare(
    "INSERT OR REPLACE INTO processedDeposits (id, pending, processed) VALUES (?, ?, ?)"
  );

  console.log("---storgin pendingDepositIds: ", pendingDepositIds);
  console.log("---storgin processedDepositIds: ", processedDepositIds);
  stmt.run(
    1,
    JSON.stringify(pendingDepositIds),
    JSON.stringify(processedDepositIds)
  );
  stmt.finalize();
}

function getProcessedDeposits(db, callback) {
  db.get(
    "SELECT pending, processed FROM processedDeposits WHERE id = ?",
    1,
    (err, row) => {
      if (err) {
        return callback(err);
      }
      if (row) {
        const pendingArray = JSON.parse(row.pending);
        const processedArray = JSON.parse(row.processed);
        return callback(null, pendingArray, processedArray);
      }
      return callback(null, []);
    }
  );
}

module.exports = {
  initDb,
  initLiquidity,
  storeSpotOrder,
  storePerpOrder,
  updateStoredDepositIds,
  getProcessedDeposits,
};
