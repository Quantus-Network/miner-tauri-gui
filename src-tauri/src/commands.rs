use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

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
    #[serde(default)]
    pub log_to_file: bool,
    // external parallel miner settings (optional)
    #[serde(default)]
    pub external_num_cores: Option<usize>,
    #[serde(default)]
    pub external_port: Option<u16>,
}

#[tauri::command]
pub async fn start_miner(app: AppHandle, args: StartMinerArgs) -> Result<(), String> {
    #[derive(Serialize)]
    struct UiLog<'a> {
        source: &'a str,
        line: String,
    }

    let _ = app.emit(
        "miner:log",
        &UiLog {
            source: "ui",
            line: format!(
                "Starting miner: binary={}, chain={}, rewards_address={}, extra_args={:?}",
                args.binary_path, args.chain, args.rewards_address, args.extra_args
            ),
        },
    );

    let app_clone = app.clone();
    match miner::start(
        app,
        MinerConfig {
            chain: args.chain,
            rewards_address: args.rewards_address,
            binary_path: args.binary_path,
            extra_args: args.extra_args,
            log_to_file: args.log_to_file,
            external_num_cores: args.external_num_cores,
            external_port: args.external_port,
        },
    )
    .await
    {
        Ok(_) => {
            let _ = app_clone.emit(
                "miner:log",
                &UiLog {
                    source: "ui",
                    line: "Miner started".into(),
                },
            );
            Ok(())
        }
        Err(e) => {
            let msg = format!("Start failed: {e}");
            let _ = app_clone.emit(
                "miner:log",
                &UiLog {
                    source: "ui",
                    line: msg.clone(),
                },
            );
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn stop_miner(app: AppHandle) -> Result<(), String> {
    // Inform UI immediately that we're stopping so buttons flip without waiting.
    let _ = app.emit(
        "miner:state",
        &serde_json::json!({ "running": false, "phase": "stopped" }),
    );
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
    let ws = crate::rpc::bootnode_ws_for_chain(chain.as_str())
        .ok_or_else(|| "unknown chain".to_string())?;
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

#[tauri::command]
pub async fn repair_miner(app: AppHandle) -> Result<(), String> {
    miner::repair_and_restart(app)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unlock_miner(app: AppHandle) -> Result<(), String> {
    miner::unlock_and_restart(app)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SafeRangesPayload {
    pub chains: std::collections::HashMap<String, Vec<[u64; 2]>>,
}

#[tauri::command]
pub async fn get_safe_ranges(app: AppHandle) -> Result<SafeRangesPayload, String> {
    // Read from current in-memory map built by miner.rs at start() time.
    let map = {
        let guard = crate::miner::SAFE_RANGES.lock().await;
        guard.clone()
    };
    let mut chains: std::collections::HashMap<String, Vec<[u64; 2]>> =
        std::collections::HashMap::new();
    for (k, v) in map {
        chains.insert(k, v.into_iter().map(|(a, b)| [a, b]).collect());
    }
    Ok(SafeRangesPayload { chains })
}

#[tauri::command]
pub async fn set_safe_ranges(app: AppHandle, payload: SafeRangesPayload) -> Result<(), String> {
    // Convert payload to internal format and save to disk, then update memory map.
    let mut new_map: std::collections::HashMap<String, Vec<(u64, u64)>> =
        std::collections::HashMap::new();
    for (k, ranges) in payload.chains {
        new_map.insert(k, ranges.into_iter().map(|p| (p[0], p[1])).collect());
    }
    crate::miner::save_safe_ranges(&app, &new_map).map_err(|e| e.to_string())?;
    {
        let mut guard = crate::miner::SAFE_RANGES.lock().await;
        *guard = new_map;
    }
    Ok(())
}
