// Declares the app's own commands to Tauri's access-control system.
//
// Without this manifest every `#[tauri::command]` of the application would be
// callable from any window. With it, each command gets an `allow-<command>`
// permission that a capability file has to grant explicitly
// (see `capabilities/main.json`).
fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "darm_take_pending",
            "darm_read_chunk",
            "darm_release",
        ]),
    ))
    .expect("failed to run tauri-build");
}
