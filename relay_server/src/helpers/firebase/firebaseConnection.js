// Import the functions you need from the SDKs you need
const { initializeApp } = require("firebase/app");
const {
  getFirestore,
  getDoc,
  updateDoc,
  setDoc,
  doc,
} = require("firebase/firestore/lite");

const {
  collection,
  getDocs,
  where,
  query,
} = require("firebase/firestore/lite");
const { firebaseConfig } = require("./firebaseAdminConfig");

const { ec, getKeyPair } = require("starknet").ec; //require("starknet/utils/ellipticCurve.js");

// Initialize Firebase
const app = initializeApp(firebaseConfig);

const db = getFirestore(app);

async function getLastDayTrades(isPerp) {
  let now = new Date().getTime() - 24 * 60 * 60 * 1000;
  now = now / 1000;

  let q;
  if (isPerp) {
    q = query(
      collection(db, "perp_fills"),
      where("timestamp", ">=", Number(now))
    );
  } else {
    q = query(collection(db, `fills`), where("timestamp", ">=", Number(now)));
  }

  let token24hVolumes = {};
  let token24hTrades = {};

  const querySnapshot = await getDocs(q);

  let fills = querySnapshot.docs.map((doc) => doc.data());

  for (let fill of fills) {
    let token = isPerp ? fill.synthetic_token : fill.base_token;

    if (!token24hVolumes[token]) {
      token24hVolumes[token] = fill.amount;
      token24hTrades[token] = 1;
    } else {
      token24hVolumes[token] += fill.amount;
      token24hTrades[token] += 1;
    }
  }

  return { token24hVolumes, token24hTrades };
}

async function storeOnchainDeposit(depositObject) {
  console.log("depositObject: ", depositObject);

  let depositDoc = doc(db, "deposits", depositObject.deposit_id.toString());
  let depositData = await getDoc(depositDoc);

  if (depositData.exists()) {
    await updateDoc(depositDoc, depositObject);
  } else {
    await setDoc(depositDoc, depositObject);
  }
}

module.exports = {
  getLastDayTrades,
  storeOnchainDeposit,
};
