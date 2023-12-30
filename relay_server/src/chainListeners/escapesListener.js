const {
  getStoredCommitment,
  storePendingCommitment,
  updateStoredCommitment,
} = require("../helpers/localStorage");

async function listenForEscapes(db, client, escapeVerifierContract) {
  // * Note Escapes * //
  escapeVerifierContract.on(
    "NoteEscapeEvent",
    async (escapeId, timestamp, escape_notes, signature) => {
      escapeId = Number(escapeId);

      let storedCommitment = await getStoredCommitment(db, 2n ** 40n + BigInt(escapeId));
      if (storedCommitment) return;

      escape_notes = escape_notes.map((note) => {
        return {
          address: { x: note.addressX.toString(), y: note.addressY.toString() },
          token: note.token.toString(),
          amount: note.amount.toString(),
          blinding: note.blinding.toString(),
          index: note.index.toString(),
        };
      });

      let escapeMessage = {
        escape_id: escapeId.toString(),
        escape_notes: escape_notes,
        signature: {
          r: signature[0].toString(),
          s: signature[1].toString(),
        },
      };

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          let commitment = {
            data_id: 2n ** 40n + BigInt(escapeId),
            action_type: GrpcOnchainActionType["NOTE_ESCAPE"],
            data_commitment: escapeId,
          };

          storePendingCommitment(db, commitment);
        }
      });
    }
  );

  // * Tab Escapes * //
  escapeVerifierContract.on(
    "OrderTabEscapeEvent",
    async (escapeId, timestamp, orderTab, signature) => {
      escapeId = Number(escapeId);

      let storedCommitment = await getStoredCommitment(db, 2n ** 40n + BigInt(escapeId));
      if (storedCommitment) return;

      orderTab = {
        tab_idx: orderTab.tab_idx.toString(),
        tab_header: {
          base_token: orderTab.base_token.toString(),
          quote_token: orderTab.quote_token.toString(),
          base_blinding: orderTab.base_blinding.toString(),
          quote_blinding: orderTab.quote_blinding.toString(),
          pub_key: orderTab.pub_key.toString(),
        },
        base_amount: orderTab.base_amount.toString(),
        quote_amount: orderTab.quote_amount.toString(),
      };

      // TODO: This has to be in the correct format
      let escapeMessage = {
        escape_id: escapeId.toString(),
        close_order_tab_req: orderTab,
        signature: {
          r: signature[0].toString(),
          s: signature[1].toString(),
        },
      };

      console.log("escapeMessage", escapeMessage);

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          let commitment = {
            data_id: 2n ** 40n + BigInt(escapeId),
            action_type: GrpcOnchainActionType["TAB_ESCAPE"],
            data_commitment: escapeId,
          };

          storePendingCommitment(db, commitment);
        }
      });
    }
  );

  // * Position Escapes * //
  escapeVerifierContract.on(
    "PositionEscapeEventA",
    async (
      escapeId,
      closePrice,
      position_a,
      open_order_fields_b,
      recipient,
      signature_a,
      signature_b
    ) => {
      escapeId = Number(escapeId);

      let storedCommitment = await getStoredCommitment(db, 2n ** 40n + BigInt(escapeId));
      if (storedCommitment) return;

      position_a = {
        index: position_a.index.toString(),
        position_header: {
          synthetic_token: position_a.synthetic_token.toString(),
          position_address: position_a.position_address.toString(),
          allow_partial_liquidations: position_a.allow_partial_liquidations,
          vlp_token: position_a.vlp_token.toString(),
          max_vlp_supply: position_a.max_vlp_supply.toString(),
        },
        order_side: !!position_a.order_side,
        position_size: position_a.position_size.toString(),
        margin: position_a.margin.toString(),
        entry_price: position_a.entry_price.toString(),
        liquidation_price: position_a.liquidation_price.toString(),
        bankruptcy_price: position_a.bankruptcy_price.toString(),
        last_funding_idx: position_a.last_funding_idx.toString(),
        vlp_supply: position_a.vlp_supply.toString(),
      };

      open_order_fields_b = {
        initial_margin: open_order_fields_b.initial_margin.toString(),
        collateral_token: open_order_fields_b.collateral_token.toString(),
        notes_in: open_order_fields_b.notes_in.map((note) => {
          return {
            address: {
              x: note.addressX.toString(),
              y: note.addressY.toString(),
            },
            token: note.token.toString(),
            amount: note.amount.toString(),
            blinding: note.blinding.toString(),
            index: note.index.toString(),
          };
        }),
        refund_note: open_order_fields_b.refund_note
          ? {
              address: {
                x: open_order_fields_b.refund_note.addressX.toString(),
                y: open_order_fields_b.refund_note.addressY.toString(),
              },
              token: open_order_fields_b.refund_note.token.toString(),
              amount: open_order_fields_b.refund_note.amount.toString(),
              blinding: open_order_fields_b.refund_note.blinding.toString(),
              index: open_order_fields_b.refund_note.index.toString(),
            }
          : null,
        position_address: open_order_fields_b.position_address,
        allow_partial_liquidations:
          open_order_fields_b.allow_partial_liquidations,
      };

      // TODO: This has to be in the correct format
      let escapeMessage = {
        escape_id: escapeId.toString(),
        close_position_message: {
          close_price: closePrice.toString(),
          position_a: position_a,
          open_order_fields_b: open_order_fields_b,
          position_b: null,
          recipient: recipient,
          signature_a: {
            r: signature_a[0].toString(),
            s: signature_a[1].toString(),
          },
          signature_b: {
            r: signature_b[0].toString(),
            s: signature_b[1].toString(),
          },
        },
      };

      console.log("escapeMessage", escapeMessage);

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          let commitment = {
            data_id: 2n ** 40n + BigInt(escapeId),
            action_type: GrpcOnchainActionType["POSITION_ESCAPE"],
            data_commitment: escapeId,
          };

          storePendingCommitment(db, commitment);
        }
      });
    }
  );

  // * Position Escapes * //
  escapeVerifierContract.on(
    "PositionEscapeEventB",
    async (
      escapeId,
      closePrice,
      position_a,
      position_b,
      recipient,
      signature_a,
      signature_b
    ) => {
      escapeId = Number(escapeId);

      let storedCommitment = await getStoredCommitment(db, 2n ** 40n + BigInt(escapeId));
      if (storedCommitment) return;

      position_a = {
        index: position_a.index.toString(),
        position_header: {
          synthetic_token: position_a.synthetic_token.toString(),
          position_address: position_a.position_address.toString(),
          allow_partial_liquidations: position_a.allow_partial_liquidations,
          vlp_token: position_a.vlp_token.toString(),
          max_vlp_supply: position_a.max_vlp_supply.toString(),
        },
        order_side: !!position_a.order_side,
        position_size: position_a.position_size.toString(),
        margin: position_a.margin.toString(),
        entry_price: position_a.entry_price.toString(),
        liquidation_price: position_a.liquidation_price.toString(),
        bankruptcy_price: position_a.bankruptcy_price.toString(),
        last_funding_idx: position_a.last_funding_idx.toString(),
        vlp_supply: position_a.vlp_supply.toString(),
      };

      position_b = {
        index: position_b.index.toString(),
        position_header: {
          synthetic_token: position_b.synthetic_token.toString(),
          position_address: position_b.position_address.toString(),
          allow_partial_liquidations: position_b.allow_partial_liquidations,
          vlp_token: position_b.vlp_token.toString(),
          max_vlp_supply: position_b.max_vlp_supply.toString(),
        },
        order_side: !!position_b.order_side,
        position_size: position_b.position_size.toString(),
        margin: position_b.margin.toString(),
        entry_price: position_b.entry_price.toString(),
        liquidation_price: position_b.liquidation_price.toString(),
        bankruptcy_price: position_b.bankruptcy_price.toString(),
        last_funding_idx: position_b.last_funding_idx.toString(),
        vlp_supply: position_b.vlp_supply.toString(),
      };

      let escapeMessage = {
        escape_id: escapeId.toString(),
        close_position_message: {
          close_price: closePrice.toString(),
          position_a: position_a,
          open_order_fields_b: null,
          position_b: position_b,
          recipient: recipient,
          signature_a: {
            r: signature_a[0].toString(),
            s: signature_a[1].toString(),
          },
          signature_b: {
            r: signature_b[0].toString(),
            s: signature_b[1].toString(),
          },
        },
      };

      console.log("escapeMessage", escapeMessage);

      await client.execute_escape(escapeMessage, function (err, _response) {
        if (err) {
          console.log(err);
        } else {
          let commitment = {
            data_id: 2n ** 40n + BigInt(escapeId),
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
