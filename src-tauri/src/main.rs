#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account_cli;
mod account_path;
mod commands;
mod installer;
mod miner;
mod parse;
mod rpc;

use commands::*;
use tauri::{Manager, Runtime};

fn main() {
    tauri::Builder::default()
        //.plugin(tauri_plugin_shell::init())
        //.plugin(tauri_plugin_process::init())
        //.plugin(tauri_plugin_updater::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ensure_miner_and_account,
            start_miner,
            stop_miner,
            read_log_tail,
            query_balance,
            select_chain,
        ])
        /*
        .setup(|app| {
            // ensure app data dir exists
            let _ = account::ensure_app_dir(&app.handle());
            Ok(())
        })
        */
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
