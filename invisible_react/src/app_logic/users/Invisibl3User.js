const bigInt = require("big-integer");
const { pedersen, computeHashOnElements } = require("../helpers/pedersen");
const { ec, getKeyPair } = require("starknet/utils/ellipticCurve");

const {
  storeUserData,
  fetchUserData,
  storePrivKey,
  removePrivKey,
  removeOrderId,
  fetchStoredNotes,
} = require("../helpers/firebase/firebaseConnection");

const {
  _subaddressPrivKeys,
  _oneTimeAddressPrivKey,
  _hideValuesForRecipient,
  _revealHiddenValues,
  _generateNewBliding,
  fetchNoteData,
  fetchPositionData,
  getActiveOrders,
  signMarginChange,
  _restoreKeyData,
  handlePfrNoteData,
} = require("./Invisibl3UserUtils.js");

const { Note, trimHash } = require("./Notes.js");
// const {
//   newLimitOrder,
//   signLimitOrder,
//   signLimitOrderFfi,
//   LimitOrderToFfiPointer,
// } = require("../helpers/FFI");
const LimitOrder = require("../transactions/LimitOrder");
const Deposit = require("../transactions/Deposit");
const {
  OpenOrderFields,
  CloseOrderFields,
  PerpOrder,
} = require("../transactions/PerpOrder");
const Withdrawal = require("../transactions/Withdrawal");

/* global BigInt */

const USER_ID_MASK =
  172815432917432758348972343289652348293569370432238525823094893243n;
const PRIVATE_SEED_MASK =
  3289567280438953725403208532754302390573452930958285878326574839523n;
const VIEW_KEY_MASK =
  7689472303258934252343208597532492385943798632767034892572348289573n;
const SPEND_KEY_MASK =
  8232958253823489479856437527982347891347326348905738437643519378455n;
// const COMMITMENT_MASK = 112233445566778899n;
// const AMOUNT_MASK = 998877665544332112n;

// TODOS !!!!!!!!!!!!!!!!!!!!!!!!!!!
// TODO: Make a function that calculates the aproximate amount of margin left for position based on entry and OB price
// TODO: A function that calculates the max leverage for a token and amount

module.exports = class User {
  // Each user has a class where he stores all his information (should never be shared with anyone)
  // private keys should be 240 bits
  constructor(_privViewKey, _privSpendKey) {
    if (
      _privViewKey.toString(2).length > 240 ||
      _privSpendKey.toString(2).length > 240
    ) {
      throw new Error("private keys should be 240 bits");
    }

    this.userId = computeHashOnElements([
      USER_ID_MASK,
      _privViewKey,
      _privSpendKey,
    ]);
    this.privViewKey = _privViewKey; //kv
    this.privSpendKey = _privSpendKey; //ks

    // ? privateSeed only uses the privViewKey because it allows someone to disclose their history,
    // ? without allowing them to spend their funds
    this.privateSeed = pedersen([PRIVATE_SEED_MASK, _privViewKey]);

    // ? Number of notes generated by the user for each token
    this.noteCounts = {};
    // ? Number of positions opened by the user for each token
    this.positionCounts = {};

    this.pubViewKey = getKeyPair(_privViewKey);
    this.pubSpendKey = getKeyPair(_privSpendKey);

    this.orderIds = [];
    this.perpetualOrderIds = [];

    this.orders = []; // {base_asset,expiration_timestamp,fee_limit,notes_in,order_id,order_side,price,qty_left,quote_asset,refund_note}
    this.perpetualOrders = []; // {order_id,expiration_timestamp,qty_left,price,synthetic_token,order_side,position_effect_type,fee_limit,position_address,notes_in,refund_note,initial_margin}

    // this.noteData structure is as follows:  {token1: [note1,..., noteN],...,tokenN: ...]}
    this.noteData = {};
    this.notePrivKeys = {}; // Maps {noteAddress: privKey}
    // this.positionData structure is as follows:  {token1: [positionJson1,..., positionJsonN],...,tokenN: ...]}
    this.positionData = {};
    this.positionPrivKeys = {}; // Maps {posAddress: privKey}
    //
    this.pfrKeys = {}; // Maps {orderId: pfrPrivKey}
    this.fills = []; // [{symbol, amount, price, side, type, time, isPerp}]
  }

  //* FETCH USER DATA  =========================================================

  getAvailableAmount(token) {
    let sum = 0;
    if (!this.noteData[token]) {
      return 0;
    }
    for (let x of this.noteData[token]) {
      sum += x.amount;
    }

    return sum;
  }

  async login() {
    let userData = await fetchUserData(this.userId).catch(console.log);

    // ? Get Note Data ============================================

    let keyPairs =
      userData.privKeys.length > 0
        ? userData.privKeys.map((pk) => getKeyPair(pk))
        : [];

    let { emptyPrivKeys, noteData, notePrivKeys } = await fetchNoteData(
      keyPairs,
      this.privateSeed
    );

    // removePrivKeys(this.userId, emptyPrivKeys);

    // ? Get Position Data ============================================
    let addressData =
      userData.positionPrivKeys.length > 0
        ? userData.positionPrivKeys.map((pk) => {
            return { pk: pk, address: getKeyPair(pk).getPublic() };
          })
        : [];

    let { emptyPositionPrivKeys, positionData, posPrivKeys } =
      await fetchPositionData(addressData);

    // removePositionPrivKeys(this.userId, emptyPositionPrivKeys);

    // ? Get Order Data ============================================

    // ? Get Order Data ============================================

    this.noteData = noteData;
    this.notePrivKeys = notePrivKeys;
    this.noteCounts = userData.noteCounts;
    this.positionCounts = userData.positionCounts;
    this.positionData = positionData;
    this.positionPrivKeys = posPrivKeys;
    this.orderIds = userData.orderIds;
    this.perpetualOrderIds = userData.perpetualOrderIds;
    this.pfrKeys = userData.pfrKeys;
  }

  async handleActiveOrders(badOrderIds, orders, badPerpOrderIds, perpOrders) {
    let activeOrderNoteIndexes = [];

    let pfrKeys = [];

    for (let order of orders) {
      if (this.pfrKeys[order.order_id]) {
        pfrKeys.push(this.pfrKeys[order.order_id]);
      }

      for (let note of order.notes_in) {
        activeOrderNoteIndexes.push(note.index);
      }
    }

    for (let order of perpOrders) {
      if (this.pfrKeys[order.order_id]) {
        pfrKeys.push(this.pfrKeys[order.order_id]);
      }
      if (order.position_effect_type == 0) {
        for (let note of order.notes_in) {
          activeOrderNoteIndexes.push(note.index);
        }
      }
    }

    let newNoteData = {};
    for (const [token, arr] of Object.entries(this.noteData)) {
      newNoteData[token] = arr.filter(
        (note) => !activeOrderNoteIndexes.includes(note.index.toString())
      );

      for (const pfrKey of pfrKeys) {
        let addr = getKeyPair(pfrKey).getPublic().getX();

        let idx = newNoteData[token].findIndex(
          (n) => n.address.getX().toString() === addr.toString()
        );
        if (idx !== -1) {
          newNoteData[token].splice(idx, 1);
        }
      }
    }

    // If bad order Id and pfrAddress exists, add the note to the user's noteData

    this.noteData = newNoteData;

    this.orders = orders;
    this.perpetualOrders = perpOrders;

    for (let orderId of badOrderIds) {
      removeOrderId(this.userId, orderId, false);

      if (this.pfrKeys[orderId]) {
        handlePfrNoteData(
          this.userId,
          this.pfrKeys[orderId],
          this.privateSeed,
          this.noteData,
          this.notePrivKeys
        );
      }
    }
    for (let orderId of badPerpOrderIds) {
      removeOrderId(this.userId, orderId, true);

      if (this.pfrKeys[orderId]) {
        handlePfrNoteData(
          this.userId,
          this.pfrKeys[orderId],
          this.privateSeed,
          this.noteData,
          this.notePrivKeys
        );
      }
    }
  }

  //* GENERATE ORDERS  ==========================================================

  makePerpetualOrder(
    expiration_timestamp,
    position_address,
    position_effect_type,
    order_side,
    synthetic_token,
    collateral_token,
    synthetic_amount,
    collateral_amount,
    price,
    fee_limit,
    initial_margin
  ) {
    if (!["Open", "Close", "Modify"].includes(position_effect_type)) {
      alert(
        "Invalid position effect type (liquidation orders created seperately)"
      );
      throw "Invalid position effect type (liquidation orders created seperately)";
    }
    if (!["Long", "Short"].includes(order_side)) {
      alert("Invalid order side");
      throw "Invalid order side";
    }

    let open_order_fields = null;
    let close_order_fields = null;

    let privKeys = null;

    let positionPrivKey = null;
    let perpPosition = null;

    if (position_effect_type == "Open") {
      // ? Get the notesIn and priv keys for these notes
      let { notesIn, refundAmount } = this.getNotesInAndRefundAmount(
        collateral_token,
        initial_margin
      );

      // ? Generate the dest spent and dest received addresses and blindings
      privKeys = notesIn.map((x) => x.privKey);
      let { KoS, koS, ytS } = this.getDestSpentAddresses(privKeys);

      // ? generate the refund note
      let refundNote =
        refundAmount > 0
          ? new Note(
              KoS,
              collateral_token,
              refundAmount,
              ytS,
              notesIn[0].note.index
            )
          : null;

      let { positionPrivKey, positionAddress } =
        this.getPositionAddress(synthetic_token);

      open_order_fields = new OpenOrderFields(
        initial_margin,
        collateral_token,
        notesIn.map((n) => n.note),
        refundNote,
        positionAddress.getX().toString(),
        ytS
      );

      storeUserData(this.userId, this.noteCounts, this.positionCounts);

      storePrivKey(this.userId, koS, false);
      storePrivKey(this.userId, positionPrivKey, true);
    } else if (position_effect_type == "Close") {
      let { KoR, koR, ytR } = this.getDestReceivedAddresses(collateral_token);

      close_order_fields = new CloseOrderFields(KoR, ytR);

      storeUserData(this.userId, this.noteCounts, this.positionCounts);

      storePrivKey(this.userId, koR, false);

      // ? Get the position priv Key for this position
      for (let position of this.positionData[synthetic_token]) {
        if (position.position_address == position_address) {
          perpPosition = position;
          positionPrivKey = this.positionPrivKeys[position_address];

          if (position.order_side == "Long") {
            order_side = "Short";
          } else {
            order_side = "Long";
          }

          break;
        }
      }

    } else {
      // ? Get the position priv Key for this position
      for (let position of this.positionData[synthetic_token]) {
        if (position.position_address == position_address) {
          perpPosition = position;
          positionPrivKey = this.positionPrivKeys[position_address];

          break;
        }
      }
    }

    let privKeySum;
    if (privKeys) {
      privKeySum = privKeys.reduce((a, b) => a + b, 0n);
    }

    let perpOrder = new PerpOrder(
      expiration_timestamp,
      perpPosition,
      position_effect_type,
      order_side,
      synthetic_token,
      synthetic_amount,
      collateral_amount,
      price,
      fee_limit,
      open_order_fields,
      close_order_fields
    );


    let _signature = perpOrder.signOrder(privKeys, positionPrivKey);

    return { perpOrder, pfrKey: privKeySum };
  }

  makeLiquidationOrder(expiration_timestamp, position) {
    // ? Get the position priv Key for this position
    let order_side = position.order_side == "Long" ? "Short" : "Long";
    let collateral_amount =
      position.order_side == "Long" ? 1 : 1_000_000_000_000_000; // want the price to be as low as possible for sell and as high as possible for buy
    let perpOrder = new PerpOrder(
      expiration_timestamp,
      position,
      "Liquidate",
      order_side,
      position.synthetic_token,
      position.position_size,
      collateral_amount,
      0,
      position.position_size * 0.01,
      null,
      null
    );

    return perpOrder;
  }

  makeLimitOrder(
    expiration_timestamp,
    token_spent,
    token_received,
    amount_spent,
    amount_received,
    price,
    fee_limit
  ) {
    // ? Get the notesIn and priv keys for these notes
    let { notesIn, refundAmount } = this.getNotesInAndRefundAmount(
      token_spent,
      amount_spent
    );

    // ? Generate the dest spent and dest received addresses and blindings
    let privKeys = notesIn.map((x) => x.privKey);
    let { KoS, koS, ytS } = this.getDestSpentAddresses(privKeys);
    let { KoR, koR, ytR } = this.getDestReceivedAddresses(token_received);
    let privKeySum = privKeys.reduce((a, b) => a + b, 0n);

    // ? generate the refund note
    let refundNote =
      refundAmount > 0
        ? new Note(KoS, token_spent, refundAmount, ytS, notesIn[0].note.index)
        : null;

    let limitOrder = new LimitOrder(
      expiration_timestamp,
      token_spent,
      token_received,
      amount_spent,
      amount_received,
      price,
      fee_limit,
      KoS,
      KoR,
      ytS,
      ytR,
      notesIn.map((x) => x.note),
      refundNote
    );

    let _sig = limitOrder.signOrder(privKeys);

    storeUserData(this.userId, this.noteCounts, this.positionCounts);

    storePrivKey(this.userId, koS, false);
    storePrivKey(this.userId, koR, false);

    return { limitOrder, pfrKey: privKeySum };
  }

  makeDepositOrder(depositId, depositAmount, depositToken, starkKey) {
    let depositStarkKey = this.getDepositStarkKey(depositToken);
    let privKey = this._getDepositStarkPrivKey(depositToken);

    if (starkKey != depositStarkKey) {
      throw new Error("Unknown stark key");
    }

    let { KoR, koR, ytR } = this.getDestReceivedAddresses(depositToken);
    let note = new Note(KoR, depositToken, depositAmount, ytR);

    let sig = Deposit.signDeposit(depositId, [note], privKey);

    let deposit = new Deposit(
      depositId,
      depositToken,
      depositAmount,
      depositStarkKey,
      [note],
      sig
    );

    this.notePrivKeys[BigInt(note.address.getX())] = koR;

    storeUserData(this.userId, this.noteCounts, this.positionCounts);

    storePrivKey(this.userId, koR, false);

    return deposit;
  }

  makeWithdrawalOrder(withdrawAmount, withdrawToken, withdrawStarkKey) {
    // ? Get the notesIn and priv keys for these notes
    let { notesIn, refundAmount } = this.getNotesInAndRefundAmount(
      withdrawToken,
      withdrawAmount
    );

    // ? Generate the dest spent and dest received addresses and blindings
    let privKeys = notesIn.map((x) => x.privKey);
    notesIn = notesIn.map((x) => x.note);
    let { KoS, koS, ytS } = this.getDestSpentAddresses(privKeys);

    // ? generate the refund note
    let refundNote = new Note(
      KoS,
      withdrawToken,
      refundAmount,
      ytS,
      notesIn[0].index
    );

    let signature = Withdrawal.signWithdrawal(
      notesIn,
      privKeys,
      refundNote,
      withdrawStarkKey
    );

    let withdrawal = new Withdrawal(
      withdrawToken,
      withdrawAmount,
      withdrawStarkKey,
      notesIn,
      refundNote,
      signature
    );

    storeUserData(this.userId, this.noteCounts, this.positionCounts);

    storePrivKey(this.userId, koS, false);

    return withdrawal;
  }

  restructureNotes(token, newAmounts) {
    if (newAmounts.length == 0) throw Error("No new amounts provided");

    let sum = newAmounts.reduce((a, b) => a + b, 0);

    let { notesIn, refundAmount } = this.getNotesInAndRefundAmount(token, sum);

    let addresses = notesIn.map((x) => x.note.address);
    let blindings = notesIn.map((x) => x.note.blinding);
    let l = addresses.length;

    let newNoteData = [];
    for (let i = 0; i < newAmounts.length; i++) {
      let newNote = new Note(
        addresses[i % l],
        token,
        newAmounts[i],
        blindings[i % l]
      );

      newNoteData.push(newNote);
    }

    // ? generate the refund note
    if (refundAmount > 0) {
      let refundNote = new Note(
        addresses[0],
        token,
        refundAmount,
        blindings[0]
      );

      newNoteData.push(refundNote);
    }

    return { notesIn: notesIn.map((n) => n.note), notesOut: newNoteData };
  }

  changeMargin(positionAddress, token, direction, amount) {
    if (amount == 0) throw Error("amount is zero");

    let position;
    let positionPrivKey;
    for (let position_ of this.positionData[token]) {
      if (position_.position_address == positionAddress) {
        position = position_;
        positionPrivKey = this.positionPrivKeys[positionAddress];

        break;
      }
    }
    if (position == null) throw Error("Position not found");

    let notes_in;
    let refund_note;
    let close_order_fields;
    let signature;
    if (direction == "increase") {
      // ? Get the notesIn and priv keys for these notes

      let { notesIn, refundAmount } = this.getNotesInAndRefundAmount(
        position.collateral_token,
        amount
      );

      // ? generate the refund note
      if (refundAmount > 0) {
        refund_note = new Note(
          notesIn[0].note.address,
          notesIn[0].note.token,
          refundAmount,
          notesIn[0].note.blinding,
          notesIn[0].note.index
        );
      }

      signature = signMarginChange(
        direction,
        amount,
        notesIn,
        refund_note,
        close_order_fields,
        position,
        positionPrivKey
      );

      notes_in = notesIn.map((n) => n.note);
    } else if (direction == "decrease") {
      let { KoR, koR, ytR } = this.getDestReceivedAddresses(
        position.collateral_token
      );

      close_order_fields = new CloseOrderFields(KoR, ytR);

      signature = signMarginChange(
        direction,
        amount,
        notes_in,
        refund_note,
        close_order_fields,
        position,
        positionPrivKey
      );

      // this.perpetualOrderIds.push(order_id);
      storeUserData(this.userId, this.noteCounts, this.positionCounts);
      storePrivKey(this.userId, koR, false);
    } else throw Error("Invalid direction");

    return {
      notes_in,
      refund_note,
      close_order_fields,
      position,
      signature,
    };
  }

  // * ORDER HELPERS ============================================================
  getDestReceivedAddresses(tokenReceived) {
    // & This returns the dest received address and blinding

    // ? Get a pseudo-random deterministic number
    // ? from the private seed and token count to generate an address
    let noteCount2 = this.noteCounts[tokenReceived] ?? 0;

    // ? Update the note count
    this.noteCounts[tokenReceived] = (noteCount2 + 1) % 50;

    // ? Generate a new address and private key pair
    let koR = this.oneTimeAddressPrivKey(noteCount2, tokenReceived);
    let KoR = getKeyPair(koR).getPublic();

    // ? Get the blinding for the note
    let ytR = this.generateBlinding(KoR);

    return { KoR, koR, ytR };
  }

  getDestSpentAddresses(privKeys) {
    // & This returns the dest spent address and blinding

    let koS = privKeys.reduce((acc, x) => acc + x, 0n);
    let KoS = getKeyPair(koS).getPublic();

    let ytS = this.generateBlinding(KoS);

    return { KoS, koS, ytS };
  }

  getNotesInAndRefundAmount(token, spendAmount) {
    // ? Get the notes in and refund note
    let notesIn = [];
    let amount = 0;

    let len = this.noteData[token].length;
    for (let i = 0; i < len; i++) {
      const note = this.noteData[token].pop();
      const privKey = this.notePrivKeys[BigInt(note.address.getX())];

      amount += note.amount;
      notesIn.push({ privKey, note });

      // ? Get the refund note
      if (amount >= spendAmount) {
        let refundAmount = amount - Number.parseInt(spendAmount);

        return { notesIn, refundAmount };
      }
    }

    // ? If we get here, we don't have enough notes to cover the amount
    throw new Error("Insufficient funds");
  }

  getPositionAddress(syntheticToken) {
    let posCount = this.positionCounts[syntheticToken] ?? 0;

    this.positionCounts[syntheticToken] = (posCount + 1) % 50;

    let positionPrivKey = this.oneTimeAddressPrivKey(posCount, syntheticToken);
    let positionAddress = getKeyPair(positionPrivKey).getPublic();

    return { positionPrivKey, positionAddress };
  }

  getDepositStarkKey(depositToken) {
    let depositStarkKey = getKeyPair(this._getDepositStarkPrivKey(depositToken))
      .getPublic()
      .getX();
    return BigInt(depositStarkKey);
  }

  _getDepositStarkPrivKey(depositToken) {
    // TODO: This is a temporary function to get the deposit stark key
    return pedersen([this.privateSeed, depositToken]);
  }

  //* HELPERS ===========================================================================

  subaddressPrivKeys(token) {
    return _subaddressPrivKeys(this.privSpendKey, this.privViewKey, token);
  }

  oneTimeAddressPrivKey(count, token) {
    let { ksi, kvi } = this.subaddressPrivKeys(token);
    let Kvi = getKeyPair(kvi).getPublic();

    return _oneTimeAddressPrivKey(Kvi, ksi, count);
  }

  generateBlinding(Ko) {
    return _generateNewBliding(Ko.getX(), this.privateSeed);
  }

  // Hides the values for the recipient
  hideValuesForRecipient(Ko, amount) {
    return _hideValuesForRecipient(Ko, amount, this.privateSeed);
  }

  // Used to reveal the blindings and amounts of the notes addressed to this user's ith subaddress
  revealHiddenValues(Ko, hiddenAmount, commitment) {
    return _revealHiddenValues(Ko, this.privateSeed, hiddenAmount, commitment);
  }

  // // Checks if the transaction is addressed to this user's its subaddress
  // checkOwnership(rKsi, Ko, ith = 1) {
  //   return _checkOwnership(rKsi, Ko, this.privSpendKey, this.privViewKey, ith);
  // }

  //* TESTS =======================================================

  static fromPrivKey(privKey) {
    // & Generates a privViewKey and privSpendKey from one onchain private key and generates a user from it
    let privViewKey = trimHash(pedersen([VIEW_KEY_MASK, privKey]), 240);
    let privSpendKey = trimHash(pedersen([SPEND_KEY_MASK, privKey]), 240);

    let user = new User(privViewKey, privSpendKey);

    return user;
  }

  static getkeyPairsFromPrivKeys(privKeys) {
    let keyPairs = [];
    for (let privKey of privKeys) {
      let keyPair = getKeyPair(privKey);
      keyPairs.push(keyPair);
    }

    return keyPairs;
  }
};

//
//
//
//
//
//
//
//
//
//
