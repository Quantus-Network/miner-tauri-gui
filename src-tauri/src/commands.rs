use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{
    miner::{self, MinerConfig},
    rpc,
};

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
    pub rewards_address: String,
    pub binary_path: String,
    pub extra_args: Vec<String>,
}

#[tauri::command]
pub async fn start_miner(app: AppHandle, args: StartMinerArgs) -> Result<(), String> {
    miner::start(
        app,
        MinerConfig {
            chain: args.chain,
            rewards_address: args.rewards_address,
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

#[tauri::command]
pub async fn ensure_miner_and_account(app: AppHandle) -> Result<serde_json::Value, String> {
    let miner_path = crate::installer::ensure_quantus_node_installed()
        .await
        .map_err(|e| e.to_string())?;
    let acct_path = crate::account_path::account_json_path(&app);
    let acct = crate::account_cli::ensure_account_json(&app, &miner_path, &acct_path)
        .await
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
      "minerPath": miner_path.to_string_lossy(),
      "account": acct,
      "accountJsonPath": acct_path.to_string_lossy(),
    }))
}
