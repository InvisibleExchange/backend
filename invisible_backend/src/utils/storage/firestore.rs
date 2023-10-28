use std::{
    fs::File,
    io::Read,
    sync::Arc,
    thread::{spawn, JoinHandle},
    time::SystemTime,
};

use firestore_db_and_auth::{documents, Credentials, ServiceSession};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    order_tab::OrderTab,
    perpetual::perp_position::PerpPosition,
    transactions::transaction_helpers::transaction_output::{FillInfo, PerpFillInfo},
    trees::superficial_tree::SuperficialTree,
    utils::notes::Note,
};

use super::{
    firestore_helpers::{
        delete_note_at_address, delete_order_tab, delete_position_at_address, store_new_note,
        store_new_position, store_order_tab,
    },
    local_storage::BackupStorage,
};

// * ==================================================================================

pub fn create_session() -> ServiceSession {
    let mut cred =
        Credentials::from_file("firebase-service-account.json").expect("Read credentials file");
    cred.download_google_jwks().expect("Download Google JWKS");

    let session = ServiceSession::new(cred).expect("Create a service account session");

    session
}

pub fn retry_failed_updates(
    state_tree: &Arc<Mutex<SuperficialTree>>,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let s: parking_lot::lock_api::MutexGuard<parking_lot::RawMutex, BackupStorage> =
        backup_storage.lock();
    let notes_info = s.read_notes();
    let positions_info = s.read_positions();
    let order_tabs_info = s.read_order_tabs();
    let spot_fills = s.read_spot_fills();
    let perp_fills = s.read_perp_fills();

    s.clear_db().unwrap();
    drop(s);

    let sess = session.lock();

    // ? ADD AND REMOVE NOTES TO/FROM THE DATABASE
    let state_tree_m = state_tree.lock();
    let notes = notes_info.0;
    for note in notes {
        if note.hash == state_tree_m.get_leaf_by_index(note.index) {
            store_new_note(&sess, backup_storage, &note);
        }
    }
    let removable_info = notes_info.1;
    for (idx, address) in removable_info {
        delete_note_at_address(&sess, backup_storage, &address, &idx.to_string());
    }

    // ? ADD AND REMOVE POSITIONS TO/FROM THE DATABASE
    let positions = positions_info.0;
    for position in positions {
        if position.hash == state_tree_m.get_leaf_by_index(position.index as u64) {
            if position.hash == position.hash_position() {
                store_new_position(&sess, backup_storage, &position);
            }
        }
    }
    let removable_info = positions_info.1;
    for (idx, address) in removable_info {
        delete_position_at_address(&sess, backup_storage, &address, &idx.to_string());
    }

    // ? ADD AND REMOVE ORDER TABS TO/FROM THE DATABASE
    let order_tabs = order_tabs_info.0;
    for tab in order_tabs {
        if tab.hash == state_tree_m.get_leaf_by_index(tab.tab_idx as u64) {
            store_order_tab(&sess, backup_storage, &tab);
        }
    }
    let removable_info = order_tabs_info.1;
    for (idx, address) in removable_info {
        delete_order_tab(&sess, backup_storage, &address, &idx.to_string());
    }

    drop(state_tree_m);

    for fill in spot_fills {
        store_new_spot_fill(&sess, backup_storage, &fill);
    }

    for fill in perp_fills {
        store_new_perp_fill(&sess, backup_storage, &fill);
    }

    Ok(())
}

// FILLS   -------------- ---------------- ----------------- ----------------

fn store_new_spot_fill(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    fill_info: &FillInfo,
) {
    let write_path = format!("fills");

    let doc_id: Option<String> = None;
    let _res = documents::write(
        session,
        write_path.as_str(),
        doc_id,
        &fill_info,
        documents::WriteOptions::default(),
    );

    if let Err(_e) = _res {
        let s = backup_storage.lock();
        if let Err(e) = s.store_spot_fill(fill_info) {
            println!("Error storing spot fill in backup storage. ERROR: {:?}", e);
        };
        drop(s);
    }
}

fn store_new_perp_fill(
    session: &ServiceSession,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    fill_info: &PerpFillInfo,
) {
    let write_path = format!("perp_fills");

    let doc_id: Option<String> = None;
    let _res = documents::write(
        session,
        write_path.as_str(),
        doc_id,
        &fill_info,
        documents::WriteOptions::default(),
    );

    if let Err(_e) = _res {
        let s = backup_storage.lock();
        if let Err(e) = s.store_perp_fill(fill_info) {
            println!("Error storing perp fill in backup storage. ERROR: {:?}", e);
        };
        drop(s);
    }
}

// * PUBLIC FUNCTIONS ===============================================================

// NOTES

pub fn start_add_note_thread(
    note: Note,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();
        // let backup_storage = backup_storage.lock();

        store_new_note(&session_, &backup, &note);
        drop(session_);
    });
    return handle;
}

pub fn start_delete_note_thread(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    address: String,
    idx: String,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();
        delete_note_at_address(&session_, &backup, address.as_str(), idx.as_str());
        drop(session_);
    });
    return handle;
}

// POSITIONS

pub fn start_add_position_thread(
    position: PerpPosition,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();

        store_new_position(&session_, &backup, &position);
        drop(session_);
    });
    return handle;
}

pub fn start_delete_position_thread(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    address: String,
    idx: String,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();
        delete_position_at_address(&session_, &backup, address.as_str(), idx.as_str());
        drop(session_);
    });
    return handle;
}

// ORDER TABS

pub fn start_add_order_tab_thread(
    order_tab: OrderTab,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();
        // let backup_storage = backup_storage.lock();

        store_order_tab(&session_, &backup, &order_tab);
        drop(session_);
    });
    return handle;
}

pub fn start_delete_order_tab_thread(
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
    pub_key: String,
    idx: String,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();
        delete_order_tab(&session_, &backup, pub_key.as_str(), idx.as_str());
        drop(session_);
    });
    return handle;
}

// FILLS

pub fn start_add_fill_thread(
    fill_info: FillInfo,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();

        store_new_spot_fill(&session_, &backup, &fill_info);
        drop(session_);
    });
    return handle;
}

pub fn start_add_perp_fill_thread(
    fill_info: PerpFillInfo,
    session: &Arc<Mutex<ServiceSession>>,
    backup_storage: &Arc<Mutex<BackupStorage>>,
) -> JoinHandle<()> {
    let s = Arc::clone(&session);
    let backup = Arc::clone(&backup_storage);

    let handle = spawn(move || {
        let session_ = s.lock();

        store_new_perp_fill(&session_, &backup, &fill_info);
        drop(session_);
    });

    return handle;
}

// * FIREBASE STORAGE ===============================================================

use reqwest::Client;
use serde_json::{from_slice, Map, Value};

// Define a struct to deserialize the response from the Firebase Storage API
#[derive(Deserialize)]
struct UploadResponse {
    name: String,
}

#[derive(Serialize, Deserialize)]
struct JsonSerdeMapWrapper(Map<String, Value>);

pub async fn upload_file_to_storage(
    file_name: String,
    serialized_data: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    //

    let (access_token, storage_bucket_url) = get_access_token()?;

    // Create a reqwest client
    let client = Client::new();

    // let serialized_data = to_vec(&value).expect("Serialization failed");

    // Make a POST request to upload the file
    let url = format!(
        "https://firebasestorage.googleapis.com/v0/b/{}/o?name={}",
        storage_bucket_url, file_name
    );

    let response = client
        .post(url)
        .header("Content-Type", "application/octet-stream")
        .header("Authorization", "Bearer ".to_owned() + &access_token)
        .body(serialized_data)
        .send()
        .await?;

    // Deserialize the response
    let upload_response: UploadResponse = match response.json().await {
        Ok(r) => r,
        Err(e) => {
            println!("Error uploading file to storage. ERROR: {:?}", e);
            return Ok(());
        }
    };

    println!(
        "File uploaded successfully. File name: {}",
        upload_response.name
    );

    Ok(())
}

pub async fn read_file_from_storage(
    file_name: String,
) -> Result<Map<String, Value>, Box<dyn std::error::Error>> {
    // Create a reqwest client
    let client = Client::new();

    let (access_token, storage_bucket_url) = get_access_token()?;

    // Make a GET request to download the file

    let url = format!(
        "https://firebasestorage.googleapis.com/v0/b/{}/o/{}?alt=media",
        storage_bucket_url, file_name
    );
    let response = client
        .get(url)
        .header("Authorization", "Bearer ".to_string() + &access_token)
        .send()
        .await?;

    // Read the response content as bytes
    let file_content = response.bytes().await?.to_vec();

    let deserialized_data: Map<String, Value> =
        from_slice(&file_content).expect("Deserialization failed");

    Ok(deserialized_data)
}

// * ACCESS TOKENS ====================================================================

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

#[derive(Debug, Serialize, Deserialize)]
struct ServiceAccount {
    #[serde(rename = "project_id")]
    project_id: String,
    #[serde(rename = "private_key_id")]
    private_key_id: String,
    #[serde(rename = "private_key")]
    private_key: String,
    #[serde(rename = "client_email")]
    client_email: String,
    #[serde(rename = "client_id")]
    client_id: String,
    #[serde(rename = "storage_url")]
    storage_url: String,
}

fn get_access_token() -> Result<(String, String), Box<dyn std::error::Error>> {
    // Read the service account file
    let mut file = File::open("firebase-service-account.json").expect("Unable to open the file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Unable to read the file");

    // Parse the service account JSON
    let service_account: ServiceAccount =
        from_slice(contents.as_bytes()).expect("Unable to parse service account JSON");

    // Create the JWT payload
    let claims = Claims {
        iss: service_account.client_email.clone(),
        sub: service_account.client_email.clone(),
        aud: format!("https://identitytoolkit.googleapis.com/google.identity.identitytoolkit.v1.IdentityToolkit"),
        iat: SystemTime::now()
            .duration_since(SystemTime:: UNIX_EPOCH)
            .expect("Unable to get UNIX EPOCH")
            .as_secs() as i64,
        exp: SystemTime::now()
        .duration_since(SystemTime:: UNIX_EPOCH)
        .expect("Unable to get UNIX EPOCH")
        .as_secs() as i64 + 180, // Token expires in 3 minutes
        uid: None,
    };

    // Encode the JWT using the private key
    let header = Header::new(Algorithm::RS256);
    let private_key = EncodingKey::from_rsa_pem(service_account.private_key.as_bytes())
        .expect("Unable to create private key from PEM");
    let token = encode(&header, &claims, &private_key).expect("Unable to encode JWT");

    // Return the access token
    Ok((token, service_account.storage_url))
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
    uid: Option<String>,
}
