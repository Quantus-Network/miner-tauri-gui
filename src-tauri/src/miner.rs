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
    // When true, we are currently running with '--max-blocks-per-request 1'
    static ref SAFE_MODE_ACTIVE: Mutex<bool> = Mutex::new(false);
    // A pending request to enable/disable safe mode detected by the stderr reader.
    static ref SAFE_MODE_PENDING: Mutex<Option<bool>> = Mutex::new(None);
}

// Troublesome block ranges per chain. For Resonance: heavy blocks around 13311..=13360.
const RESONANCE_TROUBLESOME_RANGES: &[(u64, u64)] = &[(13311, 13360)];

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
    pub log_to_file: bool,
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

    let bin_path = cfg.binary_path.clone();
    let mut cmd = Command::new(&bin_path);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| anyhow!("spawn miner: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Prepare optional file logger
    let mut log_file: Option<std::fs::File> = None;
    if cfg.log_to_file {
        if let Some(mut p) = dirs::data_local_dir() {
            // Use an app-specific log dir
            p.push("quantus-miner");
            p.push("logs");
            let _ = std::fs::create_dir_all(&p);
            // Include PID in filename
            let pid = child.id().unwrap_or(0);
            let ts = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "now".into())
                .replace(':', "-");
            let fname = format!("miner-{}-{}.log", pid, ts);
            p.push(fname);
            if let Ok(f) = std::fs::File::create(&p) {
                log_file = Some(f);
                // Inform UI of logfile path
                let _ = app.emit(
                    "miner:log",
                    &LogMsg {
                        source: "ui",
                        line: format!("Logging to file: {}", p.display()),
                    },
                );
                let _ = app.emit(
                    "miner:logfile",
                    &serde_json::json!({ "path": p.display().to_string() }),
                );
            }
        }
    }

    // Emit initial meta snapshot with known context
    let _ = app.emit(
        "miner:meta",
        &MinerMeta {
            binary: Some(bin_path.clone()),
            chain: Some(cfg.chain.clone()),
            rewards_address: Some(acct.address.clone()),
            ..Default::default()
        },
    );

    let app_clone = app.clone();
    // Clone a file handle for stdout task if enabled
    let log_file_stdout = log_file.as_ref().and_then(|f| f.try_clone().ok());
    tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        let mut file = log_file_stdout;
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(ev) = parse_event(&line) {
                let _ = app_clone.emit("miner:event", &ev);
            }
            // write to file if enabled
            if let Some(ref mut fh) = file {
                use std::io::Write;
                let _ = writeln!(fh, "{}", line);
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
    // Clone a file handle for stderr task if enabled
    let log_file_stderr = log_file.as_ref().and_then(|f| f.try_clone().ok());
    tauri::async_runtime::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        let mut meta = MinerMeta::default();
        let mut file = log_file_stderr;
        while let Ok(Some(line)) = reader.next_line().await {
            // surface stderr as logs; parse too (some miners log success to stderr)
            if let Some(ev) = parse_event(&line) {
                let _ = app_clone.emit("miner:event", &ev);
            }
            // write to file if enabled
            if let Some(ref mut fh) = file {
                use std::io::Write;
                let _ = writeln!(fh, "{}", line);
            }
            let low = line.to_lowercase();
            let _ = app_clone.emit(
                "miner:log",
                &LogMsg {
                    source: "stderr",
                    line: line.clone(),
                },
            );

            // Safe sync mode automation:
            // Detect current importing block and set a pending toggle for safe mode.
            // The actual restart/toggle is performed in the status task to keep this future Send-friendly.
            if let Some(pos) = low.find("importing block #") {
                let after = &low[pos + "importing block #".len()..];
                let mut num_str = String::new();
                for ch in after.chars() {
                    if ch.is_ascii_digit() {
                        num_str.push(ch);
                    } else {
                        break;
                    }
                }
                if let Ok(cur_block) = num_str.parse::<u64>() {
                    // Determine chain to select applicable ranges
                    let chain_ui = { LAST_CFG.lock().await.as_ref().map(|c| c.chain.clone()) };
                    if let Some(chain_name) = chain_ui {
                        let ranges: &[(u64, u64)] = match chain_name.as_str() {
                            "resonance" => RESONANCE_TROUBLESOME_RANGES,
                            _ => &[],
                        };
                        // Are we inside any troublesome range?
                        let in_range = ranges
                            .iter()
                            .any(|(s, e)| cur_block >= *s && cur_block <= *e);
                        let past_all = ranges.iter().all(|(_, e)| cur_block > *e);
                        let approaching = ranges.iter().any(|(s, _)| cur_block >= *s);

                        let active_now = { *SAFE_MODE_ACTIVE.lock().await };
                        // Request enable when approaching/in-range and not yet active
                        if !active_now && (in_range || approaching) {
                            let mut pend = SAFE_MODE_PENDING.lock().await;
                            *pend = Some(true);
                            let _ = app_clone.emit(
                                "miner:log",
                                &LogMsg {
                                    source: "ui",
                                    line: format!("Approaching heavy blocks at #{cur_block}. Scheduling safe sync enable (--max-blocks-per-request 1)..."),
                                },
                            );
                        // Request disable when past all ranges and currently active
                        } else if active_now && past_all {
                            let mut pend = SAFE_MODE_PENDING.lock().await;
                            *pend = Some(false);
                            let _ = app_clone.emit(
                                "miner:log",
                                &LogMsg {
                                    source: "ui",
                                    line: format!("Past heavy block range(s) at #{cur_block}. Scheduling safe sync disable..."),
                                },
                            );
                        }
                    }
                }
            }

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

/// Spawn a repeating background task that emits "miner:status" with peer and height info,
/// and performs pending safe-mode toggles requested by the stderr task.
/// This runs independently of the miner process; if the node is not up yet, it will emit
/// empty fields until it can connect.
fn spawn_status_task(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let mut best: Option<u64> = None;
        let mut highest: Option<u64> = None;
        let mut peers: Option<u32> = None;
        let mut is_syncing: Option<bool> = None;

        // Keep a WS connection + subscription to local node heads; periodically poll health
        let mut sub_id: Option<String> = None;
        let mut ws_opt: Option<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        > = None;
        let mut tick: u32 = 0;

        loop {
            // Handle any pending safe-mode toggle (set by stderr reader)
            if let Some(pending) = { SAFE_MODE_PENDING.lock().await.take() } {
                // Perform toggle here (this future runs under tauri async spawn and is Send)
                let _ = set_safe_mode(app.clone(), pending).await;
            }

            // Ensure WS connection to local node JSON-RPC
            if ws_opt.is_none() {
                if let Ok((ws, _)) =
                    tokio_tungstenite::connect_async(crate::rpc::local_ws_endpoint()).await
                {
                    ws_opt = Some(ws);
                    sub_id = None;
                } else {
                    // Emit whatever we have and retry shortly
                    let _ = app.emit(
                        "miner:status",
                        &MinerStatus {
                            peers,
                            current_block: best,
                            highest_block: highest,
                            is_syncing,
                        },
                    );
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    continue;
                }
            }

            let ws = ws_opt.as_mut().unwrap();

            // Ensure subscription to new heads
            if sub_id.is_none() {
                let req = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1001,
                    "method": "chain_subscribeNewHeads",
                    "params": []
                });
                if ws.send(Message::Text(req.to_string())).await.is_err() {
                    ws_opt = None;
                    continue;
                }
                // Wait for subscription id
                if let Ok(Some(Ok(Message::Text(txt)))) =
                    tokio::time::timeout(Duration::from_millis(1500), ws.next()).await
                {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                        if let Some(idv) = val.get("result") {
                            if let Some(s) = idv.as_str() {
                                sub_id = Some(s.to_string());
                            }
                        }
                    }
                }
            }

            // Read one message with a small timeout; update best height on new head
            let mut got_update = false;
            if let Ok(Some(msg)) = tokio::time::timeout(Duration::from_millis(400), ws.next()).await
            {
                match msg {
                    Ok(Message::Text(txt)) => {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                            // Subscription notification carries "params.result"
                            let head = val
                                .get("params")
                                .and_then(|p| p.get("result"))
                                .cloned()
                                .unwrap_or_else(|| {
                                    val.get("result")
                                        .cloned()
                                        .unwrap_or(serde_json::Value::Null)
                                });
                            if let Some(numv) = head.get("number") {
                                if let Some(n) = parse_u64_from_json(numv) {
                                    if best != Some(n) {
                                        best = Some(n);
                                        got_update = true;
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // Connection dropped; reconnect
                        ws_opt = None;
                        continue;
                    }
                }
            }

            // Periodic health polling (peers, isSyncing)
            tick = tick.wrapping_add(1);
            if tick % 5 == 0 {
                let req_health = serde_json::json!({
                    "jsonrpc":"2.0","id":2001,"method":"system_health","params":[]
                });
                let _ = ws.send(Message::Text(req_health.to_string())).await;
                if let Ok(Some(Ok(Message::Text(txt)))) =
                    tokio::time::timeout(Duration::from_millis(600), ws.next()).await
                {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                        if let Some(res) = val.get("result") {
                            if let Some(p) = res.get("peers").and_then(|x| x.as_u64()) {
                                let np = p as u32;
                                if peers != Some(np) {
                                    peers = Some(np);
                                    got_update = true;
                                }
                            }
                            if let Some(s) = res.get("isSyncing").and_then(|x| x.as_bool()) {
                                if is_syncing != Some(s) {
                                    is_syncing = Some(s);
                                    got_update = true;
                                }
                            }
                        }
                    }
                }
            }

            // Subscribe to bootnode heads to improve progress accuracy
            if tick % 10 == 0 {
                if let Some(chain_name) =
                    { LAST_CFG.lock().await.as_ref().map(|c| c.chain.clone()) }
                {
                    if let Some(url) = crate::rpc::bootnode_ws_for_chain(chain_name.as_str()) {
                        if let Ok((mut ws_b, _)) = tokio_tungstenite::connect_async(url).await {
                            // Start a short-lived subscription to new heads and read one notification.
                            let req = serde_json::json!({
                                "jsonrpc":"2.0","id":4242,"method":"chain_subscribeNewHeads","params":[]
                            });
                            let _ = ws_b.send(Message::Text(req.to_string())).await;

                            // First response is usually the subscription id; read it (best-effort).
                            if let Ok(Some(Ok(Message::Text(_txt1)))) =
                                tokio::time::timeout(Duration::from_millis(600), ws_b.next()).await
                            {
                                // Then read the first head notification.
                                if let Ok(Some(Ok(Message::Text(txt2)))) =
                                    tokio::time::timeout(Duration::from_millis(900), ws_b.next())
                                        .await
                                {
                                    if let Ok(val) =
                                        serde_json::from_str::<serde_json::Value>(&txt2)
                                    {
                                        if let Some(head) =
                                            val.get("params").and_then(|p| p.get("result"))
                                        {
                                            if let Some(num) =
                                                head.get("number").and_then(parse_u64_from_json)
                                            {
                                                let new_h = match highest {
                                                    Some(x) => Some(x.max(num)),
                                                    None => Some(num),
                                                };
                                                if new_h != highest {
                                                    highest = new_h;
                                                    got_update = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if got_update {
                let _ = app.emit(
                    "miner:status",
                    &MinerStatus {
                        peers,
                        current_block: best,
                        highest_block: highest,
                        is_syncing,
                    },
                );
            }
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

// Toggle safe mode by restarting with/without '--max-blocks-per-request 1'
async fn set_safe_mode(app: AppHandle, enable: bool) -> Result<()> {
    // Avoid redundant work
    {
        let active = SAFE_MODE_ACTIVE.lock().await.clone();
        if active == enable {
            return Ok(());
        }
    }

    // Read last cfg
    let mut cfg = {
        let lock = LAST_CFG.lock().await;
        lock.clone()
    }
    .ok_or_else(|| anyhow!("no previous miner configuration available"))?;

    // Adjust extra_args
    if enable {
        if !has_max_blocks_arg(&cfg.extra_args) {
            cfg.extra_args.push("--max-blocks-per-request".into());
            cfg.extra_args.push("1".into());
        }
    } else {
        remove_max_blocks_arg(&mut cfg.extra_args);
    }

    // Stop and restart
    let _ = stop().await;
    start(app.clone(), cfg).await?;
    // Mark state
    {
        let mut active = SAFE_MODE_ACTIVE.lock().await;
        *active = enable;
    }
    Ok(())
}

fn has_max_blocks_arg(args: &Vec<String>) -> bool {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--max-blocks-per-request" {
            return true;
        }
        i += 1;
    }
    false
}

fn remove_max_blocks_arg(args: &mut Vec<String>) {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--max-blocks-per-request" {
            // remove flag and possible value
            args.remove(i);
            if i < args.len() && args[i] != "--" {
                // best-effort: remove next token (value)
                args.remove(i);
            }
            continue;
        }
        i += 1;
    }
}

pub async fn unlock_and_restart(app: AppHandle) -> Result<()> {
    // Use last known configuration (same approach as repair_and_restart)
    let cfg = { LAST_CFG.lock().await.clone() }
        .ok_or_else(|| anyhow!("no previous miner configuration available"))?;

    let chain_id = chain_id_for_ui(&cfg.chain);
    let lock_path = node_base_path()?
        .join("chains")
        .join(chain_id)
        .join("db")
        .join("full")
        .join("LOCK");

    let _ = app.emit(
        "miner:log",
        &LogMsg {
            source: "ui",
            line: format!(
                "Unlock requested. Stopping node and removing lock file at {} (will restart from current state).",
                lock_path.display()
            ),
        },
    );

    // Stop first to avoid races while touching the lock file
    let _ = stop().await;

    if lock_path.exists() {
        std::fs::remove_file(&lock_path)
            .map_err(|e| anyhow!("failed to remove LOCK at {}: {e}", lock_path.display()))?;
        let _ = app.emit(
            "miner:log",
            &LogMsg {
                source: "ui",
                line: "LOCK file removed. Restarting node...".into(),
            },
        );
    } else {
        let _ = app.emit(
            "miner:log",
            &LogMsg {
                source: "ui",
                line: format!(
                    "No LOCK file found at {}. Restarting node anyway...",
                    lock_path.display()
                ),
            },
        );
    }

    start(app, cfg).await
}
