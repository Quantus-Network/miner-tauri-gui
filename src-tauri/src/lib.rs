// src-tauri/src/lib.rs
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // No-op for desktop; required so Cargo finds the library.
    // You can later refactor main.rs to call into here if you want.
}
