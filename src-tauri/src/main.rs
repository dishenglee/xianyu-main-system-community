#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The real entry point lives in `lib.rs` so the mobile (Android) build can
// reuse it via Tauri's `#[mobile_entry_point]` macro.
fn main() {
    xianyu_tauri_app::run();
}
