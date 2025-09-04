#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account_cli;
mod account_path;
mod commands;
mod installer;
mod miner;
mod parse;
mod rpc;

use commands::*;
use tauri::{LogicalSize, Manager, Runtime, Size};

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
            repair_miner,
            unlock_miner,
            get_safe_ranges,
            set_safe_ranges,
        ])
        .setup(|app| {
            if let Some(win) = app.get_webview_window("main") {
                // Try to size to 90% of the primary monitor; fallback to a large default.
                if let Ok(Some(monitor)) = app.primary_monitor() {
                    let size = monitor.size();
                    let w = (size.width as f64 * 0.9).max(800.0);
                    let h = (size.height as f64 * 0.9).max(600.0);
                    let _ = win.set_size(Size::Logical(LogicalSize::new(w, h)));
                    let _ = win.center();
                } else {
                    let _ = win.set_size(Size::Logical(LogicalSize::new(1728.0, 1080.0)));
                    let _ = win.center();
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
