use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub address: String,
    pub public_key_b64: String,
    pub secret_key_b64: String,
    pub created_at: String,
}

const ACCOUNT_FILENAME: &str = "mining-rewards-account.json";

pub fn app_data_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app_data_dir")
        .to_path_buf()
}

pub fn ensure_app_dir(app: &AppHandle) -> PathBuf {
    let dir = app_data_dir(app);
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn account_path(app: &AppHandle) -> PathBuf {
    ensure_app_dir(app).join(ACCOUNT_FILENAME)
}

/// DEMO-GRADE keygen: we just make random bytes and derive a faux address.
/// Replace this with a call into your CLI or a proper ML-DSA keygen when ready.
pub fn generate_account_unencrypted(app: &AppHandle) -> Result<Account> {
    let mut sk = vec![0u8; 64];
    rand::thread_rng().fill_bytes(&mut sk);
    // fake pub as hash of sk (demo only). Swap for real key derivation.
    let pk = blake3::hash(&sk).as_bytes().to_vec();

    // demo address: take first 20 bytes of blake3(pk) and hex it with "res" prefix.
    let addr_hash = blake3::hash(&pk);
    let addr = format!("res{}", hex::encode(&addr_hash.as_bytes()[..20]));

    let acct = Account {
        address: addr,
        public_key_b64: base64::encode(pk),
        secret_key_b64: base64::encode(sk),
        created_at: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
    };
    let path = account_path(app);
    fs::write(path, serde_json::to_vec_pretty(&acct)?)?;
    Ok(acct)
}

pub fn load_account(app: &AppHandle) -> Result<Account> {
    let path = account_path(app);
    if !path.exists() {
        return Err(anyhow!("no account"));
    }
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}
