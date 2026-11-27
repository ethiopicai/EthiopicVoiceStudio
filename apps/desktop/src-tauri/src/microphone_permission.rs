//! Reuse macOS app authorization without prompting for each WebView recording.
//! This grants only the bundled main UI's microphone request; macOS TCC still
//! enforces its own authorization. Remote Home content remains sandboxed.
use tauri::webview::{PermissionKind, PermissionResponse};

fn trusted_ui(label: &str, url: Option<&tauri::Url>) -> bool {
    if label != "main" {
        return false;
    }
    let Some(url) = url else { return false };
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return cfg!(debug_assertions)
            && url.username().is_empty()
            && url.password().is_none()
            && url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
            && url.port() == Some(1420);
    }
    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"))
}

pub fn response(label: &str, url: Option<&tauri::Url>, kind: PermissionKind) -> PermissionResponse {
    match kind {
        PermissionKind::Microphone if trusted_ui(label, url) => PermissionResponse::Allow,
        PermissionKind::Microphone | PermissionKind::Camera => PermissionResponse::Deny,
        _ => PermissionResponse::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_bundled_main_ui_can_reuse_microphone_authorization() {
        for uri in ["tauri://localhost/", "http://tauri.localhost/"] {
            let url = tauri::Url::parse(uri).unwrap();
            assert!(matches!(
                response("main", Some(&url), PermissionKind::Microphone),
                PermissionResponse::Allow
            ));
            assert!(matches!(
                response("other", Some(&url), PermissionKind::Microphone),
                PermissionResponse::Deny
            ));
            assert!(matches!(
                response("main", Some(&url), PermissionKind::Camera),
                PermissionResponse::Deny
            ));
        }
        for uri in [
            "https://ethiopic.ai/",
            "https://tauri.localhost.evil/",
            "https://localhost/",
            "file:///tmp/index.html",
            "tauri://localhost:1234/",
            "tauri://user@localhost/",
        ] {
            let url = tauri::Url::parse(uri).unwrap();
            assert!(
                matches!(
                    response("main", Some(&url), PermissionKind::Microphone),
                    PermissionResponse::Deny
                ),
                "{uri}"
            );
        }
        assert!(matches!(
            response("main", None, PermissionKind::Microphone),
            PermissionResponse::Deny
        ));
    }
}
