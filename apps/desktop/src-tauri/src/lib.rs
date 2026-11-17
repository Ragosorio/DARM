//! Native shell for the DARM reader.
//!
//! The reader itself is the web application in `apps/reader`; this crate only
//! does what a web page cannot:
//!
//! * receives `.darm` files from the operating system (double-click, "Open
//!   with", command-line arguments, a second launch, File → Open…) and hands
//!   them to the webview through an opaque-token [`registry`];
//! * opens external links in the system browser instead of the webview;
//! * keeps the webview on the application's own origin.
//!
//! The webview is granted three commands and event listening, nothing else
//! (see `capabilities/main.json`): no file-system, shell or HTTP access.

mod links;
mod log;
mod registry;

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::log::{log, trace};
use crate::registry::{FileInfo, Registry};

/// Emitted (without a payload that matters) when files arrive while the app is
/// running. The webview answers by calling `darm_take_pending`.
pub const FILES_EVENT: &str = "darm://files";

/// Label of the reader window, as declared in `tauri.conf.json`.
const MAIN_WINDOW: &str = "main";

// --- commands exposed to the webview -------------------------------------------------

/// Drains the queue of files handed to the app. Returns `{ token, name, size }` per file.
#[tauri::command]
fn darm_take_pending(registry: State<'_, Registry>) -> Vec<FileInfo> {
    let files = registry.take_pending();
    trace(|| format!("take_pending -> {} file(s)", files.len()));
    files
}

/// Returns `len` raw bytes at `offset` of a registered file as a binary IPC
/// response (an `ArrayBuffer` in JavaScript, not a JSON array of numbers).
///
/// Runs on the thread pool, not on the main thread: reading 8 MiB from a slow
/// disk must not stall the window.
#[tauri::command(async)]
fn darm_read_chunk(
    registry: State<'_, Registry>,
    token: String,
    offset: u64,
    len: u64,
) -> Result<tauri::ipc::Response, String> {
    match registry.read_chunk(&token, offset, len) {
        Ok(bytes) => {
            trace(|| {
                format!(
                    "read_chunk offset={offset} len={len} -> {} bytes",
                    bytes.len()
                )
            });
            Ok(tauri::ipc::Response::new(bytes))
        }
        Err(e) => {
            trace(|| format!("read_chunk offset={offset} len={len} -> refused: {e}"));
            Err(e.to_string())
        }
    }
}

/// Forgets a token and closes its file.
#[tauri::command]
fn darm_release(registry: State<'_, Registry>, token: String) -> bool {
    let known = registry.release(&token);
    trace(|| {
        format!(
            "release -> {} ({} file(s) still held)",
            if known { "released" } else { "unknown token" },
            registry.len()
        )
    });
    known
}

// --- files coming in ------------------------------------------------------------------

/// Registers candidate paths and, if any was accepted, wakes the webview and
/// brings the window forward. `source` is only used for the log.
fn accept_paths(app: &AppHandle, paths: impl IntoIterator<Item = PathBuf>, source: &str) {
    let registry = app.state::<Registry>();
    let mut accepted = 0usize;
    for path in paths {
        match registry.register(&path) {
            Ok(info) => {
                accepted += 1;
                log(format!(
                    "accepted from {source}: {} ({} bytes)",
                    path.display(),
                    info.size
                ));
            }
            Err(e) => log(format!("rejected from {source}: {}: {e}", path.display())),
        }
    }
    if accepted > 0 {
        if let Err(e) = app.emit(FILES_EVENT, accepted) {
            log(format!("could not notify the reader: {e}"));
        }
        focus_main_window(app);
    }
}

fn focus_main_window(app: &AppHandle) {
    #[cfg(desktop)]
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    #[cfg(not(desktop))]
    let _ = app;
}

// --- window ---------------------------------------------------------------------------

/// Creates the reader window from its `tauri.conf.json` entry (which has
/// `"create": false`) so that navigation and `window.open` can be policed.
fn create_main_window(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .cloned()
        .ok_or("tauri.conf.json does not declare the \"main\" window")?;

    // Only `tauri dev` loads the front end from the Vite dev server.
    let dev_url = if tauri::is_dev() {
        app.config().build.dev_url.clone()
    } else {
        None
    };

    let builder =
        tauri::WebviewWindowBuilder::from_config(app, &config)?.on_navigation(move |url| {
            let allowed = links::is_app_url(url, dev_url.as_ref());
            if !allowed {
                log(format!(
                    "blocked navigation away from the reader ({} URL)",
                    url.scheme()
                ));
            }
            allowed
        });

    // `window.open(href, "_blank", …)`: the reader calls it after its own
    // confirmation dialog. Never create a second webview; hand web and mail
    // links to the system and drop everything else.
    #[cfg(desktop)]
    let builder = {
        let app = app.clone();
        builder.on_new_window(move |url, _features| {
            open_external(&app, &url);
            tauri::webview::NewWindowResponse::Deny
        })
    };

    builder.build()?;
    Ok(())
}

#[cfg(desktop)]
fn open_external(app: &AppHandle, url: &tauri::Url) {
    use tauri_plugin_opener::OpenerExt;

    if !links::is_external_url(url) {
        log(format!(
            "refused to open a {} link outside the reader",
            url.scheme()
        ));
        return;
    }
    match app.opener().open_url(url.as_str(), None::<&str>) {
        Ok(()) => log(format!(
            "opened a {} link to {} in the system browser",
            url.scheme(),
            url.host_str().unwrap_or("(no host)")
        )),
        Err(e) => log(format!("could not open a {} link: {e}", url.scheme())),
    }
}

// --- menu (desktop) -------------------------------------------------------------------

#[cfg(desktop)]
const MENU_OPEN: &str = "darm-open";

#[cfg(desktop)]
fn build_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

    let open = MenuItemBuilder::with_id(MENU_OPEN, "Open…")
        .accelerator("CmdOrCtrl+O")
        .build(app)?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&open)
        .separator()
        .close_window();
    #[cfg(not(target_os = "macos"))]
    let file = file.quit();
    let file = file.build()?;

    // The standard Edit items are what make Cmd/Ctrl+C, V, X, A and Z work in
    // the reader's text fields (search, notes, pass phrases) on macOS.
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let window = SubmenuBuilder::new(app, "Window").minimize().maximize();
    #[cfg(target_os = "macos")]
    let window = window.separator().fullscreen();
    let window = window.build()?;

    let menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    let app_menu = SubmenuBuilder::new(app, "DARM")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    #[cfg(target_os = "macos")]
    let menu = menu.item(&app_menu);

    menu.items(&[&file, &edit, &window]).build()
}

/// File → Open…: a native dialog filtered to `.darm`. What the user picks goes
/// through the same registry as files handed over by the operating system.
#[cfg(desktop)]
fn open_dialog(app: &AppHandle) {
    use tauri_plugin_dialog::DialogExt;

    let handle = app.clone();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Open a DARM book")
        .add_filter("DARM books", &[registry::EXTENSION]);
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_files(move |picked| {
        let paths = picked
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.into_path().ok());
        accept_paths(&handle, paths, "the Open dialog");
    });
}

// --- entry point ----------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // Must be the first plugin: a second launch stops here, forwards its
    // arguments to the running instance and exits.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
        let paths =
            links::paths_from_args(argv.into_iter().skip(1), Some(std::path::Path::new(&cwd)));
        log(format!(
            "a second launch forwarded {} candidate path(s)",
            paths.len()
        ));
        accept_paths(app, paths, "a second launch");
        focus_main_window(app);
    }));

    #[cfg(desktop)]
    let builder = builder.menu(build_menu).on_menu_event(|app, event| {
        if event.id().as_ref() == MENU_OPEN {
            open_dialog(app);
        }
    });

    let app = builder
        .plugin(tauri_plugin_dialog::init())
        // The plugin's click interceptor is off: links are only opened from
        // Rust, after the reader's own confirmation dialog (see `open_external`).
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .manage(Registry::new())
        .invoke_handler(tauri::generate_handler![
            darm_take_pending,
            darm_read_chunk,
            darm_release
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            log(format!(
                "DARM shell {} starting (tauri {})",
                app.package_info().version,
                tauri::VERSION
            ));
            // Windows and Linux pass the file as an argument; so does
            // `DARM.app/Contents/MacOS/darm-desktop book.darm` on macOS.
            accept_paths(
                &handle,
                links::paths_from_args(std::env::args_os().skip(1), None),
                "the command line",
            );
            create_main_window(&handle)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the DARM shell");

    app.run(|app, event| match event {
        // macOS and iOS (Finder, "Open with", `open -a`, the share sheet) and
        // Android (intent filters) deliver files as URLs, possibly before the
        // window exists and at any time afterwards.
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        tauri::RunEvent::Opened { urls } => {
            let mut paths = Vec::new();
            for url in &urls {
                match links::path_from_file_url(url) {
                    Some(path) => paths.push(path),
                    // Android hands out `content://` URIs, which are not paths:
                    // see docs/platforms.md, "Android".
                    None => log(format!(
                        "ignored an opened {} URL: only file URLs are supported",
                        url.scheme()
                    )),
                }
            }
            accept_paths(app, paths, "the operating system");
        }
        _ => {
            let _ = app;
        }
    });
}
