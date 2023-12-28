const grpc = require("@grpc/grpc-js");
const protoLoader = require("@grpc/proto-loader");

const path = require("path");
const protoPath = path.join(
  __dirname,
  "../../../invisible_backend/proto",
  "engine.proto"
);

const SERVER_URL = "localhost:50052";

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

const invisibleL1Address = exchange_config["INVISIBL1_ETH_ADDRESS"];
const invisibleL1Abi = require("../abis/Invisible.json").abi;

const invisibleL1Contract = new ethers.Contract(
  invisibleL1Address,
  invisibleL1Abi,
  provider
);

// * * //


async function initListeners() {}
