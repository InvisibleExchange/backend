const grpc = require("@grpc/grpc-js");
const protoLoader = require("@grpc/proto-loader");
const { getLastDayTrades } = require("./helpers/firebase/firebaseConnection");
const {
  fetchAndCompareDbAndBackendStates,
} = require("./helpers/firebase/compareStates");

const packageDefinition = protoLoader.loadSync(
  "../../invisible_backend/proto/engine.proto",
  { keepCase: true, longs: String, enums: String, defaults: true, oneofs: true }
);
const engine = grpc.loadPackageDefinition(packageDefinition).engine;

const SERVER_URL = "localhost:50052";
// const SERVER_URL = "54.212.28.196:50052";

const client = new engine.Engine(SERVER_URL, grpc.credentials.createInsecure());

async function finalizeBatch() {
  client.finalize_batch({}, function (err, response) {
    if (err) {
      console.log(err);
    } else {
      console.log(response);
    }
  });

  // ========================
}

async function getStateInfo() {
  client.get_state_info({}, function (err, response) {
    if (err) {
      console.log(err);
    } else {
      // console.log(response);

      for (let i = 0; i < response.state_tree.length; i++) {
        const element = response.state_tree[i];
        console.log(i, "-", element);
      }
    }
  });
}

// ===========================
async function getFundingInfo() {
  client.get_funding_info({}, function (err, response) {
    if (err) {
      console.log(err);
    } else {
      console.log(response.fundings);
    }
  });
}

// ===========================
async function updateInconsistentState() {
  let invalid_indexes = ["0", "4"];

  client.update_invalid_state_indexes(
    {
      invalid_indexes,
    },
    function (err, response) {
      if (err) {
        console.log(err);
      } else {
        console.log(response);
      }
    }
  );
}

// ===========================

finalizeBatch();

// fetchAndCompareDbAndBackendStates();

// updateInconsistentState();

// getStateInfo();
