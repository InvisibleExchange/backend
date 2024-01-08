const ethers = require("ethers");
const grpc = require("@grpc/grpc-js");
const protoLoader = require("@grpc/proto-loader");

const path = require("path");
const { listenForDeposits } = require("./depositListener");
const { listenForEscapes } = require("./escapesListener");
const { listenForMMActions } = require("./mmRegistryListener");
const { initDb } = require("../helpers/localStorage");
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
const provider = new ethers.providers.JsonRpcProvider(
  "https://ethereum-sepolia.publicnode.com",
  "sepolia"
);

const exchange_config = require("../../../exchange-config.json");
const invisibleL1Address = exchange_config["INVISIBL1_ETH_ADDRESS"];
const invisibleL1Abi = require("../abis/Invisible.json").abi;

const invisibleL1Contract = new ethers.Contract(
  invisibleL1Address,
  invisibleL1Abi,
  provider
);

const escapeVerifierAddress = exchange_config["ESCAPE_VERIFIER_ETH_ADDRESS"];
const escapeVerifierAbi = require("../abis/EscapeVerifier.json").abi;

const escapeVerifierContract = new ethers.Contract(
  escapeVerifierAddress,
  escapeVerifierAbi,
  provider
);

// * * //

async function initListeners(db) {
  // ? Listen and handle onchain deposits
  await listenForDeposits(db, client, invisibleL1Contract);

  // ? Listen and handle onchain escapes
  await listenForEscapes(db, client, escapeVerifierContract);

  // ? Listen and handle onchain MM actions
  await listenForMMActions(db, client, invisibleL1Contract);
}

// TODO: FOR TESTING
async function main() {
  const db = initDb();

  await initListeners(db);
}

main();

module.exports = {
  initListeners,
};
