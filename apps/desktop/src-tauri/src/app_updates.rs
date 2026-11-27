//! Notification-only update discovery. No installer execution or resource updates.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

const ENDPOINT: &str = "https://ethiopic.ai/studio/version";
const CHANNEL: &str = "preview";
const DAY: u64 = 86_400;
const RETRY: u64 = DAY;
const MAX_BYTES: u64 = 65_536;
const SAVE_ERROR: &str = "Update settings could not be saved. Please try again.";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    installation_id: String,
    automatic_checks: bool,
    usage_statistics: bool,
    last_checked_at: Option<u64>,
    last_successful_check_at: Option<u64>,
    dismissed_version: Option<String>,
    release: Option<Release>,
}
impl Saved {
    fn fresh() -> Self {
        Self {
            installation_id: uuid::Uuid::new_v4().to_string(),
            automatic_checks: true,
            usage_statistics: true,
            last_checked_at: None,
            last_successful_check_at: None,
            dismissed_version: None,
            release: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    version: String,
    notes: String,
    url: String,
}
#[derive(Deserialize)]
struct Feed {
    version: String,
    channel: String,
    #[serde(default)]
    notes: String,
    platforms: BTreeMap<String, Platform>,
}
#[derive(Deserialize)]
struct Platform {
    url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current_version: &'static str,
    automatic_checks: bool,
    usage_statistics: bool,
    last_checked_at: Option<u64>,
    last_successful_check_at: Option<u64>,
    checking: bool,
    available_update: Option<Release>,
    show_notification: bool,
    error: Option<String>,
}
struct Inner {
    saved: Saved,
    checking: bool,
    error: Option<String>,
}
pub struct UpdateService {
    root: PathBuf,
    inner: Mutex<Option<Inner>>,
}
impl UpdateService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Mutex::new(None),
        }
    }
    fn locked(&self) -> Result<std::sync::MutexGuard<'_, Option<Inner>>, String> {
        let mut guard = self.inner.lock().map_err(|_| SAVE_ERROR)?;
        if guard.is_none() {
            let path = self.root.join("app-updates.json");
            let saved = match fs::read(path) {
                Ok(bytes) => {
                    let saved: Saved = serde_json::from_slice(&bytes).map_err(|_| "Update settings could not be read. Your privacy preferences have not been changed.")?;
                    uuid::Uuid::parse_str(&saved.installation_id)
                        .map_err(|_| "The saved installation ID is invalid.")?;
                    saved
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let saved = Saved::fresh();
                    self.persist(&saved)?;
                    saved
                }
                Err(_) => return Err("Update settings could not be read.".into()),
            };
            *guard = Some(Inner {
                saved,
                checking: false,
                error: None,
            });
        }
        Ok(guard)
    }
    fn persist(&self, saved: &Saved) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|_| SAVE_ERROR)?;
        let path = self.root.join("app-updates.json.pending");
        let result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path)?;
            file.write_all(&serde_json::to_vec(saved)?)?;
            file.sync_all()?;
            fs::rename(&path, self.root.join("app-updates.json"))
        })();
        result.map_err(|_| SAVE_ERROR.into())
    }
    fn status(&self) -> Result<UpdateStatus, String> {
        let guard = self.locked()?;
        Ok(snapshot(guard.as_ref().unwrap()))
    }
    fn preferences(&self, automatic: bool, usage: bool) -> Result<UpdateStatus, String> {
        let mut guard = self.locked()?;
        let inner = guard.as_mut().unwrap();
        let mut next = inner.saved.clone();
        next.automatic_checks = automatic;
        next.usage_statistics = usage;
        self.persist(&next)?;
        inner.saved = next;
        Ok(snapshot(inner))
    }
    fn dismiss(&self) -> Result<UpdateStatus, String> {
        let mut guard = self.locked()?;
        let inner = guard.as_mut().unwrap();
        let mut next = inner.saved.clone();
        next.dismissed_version = next.release.as_ref().map(|r| r.version.clone());
        self.persist(&next)?;
        inner.saved = next;
        Ok(snapshot(inner))
    }
    fn check_with(
        &self,
        manual: bool,
        now: u64,
        fetch: impl FnOnce(&Saved) -> Result<Option<Release>, String>,
    ) -> Result<UpdateStatus, String> {
        let saved = {
            let mut guard = self.locked()?;
            let inner = guard.as_mut().unwrap();
            if inner.checking || (!manual && !due(&inner.saved, now)) {
                return Ok(snapshot(inner));
            }
            let mut next = inner.saved.clone();
            next.last_checked_at = Some(now);
            self.persist(&next)?;
            inner.saved = next.clone();
            inner.checking = true;
            inner.error = None;
            next
        };
        // No lock while waiting for the network: settings and offline work remain responsive.
        let result = fetch(&saved);
        let mut guard = self.locked()?;
        let inner = guard.as_mut().unwrap();
        inner.checking = false;
        match result {
            Ok(release) => {
                let mut next = inner.saved.clone();
                next.release = release;
                next.last_successful_check_at = Some(now);
                if let Err(error) = self.persist(&next) {
                    inner.error = Some(error);
                } else {
                    inner.saved = next;
                    inner.error = None;
                }
            }
            Err(error) => inner.error = Some(error), // Preserve the last valid release when offline.
        }
        Ok(snapshot(inner))
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn target() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    format!("{os}-{}", std::env::consts::ARCH)
}
fn due(saved: &Saved, now: u64) -> bool {
    if !saved.automatic_checks {
        return false;
    }
    let Some(last) = saved.last_checked_at else {
        return true;
    };
    let interval = if saved.last_successful_check_at == Some(last) {
        DAY
    } else {
        RETRY
    };
    // Clock corrections should not disable checking indefinitely.
    now < last || now.saturating_sub(last) >= interval
}
fn newer(version: &str, current: &str) -> bool {
    match (
        semver::Version::parse(version),
        semver::Version::parse(current),
    ) {
        (Ok(version), Ok(current)) => version.cmp_precedence(&current).is_gt(),
        _ => false,
    }
}
fn safe_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("ethiopic.ai")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.fragment().is_none()
        && url.query().is_none()
        && (url.path() == "/studio/" || url.path().starts_with("/studio/"))
}
fn parse_feed(bytes: &[u8], platform: &str, current: &str) -> Result<Option<Release>, String> {
    let invalid =
        || "The update service returned an invalid release. Please try again later.".to_owned();
    if bytes.len() > MAX_BYTES as usize {
        return Err(invalid());
    }
    let feed: Feed = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    semver::Version::parse(&feed.version).map_err(|_| invalid())?;
    if feed.channel != CHANNEL || feed.notes.len() > 8_192 {
        return Err(invalid());
    }
    let Some(platform) = feed.platforms.get(platform) else {
        return Ok(None);
    };
    if !safe_url(&platform.url) {
        return Err(invalid());
    }
    Ok(newer(&feed.version, current).then_some(Release {
        version: feed.version,
        notes: feed.notes,
        url: platform.url.clone(),
    }))
}
fn snapshot(inner: &Inner) -> UpdateStatus {
    let release = inner
        .saved
        .release
        .clone()
        .filter(|r| newer(&r.version, env!("CARGO_PKG_VERSION")) && safe_url(&r.url));
    let show = release
        .as_ref()
        .is_some_and(|r| Some(&r.version) != inner.saved.dismissed_version.as_ref());
    UpdateStatus {
        current_version: env!("CARGO_PKG_VERSION"),
        automatic_checks: inner.saved.automatic_checks,
        usage_statistics: inner.saved.usage_statistics,
        last_checked_at: inner.saved.last_checked_at,
        last_successful_check_at: inner.saved.last_successful_check_at,
        checking: inner.checking,
        available_update: release,
        show_notification: show,
        error: inner.error.clone(),
    }
}
fn request_headers(saved: &Saved) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-studio-version",
        HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    headers.insert("x-studio-channel", HeaderValue::from_static(CHANNEL));
    headers.insert(
        "x-studio-platform",
        HeaderValue::from_str(&target()).unwrap(),
    );
    if saved.usage_statistics {
        if let Ok(id) = HeaderValue::from_str(&saved.installation_id) {
            headers.insert("x-studio-installation-id", id);
        }
    }
    headers
}
fn fetch(saved: &Saved) -> Result<Option<Release>, String> {
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(10))
        .user_agent("EthiopicVoiceStudio/update-check")
        .build()
        .map_err(|_| "Update checks are temporarily unavailable.")?;
    let response = client
        .get(ENDPOINT)
        .headers(request_headers(saved))
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache")
        .send()
        .map_err(|_| "Could not check for updates. Connect to the internet and try again.")?;
    if !response.status().is_success() {
        return Err("The update service is unavailable. Please try again later.".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES)
    {
        return Err("The update response was too large.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The update check was interrupted.")?;
    parse_feed(&bytes, &target(), env!("CARGO_PKG_VERSION"))
}
#[tauri::command]
pub fn get_app_update_status(state: State<'_, UpdateService>) -> Result<UpdateStatus, String> {
    state.status()
}
#[tauri::command]
pub async fn check_app_update(app: tauri::AppHandle, manual: bool) -> Result<UpdateStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<UpdateService>()
            .check_with(manual, now(), fetch)
    })
    .await
    .map_err(|_| "Update checks are temporarily unavailable.".to_owned())?
}
#[tauri::command]
pub fn set_app_update_preferences(
    state: State<'_, UpdateService>,
    automatic_checks: bool,
    usage_statistics: bool,
) -> Result<UpdateStatus, String> {
    state.preferences(automatic_checks, usage_statistics)
}
#[tauri::command]
pub fn dismiss_app_update(state: State<'_, UpdateService>) -> Result<UpdateStatus, String> {
    state.dismiss()
}
#[tauri::command]
pub async fn open_app_update(app: tauri::AppHandle) -> Result<(), String> {
    // Never accept a URL from the webview. Only open a validated, cached release page.
    let url = app
        .state::<UpdateService>()
        .status()?
        .available_update
        .ok_or("No update is available for this computer.")?
        .url;
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut c = crate::background_process::command("/usr/bin/open");
            c.arg(&url);
            c
        };
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut c = crate::background_process::command("rundll32.exe");
            c.args(["url.dll,FileProtocolHandler", &url]);
            c
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let mut command = {
            let mut c = crate::background_process::command("xdg-open");
            c.arg(&url);
            c
        };
        command
            .status()
            .map_err(|_| "The update page could not be opened.")?
            .success()
            .then_some(())
            .ok_or_else(|| "The update page could not be opened.".to_owned())
    })
    .await
    .map_err(|_| "The update page could not be opened.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> UpdateService {
        UpdateService::new(
            std::env::temp_dir().join(format!("studio-updates-{}", uuid::Uuid::new_v4())),
        )
    }
    fn release() -> Option<Release> {
        Some(Release {
            version: "9.0.0".into(),
            notes: "New controls".into(),
            url: "https://ethiopic.ai/studio/downloads/mac".into(),
        })
    }
    #[test]
    fn bundled_policy_matches_notification_service_without_enabling_resource_downloads() {
        let policy: serde_json::Value =
            serde_json::from_str(include_str!("../../resources/update-policy.json")).unwrap();
        assert_eq!(policy["app_update_endpoint"], ENDPOINT);
        assert_eq!(policy["check_interval_seconds"], DAY);
        assert_eq!(policy["app_notification_checks_enabled"], true);
        assert_eq!(policy["automatic_downloads_enabled"], false);
        assert_eq!(policy["automatic_checks_enabled"], false);
    }
    #[test]
    fn independent_installs_have_different_ids_and_send_only_minimum_headers() {
        let first = Saved::fresh();
        let second = Saved::fresh();
        assert_ne!(first.installation_id, second.installation_id);
        assert_eq!(
            uuid::Uuid::parse_str(&first.installation_id)
                .unwrap()
                .get_version_num(),
            4
        );
        let headers = request_headers(&first);
        assert_eq!(headers.len(), 4);
        assert!(headers.contains_key("x-studio-installation-id"));
        assert!(headers.contains_key("x-studio-platform"));
        assert!(headers.contains_key("x-studio-version"));
        assert!(headers.contains_key("x-studio-channel"));
    }
    #[test]
    fn semantic_versions_ignore_build_metadata_and_order_previews() {
        assert!(newer("0.1.0-rc.10", "0.1.0-rc.2"));
        assert!(newer("0.1.0", "0.1.0-rc.10"));
        assert!(!newer("0.1.0+new", "0.1.0+old"));
        assert!(!newer("0.1.0-rc.1", "0.1.0"));
        assert!(!newer("nonsense", "0.1.0"));
    }
    #[test]
    fn validates_channel_platform_and_first_party_destination() {
        let feed = |version: &str, channel: &str, url: &str| {
            serde_json::to_vec(&serde_json::json!({"version":version,"channel":channel,"notes":"A release","platforms":{"darwin-aarch64":{"url":url}}})).unwrap()
        };
        assert!(parse_feed(
            &feed("0.2.0", "preview", "https://ethiopic.ai/studio/"),
            "darwin-aarch64",
            "0.1.0"
        )
        .unwrap()
        .is_some());
        assert!(parse_feed(
            &feed("0.2.0", "preview", "https://ethiopic.ai/studio/"),
            "windows-x86_64",
            "0.1.0"
        )
        .unwrap()
        .is_none());
        assert!(parse_feed(
            &feed("0.0.1", "preview", "https://ethiopic.ai/studio/"),
            "darwin-aarch64",
            "0.1.0"
        )
        .unwrap()
        .is_none());
        assert!(parse_feed(
            &feed("0.2.0", "stable", "https://ethiopic.ai/studio/"),
            "darwin-aarch64",
            "0.1.0"
        )
        .is_err());
        for url in [
            "http://ethiopic.ai/studio/",
            "https://elsewhere.test/studio/",
            "https://ethiopic.ai.evil.test/studio/",
            "https://ethiopic.ai/studio/?secret=1",
            "https://user@ethiopic.ai/studio/",
            "javascript:alert(1)",
            "https://ethiopic.ai/studio/../evil",
        ] {
            assert!(
                parse_feed(&feed("0.2.0", "preview", url), "darwin-aarch64", "0.1.0").is_err(),
                "{url}"
            );
        }
        assert!(parse_feed(b"<html>Offline</html>", "darwin-aarch64", "0.1.0").is_err());
        assert!(parse_feed(
            &vec![b' '; MAX_BYTES as usize + 1],
            "darwin-aarch64",
            "0.1.0"
        )
        .is_err());
    }
    #[test]
    fn persistent_identity_opt_out_and_manual_checks() {
        let service = fixture();
        let initial = service.status().unwrap();
        assert!(initial.automatic_checks && initial.usage_statistics);
        let id = service
            .locked()
            .unwrap()
            .as_ref()
            .unwrap()
            .saved
            .installation_id
            .clone();
        service.preferences(false, false).unwrap();
        service
            .check_with(false, 100, |_| panic!("automatic checks disabled"))
            .unwrap();
        let status = service
            .check_with(true, 100, |saved| {
                assert!(!request_headers(saved).contains_key("x-studio-installation-id"));
                Ok(release())
            })
            .unwrap();
        assert_eq!(status.last_checked_at, Some(100));
        assert_eq!(status.last_successful_check_at, Some(100));
        assert!(status.show_notification);
        assert!(!service.dismiss().unwrap().show_notification);
        let reopened = UpdateService::new(service.root.clone());
        assert!(!reopened.status().unwrap().automatic_checks);
        assert_eq!(
            reopened
                .locked()
                .unwrap()
                .as_ref()
                .unwrap()
                .saved
                .installation_id,
            id
        );
        assert!(!reopened.status().unwrap().show_notification);
        service.preferences(true, true).unwrap();
        assert_eq!(
            request_headers(&service.locked().unwrap().as_ref().unwrap().saved)
                ["x-studio-installation-id"],
            id
        );
        fs::remove_dir_all(service.root).unwrap();
    }
    #[test]
    fn daily_checks_failure_backoff_and_offline_cached_notice() {
        let service = fixture();
        service.check_with(false, 100, |_| Ok(release())).unwrap();
        service
            .check_with(false, 200, |_| panic!("daily throttle"))
            .unwrap();
        let failed = service
            .check_with(false, 100 + DAY, |_| Err("offline".into()))
            .unwrap();
        assert_eq!(failed.last_successful_check_at, Some(100));
        assert_eq!(failed.last_checked_at, Some(100 + DAY));
        assert!(failed.available_update.is_some());
        service
            .check_with(false, 101 + DAY, |_| panic!("failure cooldown"))
            .unwrap();
        service
            .check_with(false, 100 + DAY + 3600, |_| {
                panic!("failed automatic checks also wait 24 hours")
            })
            .unwrap();
        service.check_with(true, 102 + DAY, |_| Ok(None)).unwrap();
        assert!(service.status().unwrap().available_update.is_none());
        fs::remove_dir_all(service.root).unwrap();
    }
    #[test]
    fn corrupt_preferences_do_not_silently_enable_tracking() {
        let service = fixture();
        fs::create_dir_all(&service.root).unwrap();
        fs::write(service.root.join("app-updates.json"), b"invalid").unwrap();
        assert!(service
            .check_with(false, 100, |_| panic!("corrupt preferences"))
            .is_err());
        fs::remove_dir_all(service.root).unwrap();
    }
}
