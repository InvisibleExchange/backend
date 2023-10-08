const {
  initOrderBooks,
  listenToLiquidityUpdates,
  compileLiqUpdateMessage,
} = require("./helpers");

const { getLastDayTrades } = require("./firebase/firebaseConnection");
const { priceUpdate } = require("../helpers/mmPriceFeeds");
const { listenForDeposits } = require("./depositListener");
const { runIndexPriceUpdator } = require("./indexPriceUpdator");

const CONFIG_CODE = "1234567890";
const RELAY_SERVER_ID = "43147634234";

function initServer(db, updateSpot24hInfo, updatePerp24hInfo, update24HInfo) {
  // & Init order books ==================
  const orderBooks = initOrderBooks();
  let fillUpdates = [];
  let wsConnections = [];

  // & Price Feeds ====================
  let PRICE_FEEDS = {};
  priceUpdate(PRICE_FEEDS);
  setInterval(() => {
    try {
      priceUpdate(PRICE_FEEDS);
    } catch {}
  }, 10_000);

  // & Deposits ====================
  listenForDeposits(db);

  // TODO: For testing only! ============================================
  runIndexPriceUpdator(PRICE_FEEDS);

  // & WEBSOCKET CLIENT =================
  let W3CWebSocket = require("websocket").w3cwebsocket;
  let wsClient = new W3CWebSocket(`ws://localhost:50053/`);

  wsClient.onopen = function () {
    console.log("WebSocket Client Connected");
    wsClient.send(
      JSON.stringify({ user_id: RELAY_SERVER_ID, config_code: CONFIG_CODE })
    );
  };

  wsClient.onmessage = function (e) {
    listenToLiquidityUpdates(e, db, orderBooks, fillUpdates);
  };

  // & WEBSOCKET SERVER
  const WebSocket = require("ws");
  const wss = new WebSocket.Server({ port: 4040 });
  const SEND_LIQUIDITY_PERIOD = 1000;

  wss.on("connection", (ws) => {
    ws.on("message", (message) => {});

    wsConnections.push(ws);
  });

  // ? Send the update to all connected clients
  setInterval(() => {
    try {
      let updates = compileLiqUpdateMessage(orderBooks);
      let message = JSON.stringify({
        message_id: "LIQUIDITY_UPDATE",
        liquidity_updates: updates,
      });

      let fillMessage = fillUpdates.length
        ? JSON.stringify({
            message_id: "SWAP_FILLED",
            fillUpdates: fillUpdates,
          })
        : null;

      update24HInfo(fillUpdates);
      fillUpdates = [];

      let priceChanges;
      if (Object.keys(PRICE_FEEDS).length > 0) {
        priceChanges = JSON.stringify({
          message_id: "24H_PRICE_UPDATE",
          price_changes: JSON.stringify(PRICE_FEEDS),
        });
      }

      for (const ws of wsConnections) {
        ws.send(message);
        if (fillMessage) {
          ws.send(fillMessage);
        }
        ws.send(priceChanges);
      }
    } catch {}
  }, SEND_LIQUIDITY_PERIOD);

  console.log("WebSocket server started on port 4040");

  // & Fetch 24h valoumes and trades every 15 minutes ==============================================
  getLastDayTrades(false).then((res) => {
    updateSpot24hInfo(res.token24hVolumes, res.token24hTrades);
  });

  getLastDayTrades(true).then((res) => {
    updatePerp24hInfo(res.token24hVolumes, res.token24hTrades);
  });

  // & Get funding every 1 hour  ===================================================================
}

function initFundingInfo(client, updateFundingInfo) {
  client.get_funding_info({}, function (err, response) {
    if (err) {
      console.log(err);
    } else {
      if (response.successful) {
        let rates = {};
        let prices = {};
        for (const fundingInfo of response.fundings) {
          rates[fundingInfo.token] = fundingInfo.funding_rates;
          prices[fundingInfo.token] = fundingInfo.funding_prices;
        }

        updateFundingInfo(rates, prices);
      }
    }
  });

  setInterval(() => {
    try {
      client.get_funding_info({}, function (err, response) {
        if (err) {
          console.log(err);
        } else {
          if (response.successful) {
            let rates = {};
            let prices = {};
            for (const fundingInfo of response.fundings) {
              rates[fundingInfo.token] = fundingInfo.funding_rates;
              prices[fundingInfo.token] = fundingInfo.funding_prices;
            }

            updateFundingInfo(rates, prices);
          }
        }
      });
    } catch {}
  }, 60 * 60 * 1000);
}

function initFundingInfoInterval(
  channel,
  queue,
  correlationIdToResolve,
  delegateRequest
) {
  setTimeout(() => {
    delegateRequest(
      {},
      "get_funding_info",
      channel,
      null,
      queue,
      correlationIdToResolve
    );
  }, 3000);

  setInterval(() => {
    try {
      delegateRequest(
        {},
        "get_funding_info",
        channel,
        null,
        queue,
        correlationIdToResolve
      );
    } catch {}
  }, 60 * 60 * 1000);
}

module.exports = {
  initServer,
  initFundingInfo,
  initFundingInfoInterval,
};
