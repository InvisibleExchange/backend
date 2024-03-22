const ethers = require("ethers");
const grpc = require("@grpc/grpc-js");
const protoLoader = require("@grpc/proto-loader");

const path = require("path");
const { listenForDeposits } = require("./depositListener");
const { listenForEscapes } = require("./escapesListener");
const { listenForMMActions } = require("./mmRegistryListener");
const protoPath = path.join(
  __dirname,
  "../../../invisible_backend/proto",
  "engine.proto"
);

const SERVER_URL = "localhost";

// * Get a connection to the backend through grpc
const packageDefinition = protoLoader.loadSync(protoPath, {
  keepCase: true,
  longs: String,
  enums: String,
  defaults: true,
  oneofs: true,
});
const engine = grpc.loadPackageDefinition(packageDefinition).engine;

const client = new engine.Engine(
  `${SERVER_URL}:50052`,
  grpc.credentials.createInsecure()
);

// * Get a connection to the smart contract
const provider = new ethers.JsonRpcProvider(
  process.env.SEPOLIA_RPC_URL ?? "",
  "sepolia"
);

const arbProvider = new ethers.JsonRpcProvider(
  process.env.ARB_SEPOLIA_RPC_URL ?? ""
);

const addressConfig = require("../../address-config.json");
const invisibleL1Address = addressConfig["L1"]["Invisible"];
const invisibleL1Abi = require("../abis/InvisibleL1.json").abi;

const invisibleL1Contract = new ethers.Contract(
  invisibleL1Address,
  invisibleL1Abi,
  provider
);

const invisibleL2Address = addressConfig["Arbitrum"]["Invisible"];
const invisibleL2Abi = require("../abis/InvisibleL2.json").abi;

const invisibleL2Contract = new ethers.Contract(
  invisibleL2Address,
  invisibleL2Abi,
  arbProvider
);

const escapeVerifierAddress = addressConfig["L1"]["EscapeVerifier"];
const escapeVerifierAbi = require("../abis/EscapeVerifier.json").abi;

const escapeVerifierContract = new ethers.Contract(
  escapeVerifierAddress,
  escapeVerifierAbi,
  provider
);

// * * //

function initListeners(db) {
  // ? Listen and handle onchain deposits
  listenForDeposits(db, client, invisibleL1Contract, true);

  // ? Listen and handle L2 onchain deposits
  listenForDeposits(db, client, invisibleL2Contract, false);

  // ? Listen and handle onchain escapes
  listenForEscapes(db, client, escapeVerifierContract);

  // ? Listen and handle onchain MM actions
  listenForMMActions(db, client, invisibleL1Contract);
}

async function getGasPrice(chainId) {
  // TODO: This should get somekind of a moving average

  if (chainId == 40161) {
    return (await provider.getFeeData()).gasPrice;
  } else if (chainId == 40231) {
    return (await arbProvider.getFeeData()).gasPrice;
  }
}

module.exports = {
  initListeners,
  getGasPrice,
};
