//! Startup/daily integrity checks; repairs reuse only assets already approved by the user.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};
const DAY: u64 = 86400;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Health {
    pub state: String,
    pub message: String,
    pub last_checked_at: Option<u64>,
}
pub struct ModelHealth {
    root: PathBuf,
    health: Mutex<Health>,
    running: AtomicBool,
    cancel: AtomicBool,
}
impl ModelHealth {
    pub fn new(root: PathBuf) -> Self {
        let health = fs::read(root.join("model-health.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            root,
            health: Mutex::new(health),
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    fn snapshot(&self) -> Result<Health, String> {
        self.health
            .lock()
            .map(|h| h.clone())
            .map_err(|_| "Voice check status unavailable".into())
    }
    fn update(&self, state: &str, message: &str, checked: bool) {
        if let Ok(mut h) = self.health.lock() {
            h.state = state.into();
            h.message = message.into();
            if checked {
                h.last_checked_at = Some(now());
            }
            if let Ok(bytes) = serde_json::to_vec(&*h) {
                let _ = fs::create_dir_all(&self.root);
                let temp = self.root.join("model-health.pending");
                if fs::write(&temp, bytes).is_ok() {
                    let _ = fs::rename(temp, self.root.join("model-health.json"));
                }
            }
        }
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn due(last: Option<u64>, time: u64, manual: bool, startup: bool) -> bool {
    manual || startup || last.is_none_or(|last| time < last || time - last >= DAY)
}
fn launch(app: tauri::AppHandle, manual: bool, startup: bool) -> Result<Health, String> {
    let state = app.state::<ModelHealth>();
    let saved = state.snapshot()?;
    if !due(saved.last_checked_at, now(), manual, startup)
        || state
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return state.snapshot();
    }
    state.update(
        "checking",
        "Checking the voice tools on this computer…",
        false,
    );
    let initial = state.snapshot()?;
    std::thread::spawn(move || {
        let health = app.state::<ModelHealth>();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Wait for active processing without repeatedly verifying model files.
            for _ in 0..120 {
                if health.cancel.load(Ordering::SeqCst) {
                    return Err("paused".to_owned());
                }
                let path = health.root.join("recognition.lock");
                fs::create_dir_all(&health.root).map_err(|_| "Voice storage unavailable")?;
                let file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)
                    .map_err(|_| "Voice storage unavailable")?;
                if fs2::FileExt::try_lock_exclusive(&file).is_ok()
                    && !app.state::<crate::voice_editing::EditingState>().is_busy()
                {
                    crate::naming_migration::run(&health.root, &health.cancel)?;
                    drop(file);
                    match crate::voice_setup::check_and_repair(&app) {
                        Ok(("waiting", message)) => {
                            health.update("waiting", &message, false);
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            continue;
                        }
                        result => return result,
                    }
                }
                drop(file);
                health.update(
                    "waiting",
                    "Voice tools will be checked after the current recording.",
                    false,
                );
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Err("The voice check will continue the next time Studio starts.".into())
        }))
        .unwrap_or_else(|_| Err("Voice tools could not be checked. Open setup to retry.".into()));
        match result {
            Ok((state, message)) => health.update(state, &message, true),
            Err(e) => health.update(if e == "paused" { "paused" } else { "failed" }, &e, true),
        }
        health.running.store(false, Ordering::SeqCst);
        let _ = app.emit("studio-tools-changed", health.snapshot().ok());
    });
    Ok(initial)
}
pub fn initialize(app: &tauri::AppHandle) {
    let _ = launch(app.clone(), false, true);
}
#[tauri::command]
pub fn check_model_health(app: tauri::AppHandle, manual: bool) -> Result<Health, String> {
    launch(app, manual, false)
}
#[tauri::command]
pub fn get_model_health(state: State<'_, ModelHealth>) -> Result<Health, String> {
    state.snapshot()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_once_on_startup_then_daily_or_manual() {
        assert!(due(Some(100), 101, false, true));
        assert!(!due(Some(100), 101, false, false));
        assert!(due(Some(100), 100 + DAY, false, false));
        assert!(due(Some(100), 101, true, false));
        assert!(due(None, 10, false, false));
    }
}
