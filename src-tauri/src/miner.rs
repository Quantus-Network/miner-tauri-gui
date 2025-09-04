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

#[derive(Debug, Clone, Serialize, Default)]
struct MinerMeta {
    // From our own start context
    binary: Option<String>,
    chain: Option<String>,
    rewards_address: Option<String>,

    // From startup logs
    version: Option<String>,
    chain_spec: Option<String>,
    node_name: Option<String>,
    role: Option<String>,
    database: Option<String>,
    local_identity: Option<String>,
    jsonrpc_addr: Option<String>,
    prometheus_addr: Option<String>,
    highest_known_block: Option<u64>,

    // System info
    os: Option<String>,
    arch: Option<String>,
    target: Option<String>,
    cpu: Option<String>,
    cpu_cores: Option<u32>,
    memory: Option<String>,
    kernel: Option<String>,
    distro: Option<String>,
    vm: Option<String>,
}

// Update MinerMeta with interesting values parsed from a single stderr log line.
// Returns true if any field changed.
fn update_meta_from_line(meta: &mut MinerMeta, line: &str) -> bool {
    let mut changed = false;
    let set = |dst: &mut Option<String>, v: String, changed: &mut bool| {
        if dst.as_deref() != Some(v.as_str()) {
            *dst = Some(v);
            *changed = true;
        }
    };
    let low = line.to_lowercase();

    // Version
    if let Some(ix) = low.find("version ") {
        let v = line[ix + "version ".len()..].trim().to_string();
        if !v.is_empty() {
            set(&mut meta.version, v, &mut changed);
        }
    }
    // Chain specification
    if let Some(ix) = line.find("Chain specification:") {
        let v = line[ix + "Chain specification:".len()..].trim().to_string();
        if !v.is_empty() {
            set(&mut meta.chain_spec, v, &mut changed);
        }
    }
    // Node name
    if let Some(ix) = line.find("Node name:") {
        let v = line[ix + "Node name:".len()..].trim().to_string();
        if !v.is_empty() {
            set(&mut meta.node_name, v, &mut changed);
        }
    }
    // Role
    if let Some(ix) = line.find("Role:") {
        let v = line[ix + "Role:".len()..].trim().to_string();
        if !v.is_empty() {
            set(&mut meta.role, v, &mut changed);
        }
    }
    // Database path
    if let Some(ix) = line.find("Database: RocksDb at") {
        let v = line[ix + "Database: RocksDb at".len()..].trim().to_string();
        if !v.is_empty() {
            set(&mut meta.database, v, &mut changed);
        }
    }
    // Local node identity
    if let Some(ix) = line.find("Local node identity is:") {
        let v = line[ix + "Local node identity is:".len()..]
            .trim()
            .to_string();
        if !v.is_empty() {
            set(&mut meta.local_identity, v, &mut changed);
        }
    }
    // JSON-RPC server address
    if let Some(ix) = line.find("Running JSON-RPC server: addr=") {
        let v = line[ix + "Running JSON-RPC server: addr=".len()..]
            .trim()
            .to_string();
        if !v.is_empty() {
            set(&mut meta.jsonrpc_addr, v, &mut changed);
        }
    }
    // Prometheus exporter
    if let Some(ix) = line.find("Prometheus exporter started at") {
        let v = line[ix + "Prometheus exporter started at".len()..]
            .trim()
            .to_string();
        if !v.is_empty() {
            set(&mut meta.prometheus_addr, v, &mut changed);
        }
    }
    // Rewards address used
    if let Some(ix) = line.find("Using provided rewards address:") {
        let v = line[ix + "Using provided rewards address:".len()..]
            .trim()
            .to_string();
        if !v.is_empty() {
            set(&mut meta.rewards_address, v, &mut changed);
        }
    }
    // Highest known block at #N
    if let Some(ix) = low.find("highest known block at #") {
        let rest = &low[ix + "highest known block at #".len()..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = num.parse::<u64>() {
            if meta.highest_known_block != Some(n) {
                meta.highest_known_block = Some(n);
                changed = true;
            }
        }
    }

    // OS / CPU details
    for (key, dst) in [
        ("Operating system:", &mut meta.os),
        ("CPU architecture:", &mut meta.arch),
        ("Target environment:", &mut meta.target),
        ("CPU:", &mut meta.cpu),
        ("Memory:", &mut meta.memory),
        ("Kernel:", &mut meta.kernel),
        ("Linux distribution:", &mut meta.distro),
        ("Virtual machine:", &mut meta.vm),
    ] {
        if let Some(ix) = line.find(key) {
            let v = line[ix + key.len()..].trim().to_string();
            if !v.is_empty() {
                set(dst, v, &mut changed);
            }
        }
    }
    if let Some(ix) = line.find("CPU cores:") {
        let v = line[ix + "CPU cores:".len()..].trim();
        if let Ok(n) = v.parse::<u32>() {
            if meta.cpu_cores != Some(n) {
                meta.cpu_cores = Some(n);
                changed = true;
            }
        }
    }

    changed
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

    // Emit initial meta snapshot with known context
    let _ = app.emit(
        "miner:meta",
        &MinerMeta {
            binary: Some(cfg.binary_path.clone()),
            chain: Some(cfg.chain.clone()),
            rewards_address: Some(acct.address.clone()),
            ..Default::default()
        },
    );

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
        let mut meta = MinerMeta::default();
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
                    line: line.clone(),
                },
            );

            // Update and emit miner meta if this line contains interesting info.
            if update_meta_from_line(&mut meta, &line) {
                let _ = app_clone.emit("miner:meta", &meta);
            }

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

    // spawn a background task that periodically queries the local node JSON-RPC
    spawn_status_task(app.clone());
    *MINER.lock().await = Some(child);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct MinerStatus {
    peers: Option<u32>,
    current_block: Option<u64>,
    highest_block: Option<u64>,
    is_syncing: Option<bool>,
}

/// Attempt to parse a u64 from a JSON value that may be a number or a 0x-prefixed hex string.
fn parse_u64_from_json(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => {
            let s = s.trim();
            if let Some(hex) = s.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).ok()
            } else {
                s.parse::<u64>().ok()
            }
        }
        _ => None,
    }
}

/// Query the local Substrate JSON-RPC (ws://127.0.0.1:9944) for health and sync state.
async fn query_local_node_status() -> Result<MinerStatus> {
    let url = "ws://127.0.0.1:9944";
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| anyhow!("ws connect: {e}"))?;

    // Prepare requests
    let req_health = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "system_health",
        "params": []
    });
    let req_sync = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "system_syncState",
        "params": []
    });

    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    ws.send(Message::Text(req_health.to_string()))
        .await
        .map_err(|e| anyhow!("ws send health: {e}"))?;
    ws.send(Message::Text(req_sync.to_string()))
        .await
        .map_err(|e| anyhow!("ws send syncState: {e}"))?;

    let mut peers: Option<u32> = None;
    let mut is_syncing: Option<bool> = None;
    let mut current_block: Option<u64> = None;
    let mut highest_block: Option<u64> = None;

    // Collect responses with a timeout
    let _ = tokio::time::timeout(Duration::from_millis(1500), async {
        while peers.is_none()
            || current_block.is_none()
            || highest_block.is_none()
            || is_syncing.is_none()
        {
            match ws.next().await {
                Some(Ok(Message::Text(txt))) => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                        let id = val.get("id").and_then(|x| x.as_i64()).unwrap_or_default();
                        let res = val
                            .get("result")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        if id == 1 {
                            // system_health
                            peers = res.get("peers").and_then(|x| x.as_u64()).map(|x| x as u32);
                            is_syncing = res.get("isSyncing").and_then(|x| x.as_bool());
                        } else if id == 2 {
                            // system_syncState
                            current_block = res.get("currentBlock").and_then(parse_u64_from_json);
                            highest_block = res.get("highestBlock").and_then(parse_u64_from_json);
                        }
                    }
                }
                Some(Ok(_)) => continue,
                Some(Err(_e)) => break,
                None => break,
            }
        }
    })
    .await;

    Ok(MinerStatus {
        peers,
        current_block,
        highest_block,
        is_syncing,
    })
}

/// Spawn a repeating background task that emits "miner:status" with peer and height info.
/// This runs independently of the miner process; if the node is not up yet, it will emit
/// empty fields until it can connect.
fn spawn_status_task(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        loop {
            // 1) Query local node for current/height/peers
            let mut snap = match query_local_node_status().await {
                Ok(s) => s,
                Err(_) => MinerStatus {
                    peers: None,
                    current_block: None,
                    highest_block: None,
                    is_syncing: None,
                },
            };

            // 2) If we know which chain we're on, consult the bootnode for the highest height
            //    and merge that into our view so progress can be computed accurately.
            let chain_ui = { LAST_CFG.lock().await.as_ref().map(|c| c.chain.clone()) };
            if let Some(chain_name) = chain_ui {
                if let Some(ws_url) = crate::rpc::bootnode_ws_for_chain(chain_name.as_str()) {
                    if let Ok((mut ws, _)) = tokio_tungstenite::connect_async(ws_url).await {
                        let req_sync = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": 42,
                            "method": "system_syncState",
                            "params": []
                        });
                        // best-effort request
                        let _ = ws.send(Message::Text(req_sync.to_string())).await;

                        // read one response with a timeout
                        if let Ok(Some(Ok(Message::Text(txt)))) =
                            tokio::time::timeout(Duration::from_millis(1500), ws.next()).await
                        {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                                let res = val
                                    .get("result")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null);
                                if let Some(h) =
                                    res.get("highestBlock").and_then(|v| parse_u64_from_json(v))
                                {
                                    snap.highest_block = match snap.highest_block {
                                        Some(local_h) => Some(local_h.max(h)),
                                        None => Some(h),
                                    };
                                }
                            }
                        }
                    }
                }
            }

            // 3) Emit consolidated status snapshot for the UI
            let _ = app.emit("miner:status", &snap);

            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
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
