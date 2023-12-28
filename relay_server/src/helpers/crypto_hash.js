// Import the Rust-generated WebAssembly package
const PoseidonHash = require("../../poseidon_pkg/poseidon");

function hash2(vec2) {
  let h = PoseidonHash.poseidon_hash_js(vec2[0].toString(), vec2[1].toString());

  return BigInt(h);
}

function computeHashOnElements(arr) {
  let h = PoseidonHash.poseidon_hash_many_js(arr.map((x) => x.toString()));

  return BigInt(h);
}

module.exports = { hash2, computeHashOnElements };
