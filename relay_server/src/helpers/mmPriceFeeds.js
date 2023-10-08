const WebSocket = require("ws");
// const fetch = require("node-fetch");
const axios = require("axios");

const path = require("path");
const dotenv = require("dotenv");
dotenv.config({ path: path.join(__dirname, "../.env") });

const MM_CONFIG = [
  {
    symbol: "BTC",
    name: "bitcoin",
    coinmarketcapId: 1,
  },
  {
    symbol: "ETH",
    name: "ethereum",
    coinmarketcapId: 1027,
  },
  {
    symbol: "SOL",
    name: "solana",
    coinmarketcapId: 5426,
  },
];

async function priceUpdate(PRICE_FEEDS, MM_CONFIG) {
  // get a random number between 0 and 4
  let randIdx = Math.floor(Math.random() * 5);

  try {
    await _priceUpdateInner(PRICE_FEEDS, MM_CONFIG, randIdx);
  } catch (_) {
    try {
      await _priceUpdateInner(PRICE_FEEDS, MM_CONFIG, (randIdx + 2) % 5);
    } catch (_) {
      try {
        await _priceUpdateInner(PRICE_FEEDS, MM_CONFIG, (randIdx + 4) % 5);
      } catch (error) {
        console.log("Error fetching prices:", error);
      }
    }
  }
}

async function _priceUpdateInner(PRICE_FEEDS, MM_CONFIG, idx) {
  if (idx === 0) {
    await fetchCoinmarketCapPrices(PRICE_FEEDS, MM_CONFIG);
  } else if (idx === 2) {
    await fetchCoinGeckoPrices(PRICE_FEEDS, MM_CONFIG);
  } else {
    await fetchCoinCapPrices(PRICE_FEEDS, MM_CONFIG);
  }
}

async function fetchCoinmarketCapPrices(PRICE_FEEDS) {
  let coinmarketcapIds = []; //BTC, ETH, SOL

  for (let config of MM_CONFIG) {
    coinmarketcapIds.push(config.coinmarketcapId);
  }

  const url =
    "https://pro-api.coinmarketcap.com/v2/cryptocurrency/quotes/latest";
  const headers = {
    "X-CMC_PRO_API_KEY": process.env.CMC_API_KEY.toString(),
    Accept: "application/json",
  };
  const params = {
    id: coinmarketcapIds.join(","),
    convert: "USD",
  };

  let response = await axios
    .get(url, { headers, params })
    .then((r) => r.data.data)
    .catch((e) => console.log(e));

  for (let cmId of coinmarketcapIds) {
    let assetRes = response[cmId];

    let x = assetRes.quote.USD;

    let price = Number(x.price);
    let percentage = Number(x.percent_change_24h);
    let absolute = Number(price * (percentage / 100));

    PRICE_FEEDS[assetRes.symbol] = {
      percentage,
      absolute,
      price,
    };
  }

  // console.log("coinmarketcap", PRICE_FEEDS);
}

async function fetchCoinGeckoPrices(PRICE_FEEDS) {
  let coingeckoIds = []; //BTC, ETH, SOL

  for (let config of MM_CONFIG) {
    coingeckoIds.push(config.name);
  }

  let idString = coingeckoIds.join("%2C");
  const url = `https://api.coingecko.com/api/v3/coins/markets?vs_currency=usd&ids=${idString}&order=market_cap_desc&locale=en`;
  const headers = {
    Accept: "application/json",
  };

  let response = await axios
    .get(url, { headers })
    .then((r) => r.data)
    .catch((e) => console.log(e));

  for (let assetRes of response) {
    let price = Number(assetRes.current_price);
    let absolute = Number(assetRes.price_change_24h);
    let percentage = Number(assetRes.price_change_percentage_24h);

    PRICE_FEEDS[assetRes.symbol.toUpperCase()] = {
      percentage,
      absolute,
      price,
    };
  }

  // console.log("coingecko", PRICE_FEEDS);
}

async function fetchCoinCapPrices(PRICE_FEEDS) {
  const url = "https://api.coincap.io/v2/assets";
  const headers = {
    "Accept-Encoding": "gzip",
    Authorization: "Bearer " + process.env.CC_API_KEY.toString(),
    Accept: "application/json",
  };

  const idsString = MM_CONFIG.map((config) => config.name).join(",");
  const params = {
    ids: idsString,
  };

  let response = await axios
    .get(url, { headers, params })
    .then((r) => r.data.data)
    .catch((e) => console.log(e));

  for (let assetRes of response) {
    let price = Number(assetRes.priceUsd);
    let percentage = Number(assetRes.changePercent24Hr);
    let absolute = Number(price * (percentage / 100));

    PRICE_FEEDS[assetRes.symbol.toUpperCase()] = {
      percentage,
      absolute,
      price,
    };
  }

  // console.log("coincap ", PRICE_FEEDS);

  //
}

// fetchCoinmarketCapPrices({});
// fetchCoinGeckoPrices({});
// fetchCoinCapPrices({});

module.exports = {
  priceUpdate,
};
