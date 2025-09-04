use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use serde::Serialize;
use std::{process::Stdio, time::Duration};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};

use crate::account_cli::AccountJson;
use crate::account_path::account_json_path;
use crate::parse::{parse_event, MinerEvent};

#[derive(Debug, Clone, Serialize)]
struct LogMsg {
    source: &'static str,
    line: String,
}

lazy_static! {
    static ref MINER: Mutex<Option<tokio::process::Child>> = Mutex::new(None);
    static ref LAST_CFG: Mutex<Option<MinerConfig>> = Mutex::new(None);
    static ref REPAIRING: Mutex<bool> = Mutex::new(false);
}

// --- Node key helpers ---
// Base data dir used by quantus-node, e.g. on Linux: ~/.local/share/quantus-node
fn node_base_path() -> Result<std::path::PathBuf> {
    let data = dirs::data_dir().ok_or_else(|| anyhow!("no data_dir available"))?;
    Ok(data.join("quantus-node"))
}

// On-disk chain id mapping (resonance -> "resonance", etc.)
fn chain_id_for_ui(chain_ui: &str) -> &str {
    match chain_ui {
        "resonance" => "resonance",
        "heisenberg" => "heisenberg",
        "quantus" => "quantus",
        other => other,
    }
}

// {base}/chains/{chain_id}/network/secret_dilithium
fn node_key_file_path_for_chain(chain_id: &str) -> Result<std::path::PathBuf> {
    Ok(node_base_path()?
        .join("chains")
        .join(chain_id)
        .join("network")
        .join("secret_dilithium"))
}

// Ensure the node key exists; if missing, generate it via:
//   quantus-node key generate-node-key --file <path>
async fn ensure_node_key_for(
    chain_id: &str,
    quantus_node_path: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let key_path = node_key_file_path_for_chain(chain_id)?;
    if let Some(parent) = key_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if key_path.exists() {
        return Ok(key_path);
    }

    let out = Command::new(quantus_node_path)
        .args([
            "key",
            "generate-node-key",
            "--file",
            &key_path.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| anyhow!("spawn keygen: {e}"))?;

    if !out.status.success() {
        return Err(anyhow!(
            "node key generation failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    if !key_path.exists() {
        return Err(anyhow!(
            "node key generation reported success but file not found at {}",
            key_path.display()
        ));
    }
    Ok(key_path)
}

#[derive(Debug, Clone, Serialize)]
pub struct MinerConfig {
    pub chain: String, // "resonance" | "heisenberg"
    pub rewards_address: String,
    pub binary_path: String,
    pub extra_args: Vec<String>,
}

pub async fn start(app: AppHandle, cfg: MinerConfig) -> Result<()> {
    // ensure previous child is stopped
    stop().await.ok();

    let acct_path = account_json_path(&app);
    let acct = AccountJson::load_from_file(&acct_path)?;
    // Map UI chain to CLI arg; disable heisenberg until required binary is released
    let cli_chain = match cfg.chain.as_str() {
        "resonance" => "live_resonance",
        "heisenberg" => {
            return Err(anyhow!(
                "Heisenberg is not available yet (requires quantus-node 0.1.6-98ceb8de72a)"
            ));
        }
        other => other,
    };

    // ensure node key exists and fetch its path for the selected chain
    let chain_id = chain_id_for_ui(&cfg.chain);
    let node_key_path =
        ensure_node_key_for(chain_id, std::path::Path::new(&cfg.binary_path)).await?;

    {
        // remember the last start configuration for potential auto-repair restart
        let mut last = LAST_CFG.lock().await;
        *last = Some(cfg.clone());
    }

    let mut args = vec![
        "--chain".into(),
        cli_chain.into(),
        "--validator".into(),
        "--node-key-file".into(),
        node_key_path.to_string_lossy().to_string(),
        "--rewards-address".into(),
        acct.address.clone(),
    ];
    args.extend(cfg.extra_args.clone());

    let mut cmd = Command::new(cfg.binary_path);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| anyhow!("spawn miner: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(ev) = parse_event(&line) {
                let _ = app_clone.emit("miner:event", &ev);
            }
            let _ = app_clone.emit(
                "miner:log",
                &LogMsg {
                    source: "stdout",
                    line: line.clone(),
                },
            );
        }
    });

    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            // surface stderr as logs; parse too (some miners log success to stderr)
            if let Some(ev) = parse_event(&line) {
                let _ = app_clone.emit("miner:event", &ev);
            }
            let low = line.to_lowercase();
            let _ = app_clone.emit(
                "miner:log",
                &LogMsg {
                    source: "stderr",
                    line,
                },
            );

            // Detect RocksDB corruption that needs a DB wipe and full resync:
            // "Invalid argument: Column families not opened: col12, col11, ..."
            if low.contains("invalid argument: column families not opened") {
                // Backend will not auto-repair here to avoid non-Send spawn issues.
                // Emit a hint so the UI can offer a "Repair" action that calls `repair_and_restart`.
                let _ = app_clone.emit(
                    "miner:log",
                    &LogMsg {
                        source: "ui",
                        line: "Detected RocksDB corruption. Please use Repair to wipe the database and restart (full resync will be required).".into(),
                    },
                );
            }
        }
    });

    *MINER.lock().await = Some(child);
    Ok(())
}

pub async fn stop() -> Result<()> {
    if let Some(mut child) = MINER.lock().await.take() {
        #[cfg(target_family = "unix")]
        {
            use nix::sys::signal::{kill, Signal::SIGINT};
            use nix::unistd::Pid;
            let _ = kill(Pid::from_raw(child.id().unwrap_or(0) as i32), SIGINT);
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        let _ = child.kill().await;
    }
    Ok(())
}

pub async fn repair_and_restart(app: AppHandle) -> Result<()> {
    // We rely on the last configuration to restart after repair.
    let cfg = { LAST_CFG.lock().await.clone() }
        .ok_or_else(|| anyhow!("no previous miner configuration available"))?;

    let chain_id = chain_id_for_ui(&cfg.chain);
    let db_path = node_base_path()?
        .join("chains")
        .join(chain_id)
        .join("db")
        .join("full");

    let _ = app.emit(
        "miner:log",
        &LogMsg {
            source: "ui",
            line: "Stopping node to repair database...".into(),
        },
    );
    let _ = stop().await;

    if db_path.exists() {
        std::fs::remove_dir_all(&db_path)
            .map_err(|e| anyhow!("failed to wipe database at {}: {e}", db_path.display()))?;
    }

    let _ = app.emit(
        "miner:log",
        &LogMsg {
            source: "ui",
            line: format!(
                "Database wiped at {}. Restarting node to resync from scratch...",
                db_path.display()
            ),
        },
    );

    start(app, cfg).await
}
