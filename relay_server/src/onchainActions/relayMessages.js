const { ethers } = require("ethers");

const path = require("path");
const dotenv = require("dotenv");
dotenv.config({ path: path.join(__dirname, "../.env") });

const { Options } = require("@layerzerolabs/lz-v2-utilities");

async function relayAccumulatedHashes(relayAddress, txBatchId, destinationIds) {
  const network = "sepolia";

  let privateKey = process.env.ETH_PRIVATE_KEY ?? "";
  let rpcUrl = process.env.SEPOLIA_RPC_URL ?? "";
  const provider = new ethers.JsonRpcProvider(rpcUrl, network);
  const signer = new ethers.Wallet(privateKey, provider);

  const relayAbi = require(path.join(
    __dirname,
    "../abis/L1MessageRelay.json"
  )).abi;
  const relayContract = new ethers.Contract(
    relayAddress,
    relayAbi,
    signer ?? undefined
  );

  // let options = "0x00030100110100000000000000000000000000030d40";
  const executorGas = 500000;
  const executorValue = 0;
  const options = Options.newOptions()
    .addExecutorLzReceiveOption(executorGas, executorValue)
    .toHex();

  // ? ==============================================
  let receipts = [];

  for (let i = 0; i < destinationIds.length; i++) {
    const destId = destinationIds[i];

    let result = await relayContract.estimateMessageFee(
      destId,
      txBatchId,
      options
    );
    let messageFee = result[0][0];

    let gasFeeData = await signer.provider.getFeeData();
    let overrides = {
      gasLimit: 500_000,
      // gasPrice: gasFeeData.gasPrice,
      maxFeePerGas: gasFeeData.maxFeePerGas,
      maxPriorityFeePerGas: gasFeeData.maxPriorityFeePerGas,
      value: messageFee,
    };

    console.log("overrides: ", overrides);

    let txRes = await relayContract
      .sendAccumulatedHashes(txBatchId, destId, options, overrides)
      .catch((err) => {
        console.log("Error: ", err);
      });

    console.log("relay tx hash: ", txRes.hash);
    let receipt = await txRes.wait();
    console.log(
      "events: ",
      receipt.logs.map((log) => log.args)
    );

    receipts.push(receipt);
  }

  return receipts;
}

// * -----------------------------------

async function relayL2Acknowledgment(provider, relayAddress, txBatchId) {
  let privateKey = process.env.ETH_PRIVATE_KEY ?? "";
  const signer = new ethers.Wallet(privateKey, provider);

  const relayAbi = require("../abis/L2MessageRelay.json").abi;
  const relayContract = new ethers.Contract(
    relayAddress,
    relayAbi,
    signer ?? undefined
  );

  const executorGas = 500000;
  const executorValue = 0;
  const _options = Options.newOptions().addExecutorLzReceiveOption(
    executorGas,
    executorValue
  );
  const options = _options.toHex();

  let result = await relayContract.estimateAcknowledgmentFee(
    txBatchId,
    options
  );
  let messageFee = result[0];

  let gasFeeData = await signer.provider.getFeeData();
  let overrides = {
    // gasLimit: 1_000_000,
    maxFeePerGas: gasFeeData.maxFeePerGas,
    maxPriorityFeePerGas: gasFeeData.maxPriorityFeePerGas,
    value: messageFee,
  };

  let txRes = await relayContract
    .sendAcknowledgment(txBatchId, options, overrides)
    .catch((err) => {
      console.log("Error: ", err);
    });
  let receipt = await txRes.wait();

  console.log(
    "events: ",
    receipt.logs.map((log) => log.args)
  );

  return receipt;
}

// let chainConfig = require(path.join(__dirname, "../../address-config.json"));

// const relayAddress = chainConfig["L1"]["MessageRelay"];
// const txBatchId = 3;
// const destinationIds = [40231];
// relayAccumulatedHashes(relayAddress, txBatchId, destinationIds);

// let rpcUrl = process.env.ARB_SEPOLIA_RPC_URL ?? "";
// const provider = new ethers.JsonRpcProvider(rpcUrl);
// const relayAddress = chainConfig["Arbitrum"]["MessageRelay"];
// const txBatchId = 3;
// relayL2Acknowledgment(provider, relayAddress, txBatchId);

module.exports = {
  relayAccumulatedHashes,
  relayL2Acknowledgment,
};
