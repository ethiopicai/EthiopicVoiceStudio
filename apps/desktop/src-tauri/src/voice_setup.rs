//! One consent-bound setup for the everyday tools on either supported platform.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    state: String,
    stage: String,
    step: usize,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
    background_repair: bool,
    user_paused: bool,
}
pub struct VoiceSetupState {
    root: PathBuf,
    progress: Arc<Mutex<Saved>>,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    last_save: Arc<AtomicU64>,
    hardware: resource_packs::studio::Hardware,
    app: Mutex<Option<tauri::AppHandle>>,
}
impl VoiceSetupState {
    pub fn new(root: PathBuf) -> Self {
        let mut saved: Saved = fs::read(root.join("voice-setup-progress.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if ["downloading", "preparing", "checking"].contains(&saved.state.as_str()) {
            saved.state = "paused".into();
            saved.stage = "Your progress is saved. Continue when you are ready.".into();
        }
        Self {
            root,
            progress: Arc::new(Mutex::new(saved)),
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            last_save: Arc::new(AtomicU64::new(0)),
            hardware: crate::hardware::inspect(),
            app: Mutex::new(None),
        }
    }
    pub fn pause(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    fn save(&self, force: bool) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if !force && self.last_save.swap(now, Ordering::SeqCst) == now {
            return;
        }
        if let Ok(saved) = self.progress.lock() {
            if self.root.is_symlink() || self.root.join("voice-setup-progress.json").is_symlink() {
                return;
            }
            if fs::create_dir_all(&self.root).is_err() {
                return;
            }
            if let Ok(bytes) = serde_json::to_vec(&*saved) {
                let path = self.root.join("voice-setup-progress.pending");
                if path.is_symlink() {
                    return;
                }
                if fs::write(&path, bytes).is_ok() {
                    let _ = fs::rename(path, self.root.join("voice-setup-progress.json"));
                }
            }
        }
        self.notify();
    }
    fn notify(&self) {
        use tauri::Emitter;
        if let (Ok(app), Ok(saved)) = (self.app.lock(), self.progress.lock()) {
            if let Some(app) = app.as_ref() {
                let _=app.emit("studio-setup-progress",serde_json::json!({"state":saved.state,"stage":saved.stage,"downloadedBytes":saved.downloaded_bytes,"totalBytes":saved.total_bytes,"ready":saved.state=="completed","canInstall":true,"requiresConsent":false,"error":saved.error,"steps":steps(&saved,saved.state=="completed")}));
            }
        }
    }
    fn phase(&self, step: usize, state: &str, stage: &str) {
        if let Ok(mut p) = self.progress.lock() {
            p.step = step;
            if state == "completed" {
                p.background_repair = false;
                p.user_paused = false;
            }
            p.state = state.into();
            p.stage = stage.into();
        }
        self.save(true);
    }
    fn bytes(&self, bytes: u64) {
        if let Ok(mut p) = self.progress.lock() {
            p.downloaded_bytes = bytes.min(p.total_bytes);
        }
        self.save(false);
    }
    fn cancelled(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::SeqCst) {
            Err("paused".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    state: String,
    stage: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    can_install: bool,
    ready: bool,
    requires_consent: bool,
    error: Option<String>,
    steps: Vec<Step>,
}
#[derive(Serialize)]
pub struct Step {
    id: &'static str,
    title: &'static str,
    state: &'static str,
}

fn components(app: &tauri::AppHandle) -> Result<(bool, bool, bool, bool), String> {
    let state = app.state::<VoiceSetupState>();
    let manifest = crate::voice_editing::setup_manifest(&state.root)?;
    let recognition =
        crate::asr::get_asr_status(app.state::<crate::asr::AsrState>()).state == "ready";
    let speech = crate::voice_editing::generation_status_for_hardware(
        &app.state::<crate::voice_editing::EditingState>(),
        &manifest,
        &state.hardware,
    )?;
    let available = (recognition
        || crate::resource_setup::has_hardware_release(&manifest, &state.hardware))
        && (speech.ready || speech.can_prepare);
    Ok((recognition, speech.ready, available, !speech.consent))
}

pub(crate) fn download_quote(
    app: &tauri::AppHandle,
) -> Result<crate::download_setup::Plan, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Voice storage unavailable")?;
    let (recognition, speech) = installed_components(app)?;
    let mut plans = Vec::new();
    if !recognition {
        plans.push(crate::resource_setup::download_quote(&root)?);
    }
    if !speech {
        plans.push(crate::voice_editing::download_quote(
            &app.state::<crate::voice_editing::EditingState>(),
            "speech",
        )?);
    }
    let available = plans.iter().all(|p| p.available);
    let reason = plans
        .iter()
        .find(|p| !p.available)
        .map(|p| p.reason.clone())
        .unwrap_or_default();
    crate::download_setup::Plan::new(
        "voice-tools",
        "Transcription, Dictation and Text to Speech",
        available,
        reason,
        plans.into_iter().flat_map(|p| p.items).collect(),
    )
}
fn installed_components(app: &tauri::AppHandle) -> Result<(bool, bool), String> {
    let (mut recognition, mut speech, _, _) = components(app)?;
    let state = app.state::<VoiceSetupState>();
    // A failed practice run can be a temporary memory problem or a damaged cache.
    // Verify existing files only on retry, so intact models are not downloaded again.
    if state
        .progress
        .lock()
        .map_err(|_| "Setup status unavailable")?
        .state
        == "failed"
    {
        if recognition {
            recognition =
                crate::asr::verified_dictation_runtime(&state.root, &AtomicBool::new(false))
                    .is_ok();
        }
        if speech {
            speech = crate::voice_editing::guided_integrity(
                &app.state::<crate::voice_editing::EditingState>(),
            )
            .is_ok();
        }
    }
    Ok((recognition, speech))
}

fn steps(saved: &Saved, ready: bool) -> Vec<Step> {
    [
        ("download", "Download voice tools"),
        ("prepare", "Prepare them"),
        ("check", "Check they work"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, title))| Step {
        id,
        title,
        state: if ready || index < saved.step {
            "done"
        } else if index > saved.step {
            "pending"
        } else {
            match saved.state.as_str() {
                "failed" => "failed",
                "paused" => "paused",
                "downloading" | "preparing" | "checking" => "active",
                _ => "pending",
            }
        },
    })
    .collect()
}
#[tauri::command]
pub fn get_voice_setup(
    app: tauri::AppHandle,
    state: State<'_, VoiceSetupState>,
) -> Result<Progress, String> {
    let (recognition, speech, available, requires_consent) = components(&app)?;
    let saved = state
        .progress
        .lock()
        .map_err(|_| "Setup status unavailable")?
        .clone();
    let running = state.running.load(Ordering::SeqCst);
    let ready = all_ready(recognition, speech, running, &saved.state);
    let saved = display_progress(saved, ready, running);
    let status = if ready {
        "installed"
    } else if (!running && saved.state == "completed") || saved.state.is_empty() {
        "idle"
    } else {
        &saved.state
    };
    Ok(Progress {
        state: status.into(),
        stage: if ready {
            "Ready. Transcribe, dictate and synthesize speech offline.".into()
        } else if !available {
            "Setup needs compatible voice tools for this computer. Check again after an app update."
                .into()
        } else if saved.stage.is_empty() || status == "idle" {
            "One setup for transcription, dictation and Text to Speech.".into()
        } else {
            saved.stage.clone()
        },
        downloaded_bytes: saved.downloaded_bytes,
        total_bytes: saved.total_bytes,
        can_install: available,
        ready,
        requires_consent,
        error: saved.error.clone(),
        steps: steps(&saved, ready),
    })
}
fn display_progress(saved: Saved, ready: bool, running: bool) -> Saved {
    if !ready && !running && saved.state == "completed" {
        Saved::default()
    } else {
        saved
    }
}
fn all_ready(recognition: bool, speech: bool, running: bool, state: &str) -> bool {
    recognition && speech && !running && matches!(state, "" | "idle" | "completed" | "installed")
}
#[tauri::command]
pub fn pause_voice_setup(app: tauri::AppHandle, state: State<'_, VoiceSetupState>) {
    if let Ok(mut p) = state.progress.lock() {
        p.user_paused = true;
    }
    state.save(true);
    state.pause();
    app.state::<crate::voice_editing::EditingState>()
        .pause_setup();
}
#[tauri::command]
pub fn start_voice_setup(
    app: tauri::AppHandle,
    state: State<'_, VoiceSetupState>,
    approved_plan: String,
    accepted_consent: bool,
) -> Result<(), String> {
    crate::download_setup::require(&app, "voice-tools", &approved_plan)?;
    let (_, _, _, needs_consent) = components(&app)?;
    if needs_consent && !accepted_consent {
        return Err("Please accept the recording-rights note before setup.".into());
    }
    let approval = download_quote(&app)?;
    remember_approval(&state.root, &approval.items)?;
    let installed = installed_components(&app)?;
    if state
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Voice setup is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let quote = match download_quote(&app) {
        Ok(q) => q,
        Err(e) => {
            state.running.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    if let Ok(mut p) = state.progress.lock() {
        p.error = None;
        p.background_repair = false;
        p.user_paused = false;
        p.step = 0;
        p.downloaded_bytes = 0;
        p.total_bytes = quote.download_bytes;
        p.state = "downloading".into();
        p.stage = "Getting your voice tools…".into();
    }
    state.save(true);
    std::thread::spawn(move || {
        let state = app.state::<VoiceSetupState>();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||run(&app, &state, accepted_consent, installed)))
            .unwrap_or_else(|_|Err("Setup stopped unexpectedly. Continue setup to try again; your recordings are safe.".into()));
        if let Err(error) = result {
            if let Ok(mut p) = state.progress.lock() {
                p.state = if error == "paused" {
                    "paused"
                } else {
                    "failed"
                }
                .into();
                p.stage = if error == "paused" {
                    "Your progress is saved. Continue whenever you are ready."
                } else {
                    "Setup needs another try. Your recordings and working tools are safe."
                }
                .into();
                p.error = if error == "paused" { None } else { Some(error) };
            }
            state.save(true);
        }
        state.running.store(false, Ordering::SeqCst);
    });
    Ok(())
}
fn run(
    app: &tauri::AppHandle,
    state: &VoiceSetupState,
    accepted_consent: bool,
    installed: (bool, bool),
) -> Result<(), String> {
    fs::create_dir_all(&state.root).map_err(|_| "Could not prepare voice storage")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.root.join("voice-setup.lock"))
        .map_err(|_| "Voice setup unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "Another Studio window is preparing these tools. Wait and retry.")?;
    let (recognition, speech) = installed;
    let (_, _, _, needs_consent) = components(app)?;
    let editing = app.state::<crate::voice_editing::EditingState>();
    let manifest = crate::voice_editing::setup_manifest(&state.root)?;
    let recognition_space = if recognition {
        0
    } else {
        crate::resource_setup::required_storage_for_hardware(
            &state.root,
            &manifest,
            &state.hardware,
        )
        .ok_or("Recognition tools are not available for this computer yet")?
    };
    let speech_space = if speech {
        0
    } else {
        crate::voice_editing::guided_storage_required(&editing)?
    };
    let required = recognition_space.saturating_add(speech_space);
    if fs2::available_space(&state.root).map_err(|_| "Could not check free storage")? < required {
        return Err(format!(
            "Free at least {:.1} GB of storage, then continue setup. Your progress will be kept.",
            required as f64 / 1e9
        ));
    }
    if needs_consent && accepted_consent {
        crate::voice_editing::accept_setup_consent(&editing)?;
    }
    let mut base = 0;
    state.cancelled()?;
    if !recognition {
        state.phase(
            0,
            "downloading",
            "Downloading the tools for transcription and dictation…",
        );
        crate::resource_setup::prefetch_guided(&state.root, &state.cancel, |bytes| {
            state.bytes(bytes)
        })?;
        base = crate::resource_setup::download_quote(&state.root)?.download_bytes;
    }
    state.cancelled()?;
    if !speech {
        state.phase(
            0,
            "downloading",
            "Downloading the tools for Text to Speech…",
        );
        crate::voice_editing::prefetch_guided(&editing, state.cancel.clone(), |bytes| {
            state.bytes(base.saturating_add(bytes))
        })?;
    }
    state.cancelled()?;
    state.phase(
        1,
        "preparing",
        "Preparing your voice tools. We take care of the setup.",
    );
    if !recognition {
        crate::resource_setup::install_guided(&state.root, &state.cancel)?;
    }
    state.cancelled()?;
    if !speech {
        crate::voice_editing::install_guided(&editing, state.cancel.clone())?;
    }
    state.cancelled()?;
    state.phase(
        2,
        "checking",
        "Trying a short recording and a little speech offline…",
    );
    crate::resource_setup::check_guided(&state.root, &state.cancel)?;
    state.cancelled()?;
    crate::voice_editing::check_guided(&editing, state.cancel.clone())?;
    state.cancelled()?;
    let (recognition, speech, _, _) = components(app)?;
    if !recognition || !speech {
        return Err("Some voice tools are not ready yet. Continue setup to finish.".into());
    }
    state.phase(3, "completed", "Ready. Your voice tools now work offline.");
    Ok(())
}

fn remember_approval(
    root: &std::path::Path,
    items: &[crate::download_setup::Item],
) -> Result<(), String> {
    let path = root.join("voice-tools-approval.json");
    if path.is_symlink() {
        return Err("Voice approval storage is redirected".into());
    }
    let mut saved: Vec<crate::download_setup::Item> = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for item in items {
        if !saved
            .iter()
            .any(|v| v.url == item.url && v.sha256 == item.sha256 && v.bytes == item.bytes)
        {
            saved.push(item.clone());
        }
    }
    fs::create_dir_all(root).map_err(|_| "Could not save download approval")?;
    let pending = root.join("voice-tools-approval.pending");
    fs::write(
        &pending,
        serde_json::to_vec(&saved).map_err(|_| "Could not save download approval")?,
    )
    .and_then(|_| fs::rename(pending, path))
    .map_err(|_| "Could not save download approval".into())
}
fn covered(
    items: &[crate::download_setup::Item],
    approved: &[crate::download_setup::Item],
) -> bool {
    !items.is_empty()
        && items.iter().all(|item| {
            approved
                .iter()
                .any(|v| v.url == item.url && v.sha256 == item.sha256 && v.bytes == item.bytes)
        })
}
fn respect_pause(saved: &Saved) -> bool {
    saved.user_paused || (saved.state == "paused" && !saved.background_repair)
}
fn has_prior_setup(root: &std::path::Path, saved: &Saved) -> bool {
    (!saved.state.is_empty() && saved.state != "idle")
        || [
            "voice-tools-approval.json",
            "asr/active/installation.json",
            "tts-runtime/active/installation.json",
            "studio-runtime/active/installation.json",
        ]
        .iter()
        .any(|path| root.join(path).is_file())
}
pub(crate) fn check_and_repair(app: &tauri::AppHandle) -> Result<(&'static str, String), String> {
    use tauri::Emitter;
    let state = app.state::<VoiceSetupState>();
    if state.running.load(Ordering::SeqCst) {
        return Ok(("checking", "Voice setup is already running.".into()));
    }
    let saved = state
        .progress
        .lock()
        .map_err(|_| "Voice setup status unavailable")?
        .clone();
    if respect_pause(&saved) {
        return Ok((
            "paused",
            "Voice setup is paused. Continue when you are ready.".into(),
        ));
    }
    // Preserve the first-use download introduction on a barebones install.
    if !has_prior_setup(&state.root, &saved) {
        return Ok((
            "needs_setup",
            "Set up your voice tools when you are ready.".into(),
        ));
    }
    let recognition = crate::asr::verified_dictation_runtime(&state.root, &state.cancel).is_ok();
    let speech = match crate::voice_editing::guided_integrity(
        &app.state::<crate::voice_editing::EditingState>(),
    ) {
        Ok(()) => true,
        Err(error) if error == "A voice-editing task is already running." => {
            return Ok((
                "waiting",
                "Voice tools will be checked after the current recording.".into(),
            ))
        }
        Err(_) => false,
    };
    let unfinished_check = matches!(saved.state.as_str(), "paused" | "failed" | "checking");
    if recognition && speech && !unfinished_check {
        return Ok(("ready", "Voice tools are ready.".into()));
    }
    let mut plans = Vec::new();
    if !recognition {
        plans.push(crate::resource_setup::download_quote(&state.root)?);
    }
    if !speech {
        plans.push(crate::voice_editing::download_quote(
            &app.state::<crate::voice_editing::EditingState>(),
            "speech",
        )?);
    }
    let approved: Vec<crate::download_setup::Item> =
        fs::read(state.root.join("voice-tools-approval.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
    let items: Vec<_> = plans.iter().flat_map(|p| p.items.clone()).collect();
    let (_, _, _, needs_consent) = components(app)?;
    if needs_consent
        || plans.iter().any(|p| !p.available)
        || (!items.is_empty() && !covered(&items, &approved))
    {
        if !state.running.load(Ordering::SeqCst)
            && (state.root.join("asr/active").exists()
                || state.root.join("voice-edit/models").exists())
        {
            if let Ok(mut p) = state.progress.lock() {
                p.state = "failed".into();
                p.stage = "Some voice tools need repair.".into();
                p.error = Some("Open setup to review the files needed for this repair.".into());
            }
            state.save(true);
        }
        return Ok((
            "needs_setup",
            "Open setup to review the voice tools needed by this computer.".into(),
        ));
    }
    if state
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(("checking", "Voice setup is already running.".into()));
    }
    state.cancel.store(false, Ordering::SeqCst);
    if let Ok(mut p) = state.progress.lock() {
        p.error = None;
        p.step = 0;
        p.state = "preparing".into();
        p.background_repair = true;
        p.user_paused = false;
        p.stage = "Repairing your approved voice tools in the background…".into();
        p.total_bytes = items.iter().map(|i| i.bytes).sum();
        p.downloaded_bytes = 0;
    }
    state.save(true);
    let _ = app.emit("studio-tools-changed", ());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(app, &state, false, (recognition, speech))
    }))
    .unwrap_or_else(|_| Err("Repair stopped unexpectedly. Open setup to retry.".into()));
    state.running.store(false, Ordering::SeqCst);
    if let Err(error) = result {
        if let Ok(mut p) = state.progress.lock() {
            p.state = if error == "paused" {
                "paused"
            } else {
                "failed"
            }
            .into();
            p.stage = "Voice tools need attention. Your recordings are safe.".into();
            p.error = Some(error.clone());
        }
        state.save(true);
        return Err(error);
    }
    Ok((
        "ready",
        "Your voice tools were repaired and checked offline.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_repairs_require_the_same_previously_approved_assets() {
        let item = crate::download_setup::Item {
            name: "Model".into(),
            purpose: "Voice".into(),
            bytes: 10,
            url: "https://huggingface.co/ethiopic/model".into(),
            sha256: "a".repeat(64),
        };
        assert!(!covered(&[item.clone()], &[]));
        assert!(covered(&[item.clone()], &[item.clone()]));
        let mut changed = item.clone();
        changed.sha256 = "b".repeat(64);
        assert!(!covered(&[changed], &[item]));
    }
    #[test]
    fn interrupted_background_repairs_resume_but_explicit_pauses_are_respected() {
        let mut saved = Saved {
            state: "paused".into(),
            ..Default::default()
        };
        assert!(respect_pause(&saved));
        saved.background_repair = true;
        assert!(!respect_pause(&saved));
        saved.user_paused = true;
        assert!(respect_pause(&saved));
        saved.state = "failed".into();
        assert!(respect_pause(&saved));
        let legacy: Saved = serde_json::from_str(r#"{"state":"paused"}"#).unwrap();
        assert!(respect_pause(&legacy));
    }
    #[test]
    fn barebones_checks_do_not_mistake_empty_model_folders_for_an_install() {
        let root = std::env::temp_dir().join(format!("studio-first-health-{}", std::process::id()));
        fs::create_dir_all(root.join("voice-edit/models")).unwrap();
        assert!(!has_prior_setup(&root, &Saved::default()));
        fs::write(root.join("voice-tools-approval.json"), b"[]").unwrap();
        assert!(has_prior_setup(&root, &Saved::default()));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ready_requires_both_features_and_successful_completed_checks() {
        assert!(!all_ready(true, false, false, "installed"));
        assert!(!all_ready(false, true, false, "installed"));
        assert!(!all_ready(true, true, true, "checking"));
        assert!(!all_ready(true, true, false, "checking"));
        assert!(!all_ready(true, true, false, "failed"));
        assert!(!all_ready(true, true, false, "paused"));
        assert!(all_ready(true, true, false, "completed"));
    }
    #[test]
    fn missing_resources_cannot_keep_old_completed_step_checks() {
        let saved = Saved {
            state: "completed".into(),
            step: 3,
            downloaded_bytes: 100,
            total_bytes: 100,
            ..Default::default()
        };
        let visible = display_progress(saved.clone(), false, false);
        assert!(steps(&visible, false)
            .iter()
            .all(|step| step.state == "pending"));
        assert_eq!(visible.downloaded_bytes, 0);
        assert_eq!(display_progress(saved, true, false).step, 3);
    }
    #[test]
    fn failed_checks_and_partial_installations_never_show_all_green() {
        let saved = Saved {
            state: "failed".into(),
            step: 2,
            ..Default::default()
        };
        let values = steps(&saved, false);
        assert_eq!(
            values.iter().map(|s| s.state).collect::<Vec<_>>(),
            vec!["done", "done", "failed"]
        );
        assert!(steps(&Saved::default(), false)
            .iter()
            .all(|s| s.state == "pending"));
        assert!(steps(&saved, true).iter().all(|s| s.state == "done"));
    }
    #[test]
    fn restart_resumes_unfinished_setup_instead_of_reporting_ready() {
        let root = std::env::temp_dir().join(format!(
            "voice-setup-restart-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let saved = Saved {
            state: "checking".into(),
            step: 2,
            downloaded_bytes: 90,
            total_bytes: 100,
            ..Default::default()
        };
        fs::write(
            root.join("voice-setup-progress.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let state = VoiceSetupState::new(root.clone());
        let p = state.progress.lock().unwrap();
        assert_eq!(p.state, "paused");
        assert_eq!(p.step, 2);
        assert_eq!(p.downloaded_bytes, 90);
        assert_eq!(steps(&p, false)[2].state, "paused");
        drop(p);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "loads real recognition and speech models from explicitly supplied qualified release files"]
    fn full_local_voice_setup_installs_both_runtimes_and_passes_offline_inference() {
        let evidence = PathBuf::from(
            std::env::var("ETHIOPIC_VOICE_SETUP_TEST_FILES")
                .expect("Supply a local test-files JSON"),
        );
        let files: serde_json::Value =
            serde_json::from_slice(&fs::read(evidence).unwrap()).unwrap();
        let root =
            std::env::temp_dir().join(format!("ethiopic-full-voice-setup-{}", std::process::id()));
        assert!(!root.exists(), "Use a fresh isolated setup directory");
        fs::create_dir_all(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if std::env::var("ETHIOPIC_KEEP_VOICE_SETUP_TEST").as_deref() != Ok("1") {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }
        }
        let _cleanup = Cleanup(root.clone());
        let editing = crate::voice_editing::EditingState::new(root.clone());
        let recognition = crate::resource_setup::download_quote(&root).unwrap();
        let speech = crate::voice_editing::download_quote(&editing, "speech").unwrap();
        assert!(
            recognition.available && speech.available,
            "Both model+runtime routes must be published"
        );
        // Seed only owner-supplied public model/runtime files; production downloaders
        // still check their pinned size and SHA before accepting the cache.
        for item in &recognition.items {
            let name = item.url.rsplit('/').next().unwrap();
            let source = PathBuf::from(files["recognitionPacks"].as_str().unwrap()).join(name);
            let cache = root.join("resource-downloads").join(&item.sha256);
            fs::create_dir_all(&cache).unwrap();
            fs::copy(source, cache.join("download.tar.gz")).unwrap();
        }
        for item in &speech.items {
            let source = if item.name == "Speech engine runtime" {
                PathBuf::from(files["speechRuntime"].as_str().unwrap())
            } else {
                PathBuf::from(files["models"].as_str().unwrap()).join(&item.name)
            };
            let cache = root.join("voice-edit/downloads").join(&item.sha256);
            fs::create_dir_all(&cache).unwrap();
            fs::copy(source, cache.join("download.tar.gz")).unwrap();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        crate::voice_editing::accept_setup_consent(&editing).unwrap();
        crate::resource_setup::prefetch_guided(&root, &cancel, |_| {}).unwrap();
        crate::voice_editing::prefetch_guided(&editing, cancel.clone(), |_| {}).unwrap();
        assert!(
            !crate::voice_editing::generation_status(&editing)
                .unwrap()
                .ready,
            "Downloads alone cannot mean Ready"
        );
        crate::resource_setup::install_guided(&root, &cancel).unwrap();
        assert!(
            !crate::voice_editing::generation_status(&editing)
                .unwrap()
                .ready,
            "ASR alone cannot complete speech setup"
        );
        crate::voice_editing::install_guided(&editing, cancel.clone()).unwrap();
        crate::resource_setup::check_guided(&root, &cancel).unwrap();
        crate::voice_editing::check_guided(&editing, cancel).unwrap();
        assert!(
            crate::voice_editing::generation_status(&editing)
                .unwrap()
                .ready
        );
        assert!(root.join("asr/active/installation.json").is_file());
        assert!(
            root.join("tts-runtime/active/installation.json").is_file()
                || root
                    .join("studio-runtime/active/installation.json")
                    .is_file()
        );
        println!("Qualified full voice setup at {}", root.display());
    }
}

pub(crate) fn attach(app: &tauri::AppHandle) {
    let state = app.state::<VoiceSetupState>();
    if let Ok(mut handle) = state.app.lock() {
        *handle = Some(app.clone());
    };
}
