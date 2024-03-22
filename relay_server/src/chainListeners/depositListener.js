const {
  getProcessedDeposits,
  updateStoredDepositIds,
  getStoredCommitment,
  storePendingCommitment,
  updateStoredCommitment,
} = require("../helpers/localStorage");
const {
  storeOnchainDeposit,
  updatePendingWithdrawals,
} = require("../helpers/firebase/firebaseConnection");

const { getDepositCommitment } = require("./dataCommitment");

function listenForDeposits(db, client, invisibleContract, isL1) {
  invisibleContract.on(
    "DepositEvent",
    async (depositId, pubKey, tokenId, depositAmountScaled, timestamp) => {
      let storedCommitment = await getStoredCommitment(db, BigInt(depositId));
      if (storedCommitment) return;

      let deposit = {
        deposit_id: depositId.toString(),
        stark_key: pubKey.toString(),
        deposit_token: tokenId.toString(),
        deposit_amount: depositAmountScaled.toString(),
        timestamp: timestamp.toString(),
      };

      // ? Store the deposit in the datatbase
      storeOnchainDeposit(deposit);

      // ? Get the deposit commitment
      let depositCommitment = getDepositCommitment(deposit);

      // ? Register the deposit commitment
      await client.register_onchain_action(
        depositCommitment,
        function (err, _response) {
          if (err) {
            console.log(err);
          } else {
            console.log("Deposit commitment registered", depositCommitment);
            storePendingCommitment(db, depositCommitment);
          }
        }
      );
    }
  );

  // * Listen for processed withdrawals
  invisibleContract.on("ProcessedWithdrawals", async (timestamp, txBatchId) => {
    await updatePendingWithdrawals(isL1);
  });
}

async function isDepositValid(deposit, db) {
  let depositCommitment = getDepositCommitment(deposit);

  let storedCommitment = await getStoredCommitment(
    db,
    BigInt(deposit.deposit_id)
  );

  if (!storedCommitment) return false;

  if (storedCommitment.action_type != depositCommitment.action_type)
    return false;

  return storedCommitment.data_commitment == depositCommitment.data_commitment;
}

async function depositProcessedCallback(db, depositId) {
  return updateStoredCommitment(db, BigInt(depositId));
}

module.exports = {
  listenForDeposits,
  isDepositValid,
  depositProcessedCallback,
};
