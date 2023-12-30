const {
  getProcessedDeposits,
  updateStoredDepositIds,
  getStoredCommitment,
  storePendingCommitment,
  updateStoredCommitment,
} = require("../helpers/localStorage");
const {
  storeOnchainDeposit,
} = require("../helpers/firebase/firebaseConnection");

const { getDepositCommitment } = require("./dataCommitment");

async function listenForDeposits(db, client, invisibleL1Contract) {
  invisibleL1Contract.on(
    "DepositEvent",
    async (depositId, pubKey, tokenId, depositAmountScaled, timestamp) => {
      let storedCommitment = await getStoredCommitment(
        db,
        BigInt(depositId) % 2n ** 32n
      );
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

  // TODO: LISTEN FOR DEPOSIT CANCELLATIONS
}

async function isDepositValid(deposit, db) {
  let depositCommitment = getDepositCommitment(deposit);

  let storedCommitment = await getStoredCommitment(
    db,
    BigInt(deposit.deposit_id) % 2n ** 32n
  );

  if (!storedCommitment) return false;

  if (storedCommitment.action_type != depositCommitment.action_type)
    return false;

  return storedCommitment.data_commitment == depositCommitment.data_commitment;
}

async function depositProcessedCallback(db, depositId) {
  return updateStoredCommitment(db, BigInt(depositId) % 2n ** 32n);
}

module.exports = {
  listenForDeposits,
  isDepositValid,
  depositProcessedCallback,
};
