let activeOrderIds = {}; // {market_id: [orderId1, orderId2, ...]}

// WEBSOCKETS

class OrderBook {
  constructor(marketId, isPerp) {
    this.market_id = marketId;
    this.is_perp = isPerp;
    this.bid_queue = []; // [price, size, timestamp]
    this.ask_queue = []; // [price, size, timestamp]
    this.prev_bid_queue = []; // [price, size, timestamp]
    this.prev_ask_queue = []; // [price, size, timestamp]
  }

  updateAndCompare() {
    let isBidQueueSame = this.bid_queue.equals(this.prev_bid_queue);
    let isAskQueueSame = this.ask_queue.equals(this.prev_ask_queue);

    this.prev_ask_queue = this.ask_queue;
    this.prev_bid_queue = this.bid_queue;

    return {
      bidQueue: this.bid_queue,
      askQueue: this.ask_queue,
      isBidQueueSame,
      isAskQueueSame,
    };
  }
}

function initOrderBooks() {
  const markets = [11, 12];
  const perpMarkets = [21, 22, 23];

  let orderBooks = {};
  for (let market of markets) {
    orderBooks[market] = new OrderBook(market, false);
  }
  for (let market of perpMarkets) {
    orderBooks[market] = new OrderBook(market, true);
  }
  return orderBooks;
}

function listenToLiquidityUpdates(e, db, orderBooks, fillUpdates) {
  let msg = JSON.parse(e.data);

  if (msg.message_id == "LIQUIDITY_UPDATE") {
    for (let liq_msg of msg.liquidity) {
      let book = orderBooks[liq_msg.market];

      book.bid_queue = liq_msg.bid_liquidity;
      book.ask_queue = liq_msg.ask_liquidity;

      let newActiveOrders = [];
      liq_msg.bid_liquidity.forEach((el) => {
        newActiveOrders.push(el[3]);
      });
      liq_msg.ask_liquidity.forEach((el) => {
        newActiveOrders.push(el[3]);
      });

      let bidQueue = JSON.stringify(liq_msg.bid_liquidity);
      let askQueue = JSON.stringify(liq_msg.ask_liquidity);

      let spotCommand =
        "UPDATE spotLiquidity SET bidQueue = $1, askQueue = $2 WHERE market_id = $3";
      let perpCommand =
        "UPDATE perpLiquidity SET bidQueue = $1, askQueue = $2 WHERE market_id = $3";

      if (liq_msg.type == "perpetual") {
        try {
          db.run(
            perpCommand,
            [bidQueue, askQueue, Number.parseInt(liq_msg.market)],
            function (err) {
              if (err) {
                return console.error(err.message);
              }
            }
          );
        } catch (error) {
          console.log("error: ", error);
        }
      } else {
        try {
          db.run(
            spotCommand,
            [bidQueue, askQueue, Number.parseInt(liq_msg.market)],
            function (err) {
              if (err) {
                return console.error(err.message);
              }
            }
          );
        } catch (error) {
          console.log("error: ", error);
        }
      }

      if (!activeOrderIds[liq_msg.market]) {
        activeOrderIds[liq_msg.market] = [];
      }

      // Get all orderIds from activeOrderIds[liq_msg.market] array that are not in newActiveOrders array
      let inactiveOrderIds = activeOrderIds[liq_msg.market].filter(
        (el) => !newActiveOrders.includes(el)
      );
      for (const orderId of inactiveOrderIds) {
        let spotCommand = "DELETE FROM spotOrders WHERE order_id = $1";
        let perpCommand = "DELETE FROM perpOrders WHERE order_id = $1";

        try {
          db.run(liq_msg.type == "perpetual" ? perpCommand : spotCommand, [
            orderId,
          ]);
        } catch (error) {
          console.log("error: ", error);
        }
      }
      activeOrderIds[liq_msg.market] = newActiveOrders;
    }
  } else if (msg.message_id == "SWAP_FILLED") {
    //   let json_msg = json!({
    //     "message_id": "SWAP_FILLED",
    //     "type": "spot",
    //     "asset": base_asset,
    //     "amount": qty,
    //     "price": price,
    //     "is_buy": taker_side == OBOrderSide::Bid,
    //     "timestamp": timestamp,
    //     "user_id_a": user_id_pair.0,
    //     "user_id_b": user_id_pair.1,
    // });

    fillUpdates.push(JSON.stringify(msg));
  } else if (msg.message_id == "NEW_POSITIONS") {
    // "message_id": "NEW_POSITIONS",
    // "position1": [position_address, position_index, synthetic_token, is_long, liquidation_price]
    // "position2":  [position_address, position_index, synthetic_token, is_long, liquidation_price]

    if (msg.position1) {
      let [
        position_address,
        position_index,
        synthetic_token,
        is_long,
        liquidation_price,
      ] = msg.position1;
      is_long = is_long ? 1 : 0;
      let command =
        "INSERT OR REPLACE INTO liquidations (position_index, position_address, synthetic_token, order_side, liquidation_price ) VALUES($1, $2, $3, $4, $5)";

      try {
        db.run(
          command,
          [
            position_index,
            position_address,
            synthetic_token,
            is_long,
            liquidation_price,
          ],
          function (err) {
            if (err) {
              return console.error(err.message);
            }
          }
        );
      } catch (error) {
        console.log("error: ", error);
      }
    }
    if (msg.position2) {
      let [
        position_address,
        position_index,
        synthetic_token,
        is_long,
        liquidation_price,
      ] = msg.position2;
      is_long = is_long ? 1 : 0;
      let command =
        "INSERT OR REPLACE INTO liquidations (position_index, position_address, synthetic_token, order_side, liquidation_price) VALUES($1, $2, $3, $4, $5)";

      try {
        db.run(
          command,
          [
            position_index,
            position_address,
            synthetic_token,
            is_long,
            liquidation_price,
          ],
          function (err) {
            if (err) {
              return console.error(err.message);
            }
          }
        );
      } catch (error) {
        console.log("error: ", error);
      }
    }
  }
}

function compileLiqUpdateMessage(orderBooks) {
  let updates = [];

  for (let book of Object.values(orderBooks)) {
    let { bidQueue, askQueue, isBidQueueSame, isAskQueueSame } =
      book.updateAndCompare();

    if (!isBidQueueSame || !isAskQueueSame) {
      updates.push(
        JSON.stringify({
          type: book.is_perp ? "perpetual" : "spot",
          market: book.market_id,
          bid_liquidity: !isBidQueueSame ? bidQueue : null,
          ask_liquidity: !isAskQueueSame ? askQueue : null,
        })
      );
    }
  }

  return updates;
}

// DB HELPERS ============================================================================================================================

// Warn if overriding existing method
if (Array.prototype.equals)
  console.warn(
    "Overriding existing Array.prototype.equals. Possible causes: New API defines the method, there's a framework conflict or you've got double inclusions in your code."
  );
// attach the .equals method to Array's prototype to call it on any array
Array.prototype.equals = function (array) {
  // if the other array is a falsy value, return
  if (!array) return false;
  // if the argument is the same array, we can be sure the contents are same as well
  if (array === this) return true;
  // compare lengths - can save a lot of time
  if (this.length != array.length) return false;

  for (var i = 0, l = this.length; i < l; i++) {
    // Check if we have nested arrays
    if (this[i] instanceof Array && array[i] instanceof Array) {
      // recurse into the nested arrays
      if (!this[i].equals(array[i])) return false;
    } else if (this[i] != array[i]) {
      // Warning - two different object instances will never be equal: {x:20} != {x:20}
      return false;
    }
  }
  return true;
};
// Hide method from for-in loops
Object.defineProperty(Array.prototype, "equals", { enumerable: false });

// * TESTING ========================================================

module.exports = {
  listenToLiquidityUpdates,
  compileLiqUpdateMessage,
  initOrderBooks,
};
