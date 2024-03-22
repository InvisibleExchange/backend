const { ethers } = require("ethers");

const path = require("path");
const dotenv = require("dotenv");
dotenv.config({ path: path.join(__dirname, "../.env") });

async function processL2Interactions(
  provider, // For the correct L2 network
  invisibleL2Address,
  messageRelayAddress,
  txBatchId,
  depositRequests,
  withdrawalRequests
) {
  let privateKey = process.env.ETH_PRIVATE_KEY ?? "";
  const signer = new ethers.Wallet(privateKey, provider);

  const invisibleL2Abi = require(path.join(
    __dirname,
    "../abis/InvisibleL2.json"
  )).abi;
  const invisibleContract = new ethers.Contract(
    invisibleL2Address,
    invisibleL2Abi,
    signer ?? undefined
  );

  let gasFeeData = await provider.getFeeData();
  let overrides = {
    // gasLimit: 3_000_000,
    maxFeePerGas: gasFeeData.maxFeePerGas,
    maxPriorityFeePerGas: gasFeeData.maxPriorityFeePerGas,
  };

  let receipts = {};

  // * PROCESS DEPOSITS ---------------------------

  let areDepositsProcessed = await areHashesProcessed(
    signer,
    messageRelayAddress,
    txBatchId,
    "deposit"
  );

  if (!areDepositsProcessed) {
    txRes = await invisibleContract
      .processDepositHashes(txBatchId, depositRequests, overrides)
      .catch((err) => {
        console.log("Error: ", err);
      });

    receipts["processDepositHashes"] = await txRes.wait();
    console.log("events: ", receipts["processDepositHashes"].logs);
  }

  // * PROCESS WITHDRAWALS ------------------------
  let areWithdrawalsProcessed = await areHashesProcessed(
    signer,
    messageRelayAddress,
    txBatchId,
    "withdrawal"
  );

  if (!areWithdrawalsProcessed) {
    txRes = await invisibleContract
      .processWithdrawals(txBatchId, withdrawalRequests, overrides)
      .catch((err) => {
        console.log("Error: ", err);
      });

    receipts["processWithdrawals"] = await txRes.wait();
    console.log("events: ", receipts["processWithdrawals"].logs);
  }

  return receipts;
}

async function areHashesProcessed(
  signer,
  messageRelayAddress,
  txBatchId,
  type
) {
  const relayAbi = require(path.join(
    __dirname,
    "../abis/L2MessageRelay.json"
  )).abi;

  const relayContract = new ethers.Contract(
    messageRelayAddress,
    relayAbi,
    signer ?? undefined
  );

  if (type === "deposit") {
    let isProcessed = await relayContract.processedDeposits(txBatchId);
    return isProcessed;
  } else if (type === "withdrawal") {
    let isProcessed = await relayContract.processedWithdrawals(txBatchId);
    return isProcessed;
  }
}

module.exports = {
  processL2Interactions,
};
