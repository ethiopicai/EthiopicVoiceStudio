//! Opt-in macOS hold-Fn dictation. No text/keystroke logging or clipboard reading.
use crate::dictation_shortcut::{SavedShortcut, Shortcut};
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};
use tauri::{Emitter, Manager, State};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
#[derive(Default)]
struct Slot {
    sequence: u64,
    busy: bool,
    pressed: bool,
    session: Option<String>,
}
pub struct FnState {
    root: PathBuf,
    requested: AtomicBool,
    installed: AtomicBool,
    slot: Mutex<Slot>,
    binding: Mutex<Shortcut>,
    capturing: AtomicBool,
}
impl FnState {
    pub fn new(root: PathBuf) -> Self {
        let binding = fs::read(root.join("dictation-shortcut.json"))
            .ok()
            .and_then(|bytes| SavedShortcut::load(&bytes, std::env::consts::OS))
            .unwrap_or_else(|| Shortcut::default_for(std::env::consts::OS));
        let requested = fs::read(root.join("fn-dictation.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<bool>(&b).ok())
            .unwrap_or(false);
        Self {
            root,
            requested: AtomicBool::new(requested),
            installed: AtomicBool::new(false),
            slot: Mutex::new(Slot::default()),
            binding: Mutex::new(binding),
            capturing: AtomicBool::new(false),
        }
    }
    pub fn shutdown(&self) {
        if let Ok(mut slot) = self.slot.lock() {
            if slot.busy && slot.pressed {
                slot.pressed = false;
                if let Some(app) = APP.get() {
                    let _ = app.emit(
                        "dictation-shortcut",
                        FnEvent {
                            request_id: slot.sequence,
                            pressed: false,
                            cancelled: true,
                        },
                    );
                }
            }
        }
        self.installed.store(false, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        unsafe {
            studio_fn_disable();
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FnEvent {
    request_id: u64,
    pressed: bool,
    cancelled: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    platform: &'static str,
    shortcut: Shortcut,
    shortcut_label: String,
    default_shortcut: Shortcut,
    supported: bool,
    enabled: bool,
    requested: bool,
    input_monitoring: bool,
    accessibility: bool,
    active: Option<FnEvent>,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn studio_fn_input_allowed() -> bool;
    fn studio_fn_accessibility_allowed() -> bool;
    fn studio_fn_request_permissions();
    fn studio_fn_enable(callback: extern "C" fn(bool, bool) -> bool) -> bool;
    fn studio_fn_disable();
    fn studio_fn_overlay(text: *const std::ffi::c_char);
    fn studio_fn_clear();
    fn studio_fn_insert(text: *const std::ffi::c_char) -> std::ffi::c_int;
    fn studio_fn_set_binding(modifiers: u32, key: i32);
}
#[cfg(any(target_os = "macos", test))]
fn transition(slot: &mut Slot, pressed: bool, cancelled: bool) -> Option<FnEvent> {
    if pressed {
        if slot.busy {
            return None;
        }
        slot.sequence = slot.sequence.wrapping_add(1);
        slot.busy = true;
        slot.pressed = true;
        slot.session = None;
    } else {
        if !slot.busy || !slot.pressed {
            return None;
        }
        slot.pressed = false;
    }
    Some(FnEvent {
        request_id: slot.sequence,
        pressed,
        cancelled,
    })
}
#[cfg(target_os = "macos")]
extern "C" fn key_changed(pressed: bool, cancelled: bool) -> bool {
    let Some(app) = APP.get() else { return false };
    let state = app.state::<FnState>();
    if !state.installed.load(Ordering::SeqCst) || state.capturing.load(Ordering::SeqCst) {
        return false;
    }
    let Ok(mut slot) = state.slot.lock() else {
        return false;
    };
    let Some(event) = transition(&mut slot, pressed, cancelled) else {
        return false;
    };
    if app.emit("dictation-shortcut", event).is_err() {
        slot.busy = false;
        return false;
    }
    true
}
pub fn initialize(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    let _ = status(&app.state::<FnState>());
}
fn status(state: &FnState) -> Status {
    #[cfg(target_os = "macos")]
    let (input_monitoring, accessibility) =
        unsafe { (studio_fn_input_allowed(), studio_fn_accessibility_allowed()) };
    #[cfg(not(target_os = "macos"))]
    let (input_monitoring, accessibility) = (false, false);
    let requested = state.requested.load(Ordering::SeqCst);
    if state.installed.load(Ordering::SeqCst) && (!requested || !input_monitoring || !accessibility)
    {
        state.shutdown();
    }
    #[cfg(target_os = "macos")]
    if requested && input_monitoring && accessibility && !state.installed.load(Ordering::SeqCst) {
        if let Ok(binding) = state.binding.lock() {
            if let Some(key) = binding.mac_key_code() {
                unsafe {
                    studio_fn_set_binding(binding.modifiers.into(), key);
                }
            }
        }
        state
            .installed
            .store(unsafe { studio_fn_enable(key_changed) }, Ordering::SeqCst);
    }
    let active = state.slot.lock().ok().and_then(|s| {
        if s.busy {
            Some(FnEvent {
                request_id: s.sequence,
                pressed: s.pressed,
                cancelled: false,
            })
        } else {
            None
        }
    });
    Status {
        platform: std::env::consts::OS,
        shortcut: state
            .binding
            .lock()
            .map(|v| v.clone())
            .unwrap_or_else(|_| Shortcut::default_for(std::env::consts::OS)),
        shortcut_label: state
            .binding
            .lock()
            .map(|v| v.label(std::env::consts::OS))
            .unwrap_or_default(),
        default_shortcut: Shortcut::default_for(std::env::consts::OS),
        supported: cfg!(target_os = "macos"),
        enabled: state.installed.load(Ordering::SeqCst),
        requested,
        input_monitoring,
        accessibility,
        active,
    }
}
#[tauri::command]
pub fn get_fn_shortcut_status(state: State<'_, FnState>) -> Status {
    status(&state)
}
#[tauri::command]
pub fn set_dictation_shortcut_capture(
    state: State<'_, FnState>,
    capturing: bool,
) -> Result<(), String> {
    if capturing && state.slot.lock().map_err(|_| "Shortcut unavailable")?.busy {
        return Err("Finish dictation before changing the shortcut.".into());
    }
    state.capturing.store(capturing, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
pub fn set_dictation_shortcut(
    state: State<'_, FnState>,
    shortcut: Shortcut,
) -> Result<Status, String> {
    shortcut.validate(std::env::consts::OS)?;
    let slot = state.slot.lock().map_err(|_| "Shortcut unavailable")?;
    if slot.busy {
        return Err("Finish dictation before changing the shortcut.".into());
    }
    let saved = SavedShortcut {
        schema_version: 1,
        platform: std::env::consts::OS.into(),
        shortcut: shortcut.clone(),
    };
    fs::create_dir_all(&state.root).map_err(|_| "Could not save shortcut settings")?;
    let bytes = serde_json::to_vec(&saved).map_err(|_| "Could not save shortcut settings")?;
    let pending = state.root.join("dictation-shortcut.pending");
    fs::write(&pending, bytes)
        .and_then(|_| fs::rename(pending, state.root.join("dictation-shortcut.json")))
        .map_err(|_| "Could not save shortcut settings")?;
    *state.binding.lock().map_err(|_| "Shortcut unavailable")? = shortcut.clone();
    #[cfg(target_os = "macos")]
    if let Some(key) = shortcut.mac_key_code() {
        unsafe {
            studio_fn_set_binding(shortcut.modifiers.into(), key);
        }
    }
    state.capturing.store(false, Ordering::SeqCst);
    drop(slot);
    Ok(status(&state))
}
#[tauri::command]
pub fn set_fn_shortcut_enabled(state: State<'_, FnState>, enabled: bool) -> Result<Status, String> {
    if !cfg!(target_os = "macos") {
        return Err("Shortcut activation is not available in this build.".into());
    }
    if state.slot.lock().map_err(|_| "Shortcut unavailable")?.busy {
        return Err("Finish dictation before changing the shortcut.".into());
    }
    fs::create_dir_all(&state.root).map_err(|_| "Could not save shortcut settings")?;
    let pending = state.root.join("fn-dictation.pending");
    fs::write(
        &pending,
        if enabled {
            b"true".as_slice()
        } else {
            b"false".as_slice()
        },
    )
    .and_then(|_| fs::rename(pending, state.root.join("fn-dictation.json")))
    .map_err(|_| "Could not save shortcut settings")?;
    state.requested.store(enabled, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    unsafe {
        if enabled {
            studio_fn_request_permissions();
        } else {
            state.shutdown();
        }
    }
    Ok(status(&state))
}
#[tauri::command]
pub fn bind_fn_dictation(
    state: State<'_, FnState>,
    dictation: State<'_, crate::dictation::DictationState>,
    request_id: u64,
    session_id: String,
) -> Result<(), String> {
    let mut slot = state.slot.lock().map_err(|_| "Shortcut unavailable")?;
    if !slot.busy || slot.sequence != request_id || slot.session.is_some() {
        return Err("This shortcut recording is no longer active.".into());
    }
    let view = crate::dictation::shortcut_snapshot(&dictation, &session_id)?;
    if !matches!(view.state.as_str(), "loading" | "ready" | "recording") {
        return Err("Dictation is no longer available.".into());
    }
    slot.session = Some(session_id);
    Ok(())
}
#[tauri::command]
pub fn update_fn_overlay(
    state: State<'_, FnState>,
    request_id: u64,
    text: String,
) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    let _ = text;
    let slot = state.slot.lock().map_err(|_| "Shortcut unavailable")?;
    if !slot.busy || slot.sequence != request_id {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    if let Ok(value) = std::ffi::CString::new(
        text.chars()
            .rev()
            .take(350)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>(),
    ) {
        unsafe {
            studio_fn_overlay(value.as_ptr());
        }
    }
    Ok(())
}
#[tauri::command]
pub fn finish_fn_dictation(
    state: State<'_, FnState>,
    dictation: State<'_, crate::dictation::DictationState>,
    request_id: u64,
    transcript_id: Option<String>,
) -> Result<String, String> {
    let session = {
        let mut slot = state.slot.lock().map_err(|_| "Shortcut unavailable")?;
        if !slot.busy || slot.sequence != request_id {
            return Err("This shortcut recording is no longer active.".into());
        }
        slot.pressed = false;
        slot.session.clone()
    };
    let result = (|| {
        let Some(id) = transcript_id else {
            return Ok(String::new());
        };
        let view = crate::dictation::shortcut_snapshot(
            &dictation,
            session.as_deref().ok_or("Shortcut recording unavailable")?,
        )?;
        if view.state != "completed" || view.transcript_id.as_deref() != Some(id.as_str()) {
            return Err("The finished shortcut transcript is not available.".into());
        }
        let transcript = transcripts::TranscriptStore::new(state.root.join("transcripts"))
            .and_then(|s| s.get(&id))
            .map_err(|_| "Your transcript is saved. Open Studio to copy it.")?;
        if transcript.text.trim().is_empty() {
            return Ok("No words were recognized. Your recording is saved in Studio.".into());
        }
        #[cfg(target_os = "macos")]
        {
            let value = std::ffi::CString::new(transcript.text)
                .map_err(|_| "Your transcript is saved. Open Studio to copy it.")?;
            let code = unsafe { studio_fn_insert(value.as_ptr()) };
            Ok(match code {0=>"Inserted into your app and saved in Studio.",2=>"Saved in Studio. Allow Accessibility access to insert into other apps.",3=>"Saved in Studio. Dictation is not inserted into password fields.",4=>"Saved in Studio. This app does not support direct insertion; copy the text from Studio.",_=>"Saved in Studio. The text field changed, so no text was inserted."}.into())
        }
        #[cfg(not(target_os = "macos"))]
        Ok("Saved in Studio.".into())
    })();
    let mut slot = state.slot.lock().map_err(|_| "Shortcut unavailable")?;
    slot.busy = false;
    slot.pressed = false;
    slot.session = None;
    #[cfg(target_os = "macos")]
    unsafe {
        studio_fn_clear();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_holds_cannot_steal_a_pending_insertion_target() {
        let mut s = Slot::default();
        let first = transition(&mut s, true, false).unwrap();
        assert!(transition(&mut s, true, false).is_none());
        assert_eq!(
            transition(&mut s, false, false).unwrap().request_id,
            first.request_id
        );
        assert!(transition(&mut s, false, false).is_none());
        assert!(transition(&mut s, true, false).is_none());
        s.busy = false;
        assert_ne!(
            transition(&mut s, true, false).unwrap().request_id,
            first.request_id
        );
    }
    #[test]
    fn function_key_chords_cancel_once_instead_of_inserting_text() {
        let mut s = Slot::default();
        transition(&mut s, true, false).unwrap();
        assert!(transition(&mut s, false, true).unwrap().cancelled);
        assert!(transition(&mut s, false, false).is_none());
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn native_insertion_refuses_an_unbound_target() {
        let text = std::ffi::CString::new("fixture only").unwrap();
        // No target was captured and no permissions are requested by this test.
        assert!(matches!(unsafe { studio_fn_insert(text.as_ptr()) }, 1 | 2));
    }
}
