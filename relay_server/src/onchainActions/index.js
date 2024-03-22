// TODO: When the batch is settled send the program_output to the prover

const { downloadInteractionsFile } = require("../helpers/firebase/readStorage");
const { processL2Interactions } = require("./processL2Interactions");
const { relayAccumulatedHashes } = require("./relayMessages");
const { transitionBatch, getProgramOutput } = require("./transitionBatch");

const ethers = require("ethers");

const path = require("path");

// TODO: After the proof is generated fetch the program_output (Need Starkware for this)

// TODO: Use the output to call transition_state on the smart contracts

// TODO: Relay the accumulated state to the L2s

// TODO: Process the interactions on the L2s

async function executeL1Actions(txBatchId) {
  let chainConfig = require(path.join(__dirname, "../../address-config.json"));

  let invisibleAddress = chainConfig["L1"]["Invisible"];
  let programOutput = await getProgramOutput(txBatchId);
  let receipt = await transitionBatch(invisibleAddress, programOutput);

  console.log("\n\n========= ============ ============== ==========\n\n");

  const relayAddress = chainConfig["L1"]["MessageRelay"];
  const destinationIds = [40231];
  let receipt2 = await relayAccumulatedHashes(
    relayAddress,
    txBatchId,
    destinationIds
  );
}

async function executeL2Actions(txBatchId, chainId) {
  let chainConfig = require(path.join(__dirname, "../../address-config.json"));

  const chainName = chainConfig.chainIdToName[chainId];
  let invisibleAddress = chainConfig[chainName]["Invisible"];
  let messageRelayAddress = chainConfig[chainName]["MessageRelay"];

  let { depositOutputs, withdrawalOutputs } = await downloadInteractionsFile(
    txBatchId
  );

  let chainRpcEnvName = chainConfig.rpcs[chainName] ?? "";
  let chainRpc = process.env[chainRpcEnvName] ?? "";
  let provider = new ethers.JsonRpcProvider(chainRpc);

  let receipts = await processL2Interactions(
    provider,
    invisibleAddress,
    messageRelayAddress,
    txBatchId,
    depositOutputs[chainId],
    withdrawalOutputs[chainId]
  );
}

let txBatchId = 2;
// executeL1Actions(txBatchId);

let chainId = "40231";
executeL2Actions(txBatchId, chainId);
