use std::{fs, path::Path};

fn main() {
    emit_static_asset_rerun_hints(Path::new("../static"));
    tauri_build::build()
}
fn emit_static_asset_rerun_hints(directory: &Path) {
    println!("cargo:rerun-if-changed={}", directory.display());

    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            emit_static_asset_rerun_hints(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}
