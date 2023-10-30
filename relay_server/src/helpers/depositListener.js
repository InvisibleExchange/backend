const ethers = require("ethers");
const {
  getProcessedDeposits,
  updateStoredDepositIds,
} = require("./localStorage");
const { storeOnchainDeposit } = require("./firebase/firebaseConnection");

// const privateKey =
//   "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
// const signer = new ethers.Wallet(privateKey, provider);

const provider = new ethers.providers.JsonRpcProvider("http://localhost:8545");

const invisibleL1Address = "0xFa62E2E9B7A3F1Aa1773e165c42fEabc52d748bB"; //Todo
const invisibleL1Abi = require("./abis/InvisibleL1.json").abi;

const invisibleL1Contract = new ethers.Contract(
  invisibleL1Address,
  invisibleL1Abi,
  provider
);

async function listenForDeposits(db) {
  let pendingDepositIds = [];
  let processedDepositIds = [];

  getProcessedDeposits(db, (err, pending_, processed_) => {
    pendingDepositIds = pending_ ?? [];
    processedDepositIds = processed_ ?? [];
  });

  invisibleL1Contract.on(
    "DepositEvent",
    (depositId, pubKey, tokenId, depositAmountScaled, timestamp) => {
      if (
        pendingDepositIds.includes(depositId.toString()) ||
        processedDepositIds.includes(depositId.toString())
      )
        return;

      let deposit = {
        depositId: depositId.toString(),
        starkKey: pubKey.toString(),
        tokenId: tokenId.toString(),
        amount: depositAmountScaled.toString(),
        timestamp: timestamp.toString(),
      };

      pendingDepositIds.push(depositId.toString());
      updateStoredDepositIds(db, pendingDepositIds, processedDepositIds);

      // ? Store the deposit in the datatbase
      storeOnchainDeposit(deposit);
    }
  );
}


function depositProcessedCallback(db, depositId) {

  

}


module.exports = { listenForDeposits };
