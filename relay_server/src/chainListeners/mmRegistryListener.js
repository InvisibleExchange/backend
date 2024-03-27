const { storeMMAction } = require("../helpers/firebase/firebaseConnection");
const {
  getStoredCommitment,
  storePendingCommitment,
  updateStoredCommitment,
} = require("../helpers/localStorage");
const {
  getRegisterMMCommitment,
  getAddLiquidityCommitment,
  getRemoveLiquidityCommitment,
  getCloseMMCommitment,
} = require("./dataCommitment");

function listenForMMActions(db, client, invisibleL1Contract) {
  // * new PerpMM Registration * //
  invisibleL1Contract.on(
    "newPerpMMRegistration",
    async (
      mm_owner,
      synthetic_asset,
      position_address,
      vlp_token,
      mmActionId
    ) => {
      mmActionId = mmActionId.toString();

      console.log("newPerpMMRegistration", {
        mm_owner,
        synthetic_asset,
        position_address,
        vlp_token,
        mmActionId,
      });

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      console.log("storedCommitment", storedCommitment);
      if (storedCommitment) return;

      let commitment = getRegisterMMCommitment(
        mmActionId,
        synthetic_asset,
        position_address,
        vlp_token
      );

      // ? Register the MM Registration commitment
      await client.register_onchain_action(
        commitment,
        function (err, _response) {
          if (err) {
            console.log(err);
          } else {
            console.log("MM Registration commitment registered", commitment);

            storeMMAction({
              mm_owner,
              synthetic_asset,
              position_address,
              vlp_token,
              action_id: mmActionId,
              action_type: "register_mm",
            });

            storePendingCommitment(db, commitment);
          }
        }
      );
    }
  );

  // * Add Liquidity * //
  invisibleL1Contract.on(
    "AddLiquidity",
    async (depositor, position_address, usdc_amount, mmActionId) => {
      mmActionId = mmActionId.toString();

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;

      let commitment = getAddLiquidityCommitment(
        mmActionId,
        depositor,
        position_address,
        usdc_amount
      );

      // ? Register the MM Registration commitment
      await client.register_onchain_action(
        commitment,
        function (err, _response) {
          if (err) {
            console.log(err);
          } else {
            console.log("Add liquidity commitment registered", commitment);

            storeMMAction({
              depositor,
              position_address,
              usdc_amount,
              action_id: mmActionId,
              action_type: "add_liquidity",
            });

            storePendingCommitment(db, commitment);
          }
        }
      );
    }
  );

  // * Remove Liquidity * //
  invisibleL1Contract.on(
    "RemoveLiquidity",
    async (
      depositor,
      position_address,
      initial_value,
      vlp_amount,
      mmActionId
    ) => {
      mmActionId = mmActionId.toString();

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;

      let commitment = getRemoveLiquidityCommitment(
        mmActionId,
        depositor,
        position_address,
        initial_value,
        vlp_amount
      );

      // ? Register the MM Registration commitment
      await client.register_onchain_action(
        commitment,
        function (err, _response) {
          if (err) {
            console.log(err);
          } else {
            console.log("Remove liquidity commitment registered", commitment);

            storeMMAction({
              depositor,
              position_address,
              initial_value,
              vlp_amount,
              action_id: mmActionId,
              action_type: "remove_liquidity",
            });

            storePendingCommitment(db, commitment);
          }
        }
      );
    }
  );

  // * Close Position Event * //
  invisibleL1Contract.on(
    "ClosePositionEvent",
    async (
      position_address,
      mm_owner,
      initial_value_sum,
      vlp_amount_sum,
      mmActionId
    ) => {
      mmActionId = mmActionId.toString();

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;

      let commitment = getCloseMMCommitment(
        mmActionId,
        position_address,
        initial_value_sum,
        vlp_amount_sum
      );

      // ? Register the MM Registration commitment
      await client.register_onchain_action(
        commitment,
        function (err, _response) {
          if (err) {
            console.log(err);
          } else {
            console.log("Close position commitment registered", commitment);

            storeMMAction({
              position_address,
              mm_owner,
              initial_value_sum,
              vlp_amount_sum,
              action_id: mmActionId,
              action_type: "close_mm",
            });

            storePendingCommitment(db, commitment);
          }
        }
      );
    }
  );
}

// * =============================================================================

async function isMMRegistrationValid(db, registerMmReq) {
  let commitment = getRegisterMMCommitment(
    registerMmReq.mm_action_id,
    registerMmReq.synthetic_token,
    registerMmReq.position.position_header.position_address,
    registerMmReq.vlp_token
  );

  return isMMActionCommitmentValid(db, registerMmReq.mm_action_id, commitment);
}

async function isMMAddLiquidityValid(db, addLiqReq) {
  let commitment = getAddLiquidityCommitment(
    addLiqReq.mm_action_id,
    addLiqReq.depositor,
    addLiqReq.position.position_header.position_address,
    addLiqReq.initial_value
  );

  return isMMActionCommitmentValid(db, addLiqReq.mm_action_id, commitment);
}

async function isMMRemoveLiquidityValid(db, removeLiqReq) {
  let commitment = getRemoveLiquidityCommitment(
    removeLiqReq.mm_action_id,
    removeLiqReq.depositor,
    removeLiqReq.position.position_header.position_address,
    removeLiqReq.initial_value,
    removeLiqReq.vlp_amount
  );

  return isMMActionCommitmentValid(db, removeLiqReq.mm_action_id, commitment);
}

async function isCloseMMValid(db, closeMMReq) {
  let commitment = getCloseMMCommitment(
    closeMMReq.mm_action_id,
    closeMMReq.position.position_header.position_address,
    closeMMReq.initial_value_sum,
    closeMMReq.vlp_amount_sum
  );

  return isMMActionCommitmentValid(db, closeMMReq.mm_action_id, commitment);
}

async function isMMActionCommitmentValid(db, mmActionId, commitment) {
  let storedCommitment = await getStoredCommitment(db, BigInt(mmActionId));
  if (!storedCommitment) return false;

  if (storedCommitment.action_type != commitment.action_type) return false;

  return storedCommitment.data_commitment == commitment.data_commitment;
}

async function mmActionProcessedCallback(db, mmActionId) {
  return updateStoredCommitment(db, BigInt(mmActionId));
}

module.exports = {
  listenForMMActions,
  isMMRegistrationValid,
  isMMAddLiquidityValid,
  isMMRemoveLiquidityValid,
  isCloseMMValid,
  mmActionProcessedCallback,
};
