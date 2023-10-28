const ethers = require("ethers");
const {
  getProcessedDeposits,
  updateStoredDepositIds,
} = require("./localStorage");
const { storeOnchainDeposit } = require("./firebase/firebaseConnection");

const privateKey =
  "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const provider = new ethers.providers.JsonRpcProvider("http://localhost:8545");
const signer = new ethers.Wallet(privateKey, provider);

const invisibleL1Address = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"; //Todo
const invisibleL1Abi = require("./abis/InvisibleL1.json").abi;

const invisibleL1Contract = new ethers.Contract(
  invisibleL1Address,
  invisibleL1Abi,
  signer
);

const TestTokenAbi = require("./abis/TestToken.json").abi;

const WbtcAddress = "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512"; //Todo
const WbtcContract = new ethers.Contract(WbtcAddress, TestTokenAbi, signer);

const UsdcAddress = "0x5FbDB2315678afecb367f032d93F642f64180aa3"; //Todo
const UsdcContract = new ethers.Contract(UsdcAddress, TestTokenAbi, signer);

const tokenContracts = {
  12345: WbtcContract,
  55555: UsdcContract,
};
const onChainErc20Decimals = {
  12345: 18,
  54321: 18,
  55555: 18,
};

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

      pendingDepositIds.push(deposit);
      updateStoredDepositIds(db, pendingDepositIds, processedDepositIds);

      // ? Store the deposit in the datatbase
      storeOnchainDeposit(deposit);
    }
  );
}

module.exports = { listenForDeposits };
