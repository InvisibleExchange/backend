const grpc = require("@grpc/grpc-js");
const protoLoader = require("@grpc/proto-loader");

const path = require("path");
const dotenv = require("dotenv");

const packageDefinition = protoLoader.loadSync(
  path.join(__dirname, "../../../invisible_backend/proto/engine.proto"),
  { keepCase: true, longs: String, enums: String, defaults: true, oneofs: true }
);
const engine = grpc.loadPackageDefinition(packageDefinition).engine;

const SERVER_URL = "localhost:50052";

let client = new engine.Engine(SERVER_URL, grpc.credentials.createInsecure());

dotenv.config({ path: path.join(__dirname, "../.env") });

const exchange_config = require("../../../exchange-config.json");

const PRICE_DECIMALS_PER_ASSET = exchange_config["PRICE_DECIMALS_PER_ASSET"];
const SYMBOLS_TO_IDS = exchange_config["SYMBOLS_TO_IDS"];

const { getKeyPair, sign } = require("starknet").ec;

/**
 *
 * @param {"btcusd" / "ethusd"} symbol
 */

async function getOracleUpdate(token, price_) {
  token = SYMBOLS_TO_IDS[token];

  let price = Number.parseInt(price_ * 10 ** PRICE_DECIMALS_PER_ASSET[token]);

  let timestamp = Math.floor(Date.now() / 1000);

  let msg =
    (BigInt(price) * 2n ** 64n + BigInt(token)) * 2n ** 64n + BigInt(timestamp);

  let keyPair = getKeyPair("0x1");
  let sig = sign(keyPair, msg.toString(16));

  let oracleUpdate = {
    token: token,
    timestamp: timestamp,
    observer_ids: [0],
    prices: [price],
    signatures: [{ r: sig[0], s: sig[1] }],
  };

  return oracleUpdate;
}

function runIndexPriceUpdator(PRICE_FEEDS) {
  setInterval(async () => {
    // Call an API here

    let updates = [];
    for (let [token, priceInfo] of Object.entries(PRICE_FEEDS)) {
      let price = Number(priceInfo.price);

      let update = await getOracleUpdate(token, price);
      if (update) {
        updates.push(update);
      }
    }
    if (updates.length == 0) {
      return;
    }

    client.update_index_price(
      { oracle_price_updates: updates },
      function (err, response) {
        if (err) {
          console.log(err);
        }
      }
    );

    //
  }, 5_000);
}

// runIndexPriceUpdator();

module.exports = {
  runIndexPriceUpdator,
};
