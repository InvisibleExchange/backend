const { ethers } = require("hardhat");

const path = require("path");
const dotenv = require("dotenv");
dotenv.config({ path: path.join(__dirname, "../.env") });

async function processL2Interactions(
  provider, // For the correct L2 network
  invisibleL2Address,
  txBatchId,
  depositRequests,
  withdrawalRequests
) {
  const signer = new ethers.Wallet(privateKey, provider);

  const invisibleL2Abi = require("../abis/InvisibleL2.json").abi;
  const invisibleContract = new ethers.Contract(
    invisibleL2Address,
    invisibleL2Abi,
    signer ?? undefined
  );

  let gasFeeData = await signer.provider.getFeeData();
  let overrides = {
    // gasLimit: 3_000_000,
    maxFeePerGas: gasFeeData.maxFeePerGas,
    maxPriorityFeePerGas: gasFeeData.maxPriorityFeePerGas,
  };

  let receipts = {};

  // * PROCESS DEPOSITS ------------------------
  let txRes = await invisibleContract
    .processDepositHashes(txBatchId, depositRequests, overrides)
    .catch((err) => {
      console.log("Error: ", err);
    });
  console.log("tx hash: ", txRes.hash);
  receipts["processDepositHashes"] = await txRes.wait();

  console.log("events: ", receipts["processDepositHashes"].logs);

  // * PROCESS WITHDRAWALS ------------------------
  txRes = await invisibleContract
    .processWithdrawals(txBatchId, withdrawalRequests, overrides)
    .catch((err) => {
      console.log("Error: ", err);
    });
  console.log("\n\n\ntx hash: ", txRes.hash);
  receipts["processWithdrawals"] = await txRes.wait();

  console.log("events: ", receipts["processWithdrawals"].logs);

  return receipts;
}

module.exports = {
  processL2Interactions,
};
