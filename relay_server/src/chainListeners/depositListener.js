const ethers = require("ethers");
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

const exchange_config = require("../../../exchange-config.json");
const { computeHashOnElements } = require("../helpers/crypto_hash");
const { getDepositCommitment } = require("./dataCommitment");

let GrpcOnchainActionType = {
  DEPOSIT: 0,
  MM_REGISTRATION: 1,
  MM_ADD_LIQUIDITY: 2,
  MM_REMOVE_LIQUIDITY: 3,
  MM_CLOSE_POSITION: 4,
  NOTE_ESCAPE: 5,
  TAB_ESCAPE: 6,
  POSITION_ESCAPE: 7,
};

async function listenForDeposits(db, client, invisibleL1Contract) {
  invisibleL1Contract.on(
    "DepositEvent",
    async (depositId, pubKey, tokenId, depositAmountScaled, timestamp) => {
      console.log("depositId", depositId.toString());

      let storedCommitment = await getStoredCommitment(db, depositId % 2 ** 32);
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
            storePendingCommitment(db, depositCommitment);
          }
        }
      );
    }
  );
}

async function isDepositValid(deposit, db) {
  let depositCommitment = getDepositCommitment(deposit);

  let storedCommitment = await getStoredCommitment(
    db,
    deposit.deposit_id % 2 ** 32
  );
  if (!storedCommitment) return false;

  if (storedCommitment.action_type !== depositCommitment.action_type)
    return false;

  return storedCommitment.data_commitment !== depositCommitment.data_commitment;
}

async function depositProcessedCallback(db, depositId) {
  return updateStoredCommitment(db, depositId % 2 ** 32);
}

module.exports = {
  listenForDeposits,
  isDepositValid,
  depositProcessedCallback,
};
