const { computeHashOnElements } = require("../helpers/crypto_hash");

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

function getDepositCommitment(deposit) {
  // & h = H(depositId, starkKey, token, deposit_amount)c
  let inputs = [
    deposit.deposit_id,
    deposit.stark_key,
    deposit.deposit_token,
    deposit.deposit_amount,
  ];

  let commitment = computeHashOnElements(inputs);

  let depositCommitment = {
    action_type: GrpcOnchainActionType["DEPOSIT"],
    data_id: (BigInt(deposit.deposit_id) % 2n ** 32n).toString(),
    data_commitment: commitment.toString(),
  };

  return depositCommitment;
}

// * =============================================================================

function getRegisterMMCommitment(
  mm_action_id,
  synthetic_asset,
  position_address,
  vlp_token
) {
  // & hash = H({mm_action_id, synthetic_asset, position_address, vlp_token})
  let inputs = [mm_action_id, synthetic_asset, position_address, vlp_token];

  let commitment = computeHashOnElements(inputs);

  let mmActionCommitment = {
    action_type: GrpcOnchainActionType["MM_REGISTRATION"],
    data_id: mm_action_id.toString(),
    data_commitment: commitment.toString(),
  };

  return mmActionCommitment;
}

function getAddLiquidityCommitment(
  mm_action_id,
  depositor,
  position_address,
  usdc_amount
) {
  // & hash = H({ mm_action_id, depositor, position_address, usdc_amount})
  let inputs = [mm_action_id, depositor, position_address, usdc_amount];

  let commitment = computeHashOnElements(inputs);

  let mmActionCommitment = {
    action_type: GrpcOnchainActionType["MM_REGISTRATION"],
    data_id: mm_action_id.toString(),
    data_commitment: commitment,
  };

  return mmActionCommitment;
}

function getRemoveLiquidityCommitment(
  mm_action_id,
  depositor,
  position_address,
  initial_value,
  vlp_amount
) {
  // & hash = H({ mm_action_id, depositor, position_address, initial_value, vlp_amount})
  let inputs = [
    mm_action_id,
    depositor,
    position_address,
    initial_value,
    vlp_amount,
  ];

  let commitment = computeHashOnElements(inputs);

  let mmActionCommitment = {
    action_type: GrpcOnchainActionType["MM_REGISTRATION"],
    data_id: mm_action_id.toString(),
    data_commitment: commitment,
  };

  return mmActionCommitment;
}

function getCloseMMCommitment(
  mm_action_id,
  position_address,
  initial_value_sum,
  vlp_amount_sum
) {
  // & hash = H({ mm_action_id, position_address, initial_value_sum, vlp_amount_sum})
  let inputs = [
    mm_action_id,
    position_address,
    initial_value_sum,
    vlp_amount_sum,
  ];

  let commitment = computeHashOnElements(inputs);

  let mmActionCommitment = {
    action_type: GrpcOnchainActionType["MM_REGISTRATION"],
    data_id: mm_action_id.toString(),
    data_commitment: commitment,
  };

  return mmActionCommitment;
}

// * =============================================================================

function getNoteEscapeCommitment(escapeId, escape_notes, signature) {
  // & hash = H(escapeId, ...noteHashes, sig)
  let inputs = [escapeId, escape_notes.map((note) => note.hash), ...signature];

  let commitment = computeHashOnElements(inputs);

  let escapeCommitment = {
    action_type: GrpcOnchainActionType["NOTE_ESCAPE"],
    data_id: escapeId,
    data_commitment: commitment,
  };

  return escapeCommitment;
}

function getTabEscapeCommitment(escapeId, orderTab, signature) {
  // & hash = H(escapeId, tab_hash, sig)
  let inputs = [escapeId, orderTab.hash, ...signature];

  let commitment = computeHashOnElements(inputs);

  let escapeCommitment = {
    action_type: GrpcOnchainActionType["TAB_ESCAPE"],
    data_id: escapeId,
    data_commitment: commitment,
  };

  return escapeCommitment;
}

function getPositionEscapeCommitment(
  escapeId,
  closePrice,
  position_hash_a,
  B,
  recipient,
  signature_a,
  signature_b
) {
  // & hash = H(escapeId, closePrice, positionA.address, additionalHashB, recipient, sigA, sigB)
  let inputs = [
    escapeId,
    closePrice,
    position_hash_a,
    additional_hash_b,
    recipient,
    ...signature_a,
    ...signature_b,
  ];

  let commitment = computeHashOnElements(inputs);

  let escapeCommitment = {
    action_type: GrpcOnchainActionType["POSITION_ESCAPE"],
    data_id: escapeId,
    data_commitment: commitment,
  };

  return escapeCommitment;
}

module.exports = {
  getDepositCommitment,
  getNoteEscapeCommitment,
  getTabEscapeCommitment,
  getPositionEscapeCommitment,
  getRegisterMMCommitment,
  getAddLiquidityCommitment,
  getRemoveLiquidityCommitment,
  getCloseMMCommitment,
};
