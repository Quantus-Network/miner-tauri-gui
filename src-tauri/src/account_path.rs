use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub fn account_json_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("app_data_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("mining-rewards-account.json")
}
