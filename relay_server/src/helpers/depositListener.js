const ethers = require("ethers");
const {
  getProcessedDeposits,
  updateStoredDepositIds,
} = require("./localStorage");
const { storeOnchainDeposit } = require("./firebase/firebaseConnection");
const { id } = require("ethers/lib/utils");

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

const tokenId2Address = {
  55555: "0x1754C78DD11F6B07DFC9e529BD19d912EAEfA1c8",
  12345: "0xBF52caf40b7612bEd0814A09842c14BAB217BaD5",
  54321: "0x0000000000000000000000000000000000000000",
};

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

const onchainDecimalsPerAsset = {
  55555: 18,
  12345: 18,
  54321: 18,
};

const DECIMALS_PER_ASSET = {
  55555: 6,
  12345: 8,
  54321: 8,
};

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
