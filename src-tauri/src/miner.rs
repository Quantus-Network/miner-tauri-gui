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

use crate::account::account_path;
use crate::parse::{parse_event, MinerEvent};

lazy_static! {
    static ref MINER: Mutex<Option<tokio::process::Child>> = Mutex::new(None);
}

#[derive(Debug, Clone, Serialize)]
pub struct MinerConfig {
    pub chain: String, // "resonance" | "heisenberg"
    pub binary_path: String,
    pub extra_args: Vec<String>,
}

pub async fn start(app: AppHandle, cfg: MinerConfig) -> Result<()> {
    // ensure previous child is stopped
    stop().await.ok();

    // TODO: choose flags your miner expects. Example layout:
    //   --chain {resonance|heisenberg}
    //   --account-file <path_to_mining-rewards-account.json>
    //   --telemetry-url wss://tc0.res.fm/feed  (optional)
    let acct = account_path(&app);
    let mut args = vec![
        "--chain".into(),
        cfg.chain.clone(),
        "--account-file".into(),
        acct.to_string_lossy().to_string(),
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
            let _ = app_clone.emit("miner:log", &line);
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
            let _ = app_clone.emit("miner:log", &format!("[err] {line}"));
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
