use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{
    account::{generate_account_unencrypted, load_account, Account},
    miner::{self, MinerConfig},
    rpc,
};

#[tauri::command]
pub async fn init_account(app: AppHandle) -> Result<Account, String> {
    match load_account(&app) {
        Ok(a) => Ok(a),
        Err(_) => generate_account_unencrypted(&app).map_err(|e| e.to_string()),
    }
}

#[tauri::command]
pub async fn read_account(app: AppHandle) -> Result<Account, String> {
    load_account(&app).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChainSelection {
    pub chain: String,
}

#[tauri::command]
pub async fn select_chain(_app: AppHandle, sel: ChainSelection) -> Result<(), String> {
    // keep selection in frontend; backend doesn’t need to persist yet
    if sel.chain != "resonance" && sel.chain != "heisenberg" && sel.chain != "quantus" {
        return Err("unknown chain".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartMinerArgs {
    pub chain: String,
    pub binary_path: String,
    pub extra_args: Vec<String>,
}

#[tauri::command]
pub async fn start_miner(app: AppHandle, args: StartMinerArgs) -> Result<(), String> {
    miner::start(
        app,
        MinerConfig {
            chain: args.chain,
            binary_path: args.binary_path,
            extra_args: args.extra_args,
        },
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_miner() -> Result<(), String> {
    miner::stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_log_tail() -> Result<Vec<String>, String> {
    // keep it simple: UI subscribes to "miner:log" instead of pulling tails.
    Ok(vec![])
}

#[tauri::command]
pub async fn query_balance(
    _app: AppHandle,
    chain: String,
    address: String,
) -> Result<crate::rpc::BalanceView, String> {
    let ws = match chain.as_str() {
        "resonance" => "wss://a.t.res.fm", // (you gave this)
        "heisenberg" => "wss://a.i.res.fm",
        // "quantus" => "...", // mainnet – disabled in UI for now
        _ => return Err("unknown chain".into()),
    };
    rpc::fetch_balance(ws, &address)
        .await
        .map_err(|e| e.to_string())
}
