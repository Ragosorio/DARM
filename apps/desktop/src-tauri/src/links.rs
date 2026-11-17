//! URL policy for the reader's webview, and turning what the operating system
//! gives us (command-line arguments, `file://` URLs) into candidate paths.
//!
//! Everything here is a pure function so that it can be unit tested.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tauri::Url;

/// Whether `url` is the application itself, i.e. something the main webview
/// may navigate to. Everything else is refused: the reader never leaves its
/// own origin.
///
/// * macOS, Linux, iOS: the bundled front end is served from `tauri://localhost`.
/// * Windows, Android: WebView2 and the Android WebView cannot serve custom
///   schemes the same way, so Tauri uses `http://tauri.localhost` (or
///   `https://` when `useHttpsScheme` is set).
/// * `tauri dev` only: the Vite dev server origin from `build.devUrl`.
pub fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    let http_scheme_platform = cfg!(any(windows, target_os = "android"));
    match url.scheme() {
        "tauri" if !http_scheme_platform => url.host_str() == Some("localhost"),
        "http" | "https" if http_scheme_platform && url.host_str() == Some("tauri.localhost") => {
            true
        }
        "about" => matches!(url.path(), "blank" | "srcdoc"),
        _ => dev_url.is_some_and(|dev| dev.origin().is_tuple() && dev.origin() == url.origin()),
    }
}

/// Whether a link the reader asked to open in a new window may be handed to
/// the system (default browser or mail client).
pub fn is_external_url(url: &Url) -> bool {
    match url.scheme() {
        "https" | "http" => url.host_str().is_some_and(|h| !h.is_empty()),
        "mailto" => true,
        _ => false,
    }
}

/// Turns process arguments (without the program name) into candidate paths.
///
/// Flags (anything starting with `-`, e.g. macOS's legacy `-psn_…`) are
/// skipped, `file://` URLs are converted, and relative paths are resolved
/// against `cwd` when one is given (a second instance reports its own working
/// directory). Whether a candidate is acceptable is decided later by the registry.
pub fn paths_from_args<I, S>(args: I, cwd: Option<&Path>) -> Vec<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    args.into_iter()
        .filter_map(|arg| {
            let arg: OsString = arg.into();
            if arg.is_empty() {
                return None;
            }
            if let Some(text) = arg.to_str() {
                if text.starts_with('-') {
                    return None;
                }
                if text.len() >= 7 && text[..7].eq_ignore_ascii_case("file://") {
                    return path_from_file_url(&Url::parse(text).ok()?);
                }
            }
            let path = PathBuf::from(arg);
            Some(match cwd {
                Some(cwd) if path.is_relative() => cwd.join(path),
                _ => path,
            })
        })
        .collect()
}

/// `file://` URL to path; any other scheme (for instance Android's `content://`) is refused.
pub fn path_from_file_url(url: &Url) -> Option<PathBuf> {
    if url.scheme() != "file" {
        return None;
    }
    url.to_file_path().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn only_the_app_origin_may_be_navigated_to() {
        #[cfg(not(any(windows, target_os = "android")))]
        {
            assert!(is_app_url(&url("tauri://localhost/"), None));
            assert!(is_app_url(
                &url("tauri://localhost/index.html?book=x#p"),
                None
            ));
            assert!(!is_app_url(&url("tauri://evil/"), None));
            assert!(!is_app_url(&url("http://tauri.localhost/"), None));
        }
        #[cfg(any(windows, target_os = "android"))]
        {
            assert!(is_app_url(&url("http://tauri.localhost/"), None));
            assert!(is_app_url(&url("https://tauri.localhost/index.html"), None));
            assert!(!is_app_url(&url("tauri://localhost/"), None));
        }
        assert!(is_app_url(&url("about:blank"), None));
        for bad in [
            "https://example.com/",
            "http://localhost:5173/",
            "http://tauri.localhost.evil.example/",
            "file:///etc/passwd",
            "data:text/html,<script>1</script>",
            "javascript:alert(1)",
            "blob:tauri://localhost/1234",
            "about:config",
            "ipc://localhost/darm_read_chunk",
        ] {
            assert!(!is_app_url(&url(bad), None), "{bad} must not be navigable");
        }
    }

    #[test]
    fn the_dev_server_is_only_trusted_when_configured_and_only_its_exact_origin() {
        let dev = url("http://localhost:5173/");
        assert!(is_app_url(
            &url("http://localhost:5173/src/main.ts"),
            Some(&dev)
        ));
        assert!(!is_app_url(&url("http://localhost:5174/"), Some(&dev)));
        assert!(!is_app_url(&url("https://localhost:5173/"), Some(&dev)));
        assert!(!is_app_url(&url("http://127.0.0.1:5173/"), Some(&dev)));
        assert!(!is_app_url(&url("https://example.com/"), Some(&dev)));
        // An opaque origin never equals itself for this purpose.
        let odd = url("data:text/plain,x");
        assert!(!is_app_url(&odd, Some(&odd)));
    }

    #[test]
    fn external_links_are_limited_to_web_and_mail() {
        for ok in [
            "https://example.com/a?b#c",
            "http://example.com",
            "mailto:someone@example.com",
        ] {
            assert!(is_external_url(&url(ok)), "{ok}");
        }
        for bad in [
            "file:///Applications/Calculator.app",
            "javascript:alert(1)",
            "data:text/html,x",
            "tauri://localhost/",
            "ftp://example.com/",
            "smb://server/share",
            "tel:+123",
            "x-apple.systempreferences:com.apple.preference.security",
            "vbscript:x",
            "about:blank",
        ] {
            assert!(!is_external_url(&url(bad)), "{bad}");
        }
    }

    #[test]
    fn arguments_become_paths() {
        let cwd = Path::new(if cfg!(windows) { r"C:\work" } else { "/work" });
        let got = paths_from_args(
            ["-psn_0_12345", "--flag", "", "rel/a.darm", "b.darm"],
            Some(cwd),
        );
        assert_eq!(got, vec![cwd.join("rel/a.darm"), cwd.join("b.darm")]);

        // Without a working directory, relative paths are left for the OS to resolve.
        assert_eq!(
            paths_from_args(["a.darm"], None),
            vec![PathBuf::from("a.darm")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn absolute_paths_and_file_urls_are_kept() {
        let got = paths_from_args(
            [
                "/books/a.darm",
                "file:///books/b%20c.darm",
                "FILE:///books/d.darm",
                "https://example.com/e.darm",
            ],
            Some(Path::new("/work")),
        );
        assert_eq!(
            got,
            vec![
                PathBuf::from("/books/a.darm"),
                PathBuf::from("/books/b c.darm"),
                PathBuf::from("/books/d.darm"),
                // Not a file URL: it is treated as a (nonexistent) relative path and the registry rejects it.
                PathBuf::from("/work/https://example.com/e.darm"),
            ]
        );
    }

    #[test]
    fn only_file_urls_convert_to_paths() {
        assert!(path_from_file_url(&url("content://com.android.providers/document/1")).is_none());
        assert!(path_from_file_url(&url("https://example.com/a.darm")).is_none());
        #[cfg(unix)]
        assert_eq!(
            path_from_file_url(&url("file:///tmp/a.darm")),
            Some(PathBuf::from("/tmp/a.darm"))
        );
    }
}
