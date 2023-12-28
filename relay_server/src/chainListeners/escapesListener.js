const {
  getStoredCommitment,
  storePendingCommitment,
  updateStoredCommitment,
} = require("../helpers/localStorage");

async function listenForEscapes(db, client, invisibleL1Contract) {
  // * Note Escapes * //
  invisibleL1Contract.on(
    "NoteEscapeEvent",
    async (escapeId, timestamp, escape_notes, signature) => {
      console.log("escapeId", escapeId.toString());

      let storedCommitment = await getStoredCommitment(db, escapeId);
      if (storedCommitment) return;

      // TODO: This has to be in the correct format
      let escapeMessage = {
        escape_id: escapeId.toString(),
        escape_notes: escape_notes,
        signature: signature,
      };

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          // Todo: If successful, update the stored commitment

          let commitment = {
            data_id: escapeId,
            action_type: GrpcOnchainActionType["NOTE_ESCAPE"],
            data_commitment: escapeId,
          };

          storePendingCommitment(db, commitment);
        }
      });
    }
  );

  // * Tab Escapes * //
  invisibleL1Contract.on(
    "OrderTabEscapeEvent",
    async (escapeId, timestamp, orderTab, signature) => {
      console.log("escapeId", escapeId.toString());

      let storedCommitment = await getStoredCommitment(db, escapeId);
      if (storedCommitment) return;

      // TODO: This has to be in the correct format
      let escapeMessage = {
        escape_id: escapeId.toString(),
        close_order_tab_req: orderTab,
        signature: signature,
      };

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          // Todo: If successful, update the stored commitment

          let commitment = {
            data_id: escapeId,
            action_type: GrpcOnchainActionType["TAB_ESCAPE"],
            data_commitment: escapeId,
          };

          storePendingCommitment(db, commitment);
        }
      });
    }
  );

  // * Position Escapes * //
  invisibleL1Contract.on(
    "PositionEscapeEvent",
    async (
      escapeId,
      closePrice,
      position_a,
      B,
      hash_b,
      recipient,
      signature_a,
      signature_b
    ) => {
      console.log("escapeId", escapeId.toString());

      let storedCommitment = await getStoredCommitment(db, escapeId);
      if (storedCommitment) return;

      // TODO: This has to be in the correct format
      let escapeMessage = {
        escape_id: escapeId.toString(),
        close_position_message: {
          close_price: closePrice.toString(),
          position_a: position_a,
          open_order_fields_b: B,
          position_b: B,
          recipient: recipient,
          signature_a: signature_a,
          signature_b: signature_b,
        },
      };

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          // Todo: If successful, update the stored commitment

          let commitment = {
            data_id: escapeId,
            action_type: GrpcOnchainActionType["POSITION_ESCAPE"],
            data_commitment: escapeId,
          };

          storePendingCommitment(db, commitment);
        }
      });
    }
  );
}

module.exports = {
  listenForEscapes,
};
