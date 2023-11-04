const ethers = require("ethers");
const {
  getProcessedDeposits,
  updateStoredDepositIds,
} = require("./localStorage");
const { storeOnchainDeposit } = require("./firebase/firebaseConnection");

// const privateKey =
//   "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
// const signer = new ethers.Wallet(privateKey, provider);

const exchange_config = require("../../../exchange-config.json");

const provider = new ethers.providers.JsonRpcProvider("http://localhost:8545");

const invisibleL1Address = exchange_config["INVISIBL1_ETH_ADDRESS"];
const invisibleL1Abi = require("./abis/InvisibleL1.json").abi;

const invisibleL1Contract = new ethers.Contract(
  invisibleL1Address,
  invisibleL1Abi,
  provider
);

const tokenId2Address = exchange_config["TOKEN_ID_2_ADDRESS"];

async function listenForDeposits(db) {
  let { pendingDeposits, processedDepositIds } = (await getProcessedDeposits(
    db
  )) || { pendingDeposits: [], processedDepositIds: [] };

  invisibleL1Contract.on(
    "DepositEvent",
    (depositId, pubKey, tokenId, depositAmountScaled, timestamp) => {
      if (
        pendingDeposits.includes(depositId.toString()) ||
        processedDepositIds.includes(depositId.toString())
      )
        return;

      let deposit = {
        deposit_id: depositId.toString(),
        stark_key: pubKey.toString(),
        deposit_token: tokenId.toString(),
        deposit_amount: depositAmountScaled.toString(),
        timestamp: timestamp.toString(),
      };

      pendingDeposits.push(deposit);
      updateStoredDepositIds(db, pendingDeposits, processedDepositIds);

      // ? Store the deposit in the datatbase
      storeOnchainDeposit(deposit);
    }
  );
}

const onchainDecimalsPerAsset = exchange_config["ONCHAIN_DECIMALS_PER_ASSET"];

const DECIMALS_PER_ASSET = exchange_config["DECIMALS_PER_ASSET"];

async function isDepositValid(deposit, db) {
  let { pendingDeposits, processedDepositIds } = (await getProcessedDeposits(
    db
  )) || { pendingDeposits: [], processedDepositIds: [] };

  if (processedDepositIds.includes(deposit.deposit_id)) return false;

  let pendingDeposit = pendingDeposits.find(
    (dep) => dep.deposit_id == deposit.deposit_id
  );

  if (!pendingDeposit) return false;

  if (pendingDeposit.stark_key !== deposit.stark_key) return false;
  if (pendingDeposit.deposit_token !== deposit.deposit_token) return false;
  if (pendingDeposit.deposit_amount !== deposit.deposit_amount) return false;

  let depositAmount = await invisibleL1Contract.getPendingDepositAmount(
    deposit.stark_key,
    tokenId2Address[deposit.deposit_token]
  );

  let scaledDownOnChainAmount = ethers.utils.formatUnits(
    depositAmount,
    onchainDecimalsPerAsset[deposit.deposit_token]
  );
  let scaledDownDepositAmount =
    pendingDeposit.deposit_amount /
    10 ** DECIMALS_PER_ASSET[deposit.deposit_token];

  return scaledDownOnChainAmount >= scaledDownDepositAmount;
}

async function depositProcessedCallback(db, depositId) {
  let { pendingDeposits, processedDepositIds } = (await getProcessedDeposits(
    db
  )) || { pendingDeposits: [], processedDepositIds: [] };

  let pendingDeposit = pendingDeposits.find(
    (deposit) => deposit.deposit_id === depositId
  );
  if (!pendingDeposit) return false;

  pendingDeposits = pendingDeposits.filter((id) => id !== depositId);
  processedDepositIds.push(depositId);

  updateStoredDepositIds(db, pendingDeposits, processedDepositIds);
}

module.exports = {
  listenForDeposits,
  isDepositValid,
  depositProcessedCallback,
};
