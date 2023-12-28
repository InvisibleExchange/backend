const {
  getStoredCommitment,
  storePendingCommitment,
  updateStoredCommitment,
} = require("../helpers/localStorage");

async function listenForEscapes(db, client, invisibleL1Contract) {
  // * new PerpMM Registration * //
  invisibleL1Contract.on(
    "newPerpMMRegistration",
    async (
      mmOwner,
      syntheticAsset,
      positionAddress,
      maxVlpSupply,
      vlpTokenId,
      mmActionId
    ) => {
      console.log("escapeId", mmActionId.toString());

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;
    }
  );

  // * Add Liquidity * //
  invisibleL1Contract.on(
    "AddLiquidity",
    async (depositor, mmPositionAddress, usdcAmount, mmActionId) => {
      console.log("escapeId", mmActionId.toString());

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;
    }
  );

  // * Remove Liquidity * //
  invisibleL1Contract.on(
    "RemoveLiquidity",
    async (
      depositor,
      mmPositionAddress,
      initialValue,
      vlpAmount,
      mmActionId
    ) => {
      console.log("escapeId", mmActionId.toString());

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;
    }
  );

  // * Close Position Event * //
  invisibleL1Contract.on(
    "ClosePositionEvent",
    async (
      positionAddress,
      mmOwner,
      initialValueSum,
      vlpAmountSum,
      mmActionId
    ) => {
      console.log("escapeId", mmActionId.toString());

      let storedCommitment = await getStoredCommitment(db, mmActionId);
      if (storedCommitment) return;
    }
  );
}

module.exports = {
  listenForEscapes,
};
