// Import the functions you need from the SDKs you need
const { initializeApp } = require("firebase/app");
const {
  getFirestore,
  getDoc,
  updateDoc,
  setDoc,
  doc,
  deleteDoc,
} = require("firebase/firestore/lite");

const {
  collection,
  getDocs,
  where,
  query,
} = require("firebase/firestore/lite");
const { firebaseConfig, db: adminDb } = require("./firebaseAdminConfig");

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
  let depositDoc = doc(db, "deposits", depositObject.deposit_id.toString());
  let depositData = await getDoc(depositDoc);

  if (depositData.exists()) {
    await updateDoc(depositDoc, depositObject);
  } else {
    await setDoc(depositDoc, depositObject);
  }
}

async function storeMMAction(mmAction) {
  for (let key of Object.keys(mmAction)) {
    mmAction[key] = mmAction[key].toString();
  }

  let mmActionDoc = doc(db, "mm_actions", mmAction.action_id.toString());

  await setDoc(mmActionDoc, mmAction);
}

async function removeMMAction(mmActionId) {
  let mmActionDoc = doc(db, "mm_actions", mmActionId.toString());

  await deleteDoc(mmActionDoc);
}

async function updatePendingWithdrawals(isL1) {
  let withdrawals = adminDb.collection(
    `withdrawals/${isL1 ? "L1" : "L2"}/pending`
  );
  let docs = await withdrawals.listDocuments();

  let manualWithdrawals = {};
  let futures = [];
  docs.forEach(async (doc) => {
    let f = doc.get().then((doc) => {
      let withdrawalId = doc.id;
      let withdrawalData = doc.data();

      if (!withdrawalData.is_automatic) {
        manualWithdrawals[withdrawalId] = withdrawalData;
      }
    });

    futures.push(f);
  });

  await Promise.all(futures);

  deleteCollection(`withdrawals/${isL1 ? "L1" : "L2"}/pending`);

  let historyWithdrawals = adminDb.collection(
    `withdrawals/${isL1 ? "L1" : "L2"}/history`
  );

  futures = [];
  for (const [withdrawalId, withdrawalData] of Object.entries(
    manualWithdrawals
  )) {
    let docRef = historyWithdrawals.doc(withdrawalId);

    let f = docRef.set(withdrawalData);
    futures.push(f);
  }

  await Promise.all(futures);
}

async function deleteCollection(collectionPath) {
  const collectionRef = adminDb.collection(collectionPath);
  const documents = await collectionRef.listDocuments();

  const chunks = [];
  for (let i = 0; i < documents.length; i += 500) {
    chunks.push(documents.slice(i, i + 500));
  }

  for (const chunk of chunks) {
    const batch = adminDb.batch();
    chunk.forEach((document) => {
      batch.delete(document);
    });
    await batch.commit();
  }
}

async function incrementOrSetDocumentField(docRef, fieldToUpdate, incrementBy) {
  try {
    const docSnapshot = await docRef.get();

    if (docSnapshot.exists) {
      // Document exists, increment the field
      const currentValue = docSnapshot.data()[fieldToUpdate] || 0;
      await docRef.update({
        [fieldToUpdate]: currentValue + incrementBy,
      });
    } else {
      // Document doesn't exist, create a new one with the initial value
      await docRef.set({
        [fieldToUpdate]: incrementBy,
      });
    }
  } catch (error) {
    console.error("Error updating or creating document:", error);
  }
}

module.exports = {
  getLastDayTrades,
  storeOnchainDeposit,
  storeMMAction,
  removeMMAction,
  updatePendingWithdrawals,
};

updatePendingWithdrawals(true);
