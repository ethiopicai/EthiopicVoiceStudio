use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Cursor, Read},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};
use transcripts::{Transcript, TranscriptStore};

struct JobFiles(PathBuf);
impl Drop for JobFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MAX_AUDIO: usize = 100 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn id() -> String {
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::SeqCst)
    )
}
fn safe_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("Invalid recording identifier".into());
    }
    Ok(())
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub state: String,
    pub stage: String,
    pub error: Option<String>,
    pub transcript_id: Option<String>,
}
struct Task {
    job: Job,
    cancel: Arc<AtomicBool>,
}
pub struct AsrState {
    root: PathBuf,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    running: Arc<AtomicBool>,
}
impl AsrState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            tasks: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}
#[derive(Deserialize)]
struct IntegrityFile {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
struct Installation {
    schema_version: u32,
    os: String,
    arch: String,
    python: String,
    site_packages: String,
    model_version: String,
    files: Vec<IntegrityFile>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrStatus {
    release: Option<String>,
    family: Option<String>,
    model_id: Option<String>,
    pub(crate) state: String,
    message: String,
    model_version: Option<String>,
}
fn resources(root: &Path) -> PathBuf {
    root.join("asr/active")
}
fn inside(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("Voice tools need repair".into());
    }
    let base = root
        .canonicalize()
        .map_err(|_| "Voice tools are not installed")?;
    let target = root
        .join(path)
        .canonicalize()
        .map_err(|_| "Voice tools are incomplete. Run setup or repair.")?;
    if !target.starts_with(base) {
        return Err("Voice tools need repair".into());
    }
    Ok(target)
}
fn installation(root: &Path) -> Result<(Installation, PathBuf, PathBuf), String> {
    let root = resources(root);
    let data=fs::read(root.join("installation.json")).map_err(|_|"Amharic voice tools are not installed. Open setup to install the model and private runtime.")?;
    let config: Installation =
        serde_json::from_slice(&data).map_err(|_| "Voice tools need repair")?;
    if config.schema_version != 1
        || config.os != std::env::consts::OS
        || config.arch != std::env::consts::ARCH
    {
        return Err("This voice-tool package is not compatible with this computer.".into());
    }
    if let Some(bundle) = validate_bundle(&root, &config)? {
        if bundle.transcription.model_version != config.model_version {
            return Err(
                "Recognition version does not match its installation. Repair voice tools.".into(),
            );
        }
    } else {
        for file in [
            "bundle/model/model.safetensors",
            "bundle/model/config.json",
            "bundle/model/tokenizer.model",
            "bundle/omniasr_hf/transcribe.py",
        ] {
            inside(&root, file)?;
        }
        let version: serde_json::Value = serde_json::from_slice(
            &fs::read(inside(&root, "bundle/model/version.json")?)
                .map_err(|_| "Voice tools need repair")?,
        )
        .map_err(|_| "Voice tools need repair")?;
        let effective = crate::naming_migration::legacy_version(&root.join("bundle"), &version);
        if effective
            .as_deref()
            .or_else(|| version["model_version"].as_str())
            != Some(&config.model_version)
        {
            return Err(
                "The model version does not match its installation. Repair voice tools.".into(),
            );
        }
    }
    let python = inside(&root, &config.python)?;
    let site = inside(&root, &config.site_packages)?;
    Ok((config, python, site))
}

fn validate_bundle(
    installation_root: &Path,
    config: &Installation,
) -> Result<Option<crate::recognition_bundle::Bundle>, String> {
    let Some(bundle) = crate::recognition_bundle::read(&installation_root.join("bundle"))? else {
        return Ok(None);
    };
    let app_root = installation_root
        .parent()
        .and_then(Path::parent)
        .ok_or("Voice storage unavailable")?;
    let manifest = crate::voice_editing::setup_manifest(app_root)?;
    let mut files = HashMap::new();
    for file in &config.files {
        if file.sha256.len() != 64
            || !file.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            || files
                .insert(file.path.as_str(), file.sha256.as_str())
                .is_some()
        {
            return Err("Recognition integrity manifest is invalid. Repair voice tools.".into());
        }
    }
    if !resource_packs::safe_path(&config.python)
        || !config.python.starts_with("runtime/")
        || !resource_packs::safe_path(&config.site_packages)
        || !config.site_packages.starts_with("runtime/")
        || !files.contains_key("bundle/recognition.json")
        || !files.contains_key(config.python.as_str())
        || !config.files.iter().any(|f| {
            f.path
                .starts_with(&format!("{}/", config.site_packages.trim_end_matches('/')))
        })
    {
        return Err("Recognition integrity manifest is incomplete. Repair voice tools.".into());
    }
    bundle.validate_files(&installation_root.join("bundle"), &manifest, &files)?;
    Ok(Some(bundle))
}

pub(crate) fn validate_model_identity(
    root: &Path,
    dictation: bool,
    result: &transcripts::ModelTranscript,
) -> Result<(), String> {
    if let Some(bundle) = crate::recognition_bundle::read(&resources(root).join("bundle"))? {
        let selected = if dictation {
            bundle.dictation
        } else {
            bundle.transcription
        };
        if result.model_version != selected.model_version
            || result.model.as_deref() != Some(selected.model.as_str())
        {
            return Err(
                "The transcript does not match its selected recognition model. Repair voice tools."
                    .into(),
            );
        }
    }
    Ok(())
}
fn verify_installation(
    root: &Path,
    config: &Installation,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let entries: Vec<_> = config
        .files
        .iter()
        .map(|entry| (entry.path.as_str(), entry.sha256.as_str()))
        .collect();
    crate::resource_verification::verify(
        &resources(root),
        &entries,
        &root.join("asr/verification-cache.json"),
        cancel,
    )
    .map(|_| ())
}
/// Validated local runtime used by streaming dictation; no alternate interpreter or hub fallback.
pub(crate) fn verified_dictation_runtime(
    root: &Path,
    cancel: &AtomicBool,
) -> Result<(String, PathBuf, PathBuf, PathBuf), String> {
    let (config, python, site) = installation(root)?;
    let base = resources(root);
    let selection = validate_bundle(&base, &config)?;
    if selection.is_none() {
        for required in [
            "bundle/model/model.safetensors",
            "bundle/model/config.json",
            "bundle/model/tokenizer.model",
            "bundle/model/version.json",
            "bundle/omniasr_hf/transcribe.py",
            "bundle/omniasr_hf/audio_text.py",
            "bundle/omniasr_hf/__init__.py",
            &config.python,
        ] {
            if !config.files.iter().any(|entry| entry.path == required) {
                return Err(
                    "Dictation integrity manifest is incomplete. Repair voice tools.".into(),
                );
            }
        }
    }
    if !config.files.iter().any(|entry| {
        entry
            .path
            .starts_with(&format!("{}/", config.site_packages.trim_end_matches('/')))
    }) {
        return Err("Dictation runtime integrity manifest is incomplete".into());
    }
    verify_installation(root, &config, cancel)?;
    let version = selection
        .map(|b| b.dictation.model_version)
        .unwrap_or(config.model_version);
    Ok((version, python, site, base.join("bundle")))
}
#[tauri::command]
pub fn get_asr_status(state: State<'_, AsrState>) -> AsrStatus {
    match installation(&state.root) {
        Ok((config, _, _)) => {
            let manifest = crate::voice_editing::setup_manifest(&state.root).ok();
            let id = crate::recognition_bundle::read(&resources(&state.root).join("bundle"))
                .ok()
                .flatten()
                .map(|b| b.transcription.variant)
                .or_else(|| {
                    (config.model_version == "v1.0")
                        .then(|| "asr-wav2vec_ctc-1b-amh-fp16".to_owned())
                });
            let variant = id.as_deref().and_then(|id| manifest.as_ref()?.find(id));
            AsrStatus {
                release: variant.map(|v| v.release.clone()),
                family: variant.map(|v| v.family.clone()),
                model_id: id.clone(),
                state: "ready".into(),
                message: "Amharic ASR is installed. Recognition runs locally on this computer."
                    .into(),
                model_version: Some(config.model_version),
            }
        }
        Err(message) => AsrStatus {
            release: None,
            family: None,
            model_id: None,
            state: "not_installed".into(),
            message,
            model_version: None,
        },
    }
}
pub fn validate_audio(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_AUDIO || bytes.len() < 44 {
        return Err("Choose a nonempty PCM WAV recording up to 100 MB.".into());
    }
    let reader = hound::WavReader::new(Cursor::new(bytes))
        .map_err(|_| "Choose an uncompressed PCM WAV recording.")?;
    let s = reader.spec();
    if s.sample_format != hound::SampleFormat::Int
        || ![8, 16, 24, 32].contains(&s.bits_per_sample)
        || !(1..=8).contains(&s.channels)
        || !(8000..=192000).contains(&s.sample_rate)
        || reader.duration() < s.sample_rate / 40
    {
        return Err(
            "Use a PCM WAV recording with speech, 8–192 kHz and no more than 8 channels.".into(),
        );
    }
    Ok(())
}
#[derive(Serialize)]
pub struct Imported {
    pub id: String,
    pub name: String,
}
#[tauri::command]
pub async fn import_transcription_audio(
    state: State<'_, AsrState>,
    name: String,
    bytes: Vec<u8>,
    source_media: Option<crate::recording_transfer::Context>,
) -> Result<Imported, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || import_audio(&root, name, bytes, source_media))
        .await
        .map_err(|_| "Import interrupted")?
}
pub(crate) fn import_audio(
    root: &Path,
    name: String,
    bytes: Vec<u8>,
    source_media: Option<crate::recording_transfer::Context>,
) -> Result<Imported, String> {
    let (name, bytes) = crate::audio_import::prepare(name, bytes)?;
    validate_audio(&bytes)?;
    if name.is_empty()
        || name.len() > 240
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || !name.to_lowercase().ends_with(".wav")
    {
        return Err("Choose a WAV recording with a valid filename.".into());
    }
    let id = id();
    let parent = root.join("recordings");
    fs::create_dir_all(&parent).map_err(|_| "Recording storage unavailable")?;
    if parent
        .canonicalize()
        .map_err(|_| "Recording storage unavailable")?
        != root
            .canonicalize()
            .map_err(|_| "Recording storage unavailable")?
            .join("recordings")
    {
        return Err("Redirected recording storage is not supported.".into());
    }
    let dir = parent.join(format!(".pending-{id}"));
    fs::create_dir(&dir).map_err(|_| "Recording storage unavailable")?;
    let result = (|| {
        crate::recording_transfer::write_context(&dir, source_media.as_ref(), &bytes)?;
        fs::write(dir.join(&name), bytes)
            .map_err(|_| "Could not save recording. Check available storage.")?;
        fs::write(dir.join("name.json"), serde_json::to_vec(&name).unwrap())
            .map_err(|_| "Could not save recording metadata")?;
        fs::rename(&dir, parent.join(&id)).map_err(|_| "Could not finish saving recording")?;
        Ok::<_, String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    result?;
    Ok(Imported { id, name })
}
fn store(state: &AsrState) -> Result<TranscriptStore, String> {
    TranscriptStore::new(state.root.join("transcripts"))
        .map_err(|_| "Transcript storage could not be opened".into())
}
#[tauri::command]
pub async fn list_transcripts(state: State<'_, AsrState>) -> Result<Vec<Transcript>, String> {
    store(&state)?
        .list()
        .map_err(|_| "Transcript history could not be read".into())
}
fn deletion_lock(root: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(root).map_err(|_| "History storage unavailable")?;
    let path = root.join("recognition.lock");
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("History storage needs repair".into());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| "History storage unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "Wait for processing to finish or cancel it before deleting history.")?;
    Ok(lock)
}
pub(crate) fn managed_deletion_dir(
    root: &Path,
    category: &str,
    id: &str,
) -> Result<PathBuf, String> {
    safe_id(id)?;
    let base = root.join(category);
    let path = base.join(id);
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "History storage unavailable")?;
    if base.exists()
        && base
            .canonicalize()
            .map_err(|_| "History storage unavailable")?
            != canonical_root.join(category)
    {
        return Err("Refusing to delete redirected history storage".into());
    }
    fn check(path: &Path) -> Result<(), String> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("History files could not be checked".into()),
        };
        if metadata.file_type().is_symlink() {
            return Err("Refusing to delete redirected history files".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path).map_err(|_| "History files could not be checked")? {
                check(
                    &entry
                        .map_err(|_| "History files could not be checked")?
                        .path(),
                )?;
            }
        }
        Ok(())
    }
    if fs::symlink_metadata(&base).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Refusing to delete redirected history storage".into());
    }
    check(&path)?;
    Ok(path)
}
pub(crate) fn remove_managed_dir(root: &Path, category: &str, id: &str) -> Result<(), String> {
    let path = managed_deletion_dir(root, category, id)?;
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Some app files could not be deleted. Close audio players and retry.".into()),
    }
}
fn delete_history(state: &AsrState, id: &str, revision: u64) -> Result<Vec<String>, String> {
    let _processing = deletion_lock(&state.root)?;
    delete_history_locked(state, id, revision)
}
fn delete_history_locked(state: &AsrState, id: &str, revision: u64) -> Result<Vec<String>, String> {
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "History queue unavailable")?;
    let dir = managed_deletion_dir(&state.root, "transcripts", id)?;
    let store = store(state)?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let transcript = store
        .get(id)
        .map_err(|_| "Transcript could not be read; refresh history and retry")?;
    if transcript.revision != revision {
        return Err("Transcript changed. Refresh history before deleting.".into());
    }
    let source = match fs::read(dir.join("audio.wav")) {
        Ok(bytes) => Some(bytes),
        // A previous deletion can remove audio before metadata removal fails.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Transcript audio unavailable; deletion did not finish".into()),
    };
    let mut removed = vec![];
    let recordings = state.root.join("recordings");
    if recordings.exists() {
        for entry in fs::read_dir(&recordings).map_err(|_| "Imported recordings unavailable")? {
            let entry = entry.map_err(|_| "Imported recordings unavailable")?;
            let candidate = entry.file_name().to_string_lossy().to_string();
            if safe_id(&candidate).is_err() {
                continue;
            }
            if let Some(source_id) = &transcript.source_recording_id {
                if source_id != &candidate {
                    continue;
                }
            } else if candidate
                .split('-')
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|created| created > transcript.created_at_ms)
            {
                continue;
            }
            let path = managed_deletion_dir(&state.root, "recordings", &candidate)?;
            let Ok(metadata) = fs::read(path.join("name.json")) else {
                continue;
            };
            let Ok(name) = serde_json::from_slice::<String>(&metadata) else {
                continue;
            };
            if name != transcript.name || name.contains(['/', '\\']) {
                continue;
            }
            let audio = path.join(name);
            if source.as_ref().is_some_and(|source| {
                fs::metadata(&audio).is_ok_and(|m| m.len() == source.len() as u64)
                    && fs::read(audio).is_ok_and(|bytes| bytes == *source)
            }) {
                remove_managed_dir(&state.root, "recordings", &candidate)?;
                removed.push(candidate);
            }
        }
    }
    // No inference is running under the process lock. Purge transient worker files,
    // including any leftovers from a previous crash; these are never source files.
    for category in ["jobs", "dictation-jobs"] {
        let jobs = state.root.join(category);
        if jobs.exists() {
            for entry in fs::read_dir(jobs).map_err(|_| "Temporary history files unavailable")? {
                let name = entry
                    .map_err(|_| "Temporary history files unavailable")?
                    .file_name()
                    .to_string_lossy()
                    .to_string();
                remove_managed_dir(&state.root, category, &name)?;
            }
        }
    }
    store
        .delete(id, revision)
        .map_err(|_| "Deletion did not finish. Refresh history and retry.".to_string())?;
    tasks.retain(|_, task| task.job.transcript_id.as_deref() != Some(id));
    Ok(removed)
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum DeleteKind {
    Transcript,
    Recording,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteItem {
    kind: DeleteKind,
    id: String,
    revision: Option<u64>,
}
#[derive(Serialize, Debug)]
pub struct DeletedItem {
    kind: DeleteKind,
    id: String,
}
#[derive(Serialize, Debug)]
pub struct DeleteFailure {
    kind: DeleteKind,
    id: String,
    message: String,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
pub struct DeleteBatchResult {
    deleted: Vec<DeletedItem>,
    failed: Vec<DeleteFailure>,
    removed_recording_ids: Vec<String>,
}
fn preflight_deletions(state: &AsrState, items: &[DeleteItem]) -> Result<Vec<String>, String> {
    if items.len() > 5000 {
        return Err("Select no more than 5,000 transcriptions at a time.".into());
    }
    let mut seen = std::collections::HashSet::new();
    for item in items {
        safe_id(&item.id)?;
        if !seen.insert((item.kind, item.id.as_str())) {
            return Err(
                "The deletion selection contains duplicate items. Refresh history and retry."
                    .into(),
            );
        }
        match item.kind {
            DeleteKind::Transcript => {
                let revision = item.revision.filter(|revision| *revision > 0).ok_or(
                    "Saved transcriptions require a current revision. Refresh history and retry.",
                )?;
                let path = managed_deletion_dir(&state.root, "transcripts", &item.id)?;
                if path.exists() {
                    let transcript = store(state)?.get(&item.id).map_err(|_| {
                        "A selected transcript could not be checked. Refresh history and retry."
                    })?;
                    if transcript.revision != revision {
                        return Err(
                            "A selected transcript changed. Refresh history before deleting."
                                .into(),
                        );
                    }
                }
            }
            DeleteKind::Recording => {
                managed_deletion_dir(&state.root, "recordings", &item.id)?;
            }
        }
    }
    let mut recordings = vec![];
    // Saved-project deletion also removes linked imports and stale worker output.
    // Validate these derived targets before deleting any selected item.
    for category in ["recordings", "jobs", "dictation-jobs"] {
        managed_deletion_dir(&state.root, category, "batch-preflight")?;
        let parent = state.root.join(category);
        if !parent.exists() {
            continue;
        }
        for entry in fs::read_dir(parent)
            .map_err(|_| "History files could not be checked before deletion")?
        {
            let entry = entry.map_err(|_| "History files could not be checked before deletion")?;
            let id = entry.file_name().to_string_lossy().to_string();
            if category == "recordings" && safe_id(&id).is_err() {
                continue;
            }
            managed_deletion_dir(&state.root, category, &id)?;
            if category == "recordings" {
                recordings.push(id);
            }
        }
    }
    Ok(recordings)
}
fn delete_batch_with(
    state: &AsrState,
    items: &[DeleteItem],
    mut remove: impl FnMut(&AsrState, &DeleteItem) -> Result<Vec<String>, String>,
) -> Result<DeleteBatchResult, String> {
    let _processing = deletion_lock(&state.root)?;
    let recordings_before = preflight_deletions(state, items)?;
    let mut result = DeleteBatchResult::default();
    let mut removed = std::collections::HashSet::new();
    for item in items {
        match remove(state,item) {
            Ok(ids)=>{removed.extend(ids);result.deleted.push(DeletedItem{kind:item.kind,id:item.id.clone()});},
            Err(_)=>result.failed.push(DeleteFailure{kind:item.kind,id:item.id.clone(),message:"Some app files could not be deleted. Refresh history and retry this item. Original files and exports are unchanged.".into()})
        }
    }
    // A filesystem failure can follow successful removal of a source import.
    // Include those removals so the UI does not retain stale draft references.
    removed.extend(
        recordings_before
            .into_iter()
            .filter(|id| !state.root.join("recordings").join(id).exists()),
    );
    result.removed_recording_ids = removed.into_iter().collect();
    result.removed_recording_ids.sort();
    Ok(result)
}
fn delete_batch(state: &AsrState, items: &[DeleteItem]) -> Result<DeleteBatchResult, String> {
    delete_batch_with(state, items, |state, item| match item.kind {
        DeleteKind::Transcript => delete_history_locked(
            state,
            &item.id,
            item.revision.ok_or("Transcript revision unavailable")?,
        ),
        DeleteKind::Recording => {
            remove_managed_dir(&state.root, "recordings", &item.id)?;
            Ok(vec![item.id.clone()])
        }
    })
}
#[tauri::command]
pub async fn delete_transcripts(
    app: tauri::AppHandle,
    items: Vec<DeleteItem>,
) -> Result<DeleteBatchResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AsrState>();
        let result = delete_batch(&state, &items)?;
        for item in &result.deleted {
            if item.kind == DeleteKind::Transcript {
                crate::dictation::forget_history(&app, Some(&item.id), &[]);
            }
        }
        crate::dictation::forget_history(&app, None, &result.removed_recording_ids);
        Ok(result)
    })
    .await
    .map_err(|_| "Batch deletion could not finish. Refresh history and retry.")?
}
#[tauri::command]
pub async fn delete_transcript(
    app: tauri::AppHandle,
    state: State<'_, AsrState>,
    id: String,
    revision: u64,
) -> Result<Vec<String>, String> {
    let removed = delete_history(&state, &id, revision)?;
    crate::dictation::forget_history(&app, Some(&id), &removed);
    Ok(removed)
}
#[tauri::command]
pub async fn delete_transcription_recording(
    app: tauri::AppHandle,
    state: State<'_, AsrState>,
    audio_id: String,
) -> Result<(), String> {
    let _processing = deletion_lock(&state.root)?;
    remove_managed_dir(&state.root, "recordings", &audio_id)?;
    crate::dictation::forget_history(&app, None, &[audio_id]);
    Ok(())
}
#[tauri::command]
pub async fn get_transcript(state: State<'_, AsrState>, id: String) -> Result<Transcript, String> {
    store(&state)?
        .get(&id)
        .map_err(|_| "This transcript could not be opened".into())
}
#[derive(Serialize)]
pub struct Audio {
    name: String,
    bytes: Vec<u8>,
    #[serde(rename = "sourceMedia", skip_serializing_if = "Option::is_none")]
    source_media: Option<crate::recording_transfer::Context>,
}
#[tauri::command]
pub async fn get_transcript_audio(state: State<'_, AsrState>, id: String) -> Result<Audio, String> {
    let store = store(&state)?;
    let t = store.get(&id).map_err(|_| "Transcript unavailable")?;
    let path = store.audio_path(&id).map_err(|_| "Recording unavailable")?;
    let bytes = fs::read(&path).map_err(|_| "Recording unavailable")?;
    if bytes.len() > MAX_AUDIO {
        return Err("Recording exceeds the playback limit".into());
    }
    Ok(Audio {
        name: t.name,
        source_media: crate::recording_transfer::read_context(
            path.parent().ok_or("Recording folder unavailable")?,
            &bytes,
        )?,
        bytes,
    })
}
#[tauri::command]
pub async fn list_transcription_recordings(
    state: State<'_, AsrState>,
) -> Result<Vec<Imported>, String> {
    pending_recordings(&state)
}
fn managed_history_file(
    root: &Path,
    category: &str,
    id: &str,
    name: &str,
    max_bytes: u64,
) -> Result<PathBuf, String> {
    safe_id(id)?;
    if name.is_empty()
        || name.len() > 240
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err("Invalid recording filename".into());
    }
    let base = root
        .canonicalize()
        .map_err(|_| "History storage unavailable")?;
    let path = root.join(category).join(id).join(name);
    let metadata = fs::symlink_metadata(&path).map_err(|_| "History file unavailable")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > max_bytes
        || path
            .canonicalize()
            .map_err(|_| "History file unavailable")?
            != base.join(category).join(id).join(name)
    {
        return Err("Unsafe or oversized history file".into());
    }
    Ok(path)
}
fn audio_digest(path: &Path) -> Result<[u8; 32], String> {
    let mut file = fs::File::open(path).map_err(|_| "Recording unavailable")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0usize;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Recording unavailable")?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count).ok_or("Recording too large")?;
        if total > MAX_AUDIO {
            return Err("Recording too large".into());
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}
fn pending_recordings(state: &AsrState) -> Result<Vec<Imported>, String> {
    let recordings = state.root.join("recordings");
    if !recordings.exists() {
        return Ok(vec![]);
    }
    let base = state
        .root
        .canonicalize()
        .map_err(|_| "History storage unavailable")?;
    if recordings
        .canonicalize()
        .map_err(|_| "Recordings unavailable")?
        != base.join("recordings")
    {
        return Err("Unsafe recording storage".into());
    }
    let transcripts = state.root.join("transcripts");
    if transcripts.exists()
        && transcripts
            .canonicalize()
            .map_err(|_| "History unavailable")?
            != base.join("transcripts")
    {
        return Err("Unsafe transcript storage".into());
    }
    let history = store(state)?
        .list()
        .map_err(|_| "Transcript history could not be read")?;
    let mut linked = std::collections::HashSet::new();
    let mut legacy: HashMap<String, Vec<([u8; 32], u64)>> = HashMap::new();
    for transcript in history {
        if let Some(id) = transcript.source_recording_id {
            linked.insert(id);
        } else {
            let path = managed_history_file(
                &state.root,
                "transcripts",
                &transcript.id,
                "audio.wav",
                MAX_AUDIO as u64,
            )?;
            legacy
                .entry(transcript.name)
                .or_default()
                .push((audio_digest(&path)?, transcript.created_at_ms));
        }
    }
    let mut items = vec![];
    for entry in fs::read_dir(recordings).map_err(|_| "Recordings could not be listed")? {
        let entry = entry.map_err(|_| "Recordings could not be listed")?;
        if !entry
            .file_type()
            .map_err(|_| "Recording metadata unavailable")?
            .is_dir()
        {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if safe_id(&id).is_err() || linked.contains(&id) {
            continue;
        }
        let metadata = match managed_history_file(&state.root, "recordings", &id, "name.json", 4096)
        {
            Ok(path) => path,
            Err(_) => continue,
        };
        let Some(name) = fs::read(metadata)
            .ok()
            .and_then(|data| serde_json::from_slice::<String>(&data).ok())
        else {
            continue;
        };
        if !name.to_lowercase().ends_with(".wav") {
            continue;
        }
        let path =
            match managed_history_file(&state.root, "recordings", &id, &name, MAX_AUDIO as u64) {
                Ok(path) => path,
                Err(_) => continue,
            };
        if let Some(digests) = legacy.get(&name) {
            let imported_at = id
                .split('-')
                .next()
                .and_then(|part| part.parse::<u64>().ok())
                .unwrap_or(0);
            let digest = audio_digest(&path)?;
            // A deliberate new import must not disappear because an older legacy
            // transcript happens to contain identical audio.
            if digests
                .iter()
                .any(|(hash, created)| *hash == digest && imported_at <= *created)
            {
                continue;
            }
        }
        items.push(Imported { id, name });
    }
    items.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(items)
}

#[tauri::command]
pub async fn get_transcription_recording_audio(
    state: State<'_, AsrState>,
    audio_id: String,
) -> Result<Audio, String> {
    safe_id(&audio_id)?;
    let dir = state.root.join("recordings").join(audio_id);
    let name: String = serde_json::from_slice(
        &fs::read(dir.join("name.json")).map_err(|_| "Recording unavailable")?,
    )
    .map_err(|_| "Recording unavailable")?;
    if name.contains(['/', '\\']) {
        return Err("Invalid recording".into());
    }
    let bytes = fs::read(dir.join(&name)).map_err(|_| "Recording unavailable")?;
    validate_audio(&bytes)?;
    let source_media = crate::recording_transfer::read_context(&dir, &bytes)?;
    Ok(Audio {
        name,
        bytes,
        source_media,
    })
}
#[tauri::command]
pub async fn correct_transcript_word(
    state: State<'_, AsrState>,
    id: String,
    word_id: usize,
    text: String,
    revision: u64,
) -> Result<Transcript, String> {
    if text.trim().is_empty() || text.chars().count() > 200 {
        return Err("Use 1–200 characters for this correction.".into());
    }
    let end = word_id.checked_add(1).ok_or("Invalid word")?;
    store(&state)?
        .correct(&id, revision, word_id, end, text)
        .map_err(|_| "The correction was not saved. Reopen the latest transcript and retry.".into())
}
fn update(tasks: &Mutex<HashMap<String, Task>>, id: &str, apply: impl FnOnce(&mut Job)) {
    if let Ok(mut tasks) = tasks.lock() {
        if let Some(t) = tasks.get_mut(id) {
            apply(&mut t.job);
        }
    }
}
#[tauri::command]
pub async fn remove_transcript_word(
    state: State<'_, AsrState>,
    id: String,
    word_id: usize,
    revision: u64,
) -> Result<Transcript, String> {
    let end = word_id.checked_add(1).ok_or("Invalid word")?;
    store(&state)?
        .correct(&id, revision, word_id, end, String::new())
        .map_err(|_| "The correction was not saved. Reopen the latest transcript and retry.".into())
}
fn supervise(
    root: &Path,
    audio: &Path,
    job_id: &str,
    cancel: &AtomicBool,
    tasks: &Mutex<HashMap<String, Task>>,
) -> Result<String, String> {
    let (config, python, site) = installation(root)?;
    update(tasks, job_id, |j| {
        j.stage = "Checking installed voice files".into()
    });
    verify_installation(root, &config, cancel)?;
    let work = root.join("jobs").join(job_id);
    let noise = crate::noise_reduction::configuration(root, cancel, false)?;
    fs::create_dir_all(&work).map_err(|_| "Check available storage and retry")?;
    let _cleanup = JobFiles(work.clone());
    let worker = work.join("worker.py");
    fs::write(&worker, include_str!("../../../../workers/asr/worker.py"))
        .map_err(|_| "Could not prepare voice tools")?;
    crate::recognition_bundle::stage_workers(&work)?;
    let output = work.join("result.json");
    crate::noise_reduction::stage_adapter(&work)?;
    if crate::asr_runtime::enabled(root) {
        crate::asr_runtime::execute(
            root,
            (&python, &site, &resources(root).join("bundle")),
            cancel,
            serde_json::json!({"type":"transcribe","audio":audio,"output":output,"noise_reduction":noise}),
            |_| Ok(()),
            |event| {
                if let Some(stage) = event["stage"].as_str() {
                    update(tasks, job_id, |j| j.stage = stage.to_string());
                }
                Ok(())
            },
        )?;
    } else {
        let mut command = crate::background_process::command(python);
        command.arg("-I").arg("-B").arg("-u").arg("-X").arg("utf8").arg("-c").arg("import runpy,sys,pathlib; sys.path.insert(0,sys.argv.pop(1)); worker=sys.argv.pop(1);sys.path.insert(0,str(pathlib.Path(worker).parent)); runpy.run_path(worker,run_name='__main__')")
        .arg(site).arg(&worker).arg("--bundle").arg(resources(root).join("bundle")).arg("--audio").arg(audio).arg("--output").arg(&output)
        .env("ORT_DISABLE_TELEMETRY", "1").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1").env("OMP_NUM_THREADS","8").env("ETHIOPIC_MANAGED_WORKER","1");
        if let Some(config) = &noise {
            let path = work.join("noise-config.json");
            fs::write(
                &path,
                serde_json::to_vec(config)
                    .map_err(|_| "Noise reduction configuration unavailable")?,
            )
            .map_err(|_| "Noise reduction configuration unavailable")?;
            command.arg("--noise-config").arg(path);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                "The private voice runtime could not start. Repair voice tools and retry."
            })?;
        let stdout = child.stdout.take().ok_or("Voice runtime unavailable")?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.split(b'\n') {
                let Ok(line) = line else { break };
                if line.len() > 4096 {
                    break;
                }
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let started = Instant::now();
        let mut failure = None;
        let status = loop {
            if cancel.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(1800) {
                let _ = child.kill();
                let _ = child.wait();
                failure = Some(
                    if cancel.load(Ordering::SeqCst) {
                        "cancelled"
                    } else {
                        "Recognition took too long. Try a shorter recording."
                    }
                    .to_string(),
                );
                break None;
            }
            while let Ok(line) = receiver.try_recv() {
                if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&line) {
                    if event["protocol"] == 1 {
                        if let Some(stage) = event["stage"].as_str() {
                            let stage = stage.to_string();
                            update(tasks, job_id, |j| j.stage = stage);
                        }
                        if event["error"] == "out_of_memory" {
                            failure=Some("Not enough free memory. Close other applications and try a shorter recording.".into());
                        } else if let Some(message) = event["error"]
                            .as_str()
                            .and_then(crate::recognition_bundle::provider_error)
                        {
                            failure = Some(message.into());
                        }
                    }
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    failure = Some(
                        "Recognition stopped unexpectedly. Your recording is safe; please retry."
                            .into(),
                    );
                    break None;
                }
            }
        };
        let _ = reader.join();
        if let Some(error) = failure {
            return Err(error);
        }
        if !status.is_some_and(|s| s.success()) {
            return Err("The voice tools could not finish recognition. Your recording is safe. Retry, or repair voice tools if this repeats.".into());
        }
    }
    let file = fs::File::open(&output)
        .map_err(|_| "Recognition did not produce a transcript. Please retry.")?;
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read transcript")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Transcript output was too large".into());
    }
    let model: transcripts::ModelTranscript = serde_json::from_slice(&bytes)
        .map_err(|_| "Recognition produced an invalid transcript. Please retry.")?;
    crate::noise_reduction::verify_result(&model, noise.as_ref())?;
    validate_model_identity(root, false, &model)?;
    if model.model_version != config.model_version {
        return Err("Model version mismatch. Repair voice tools.".into());
    }
    let store = TranscriptStore::new(root.join("transcripts"))
        .map_err(|_| "Could not open transcript storage")?;
    let source_recording_id = audio
        .parent()
        .filter(|dir| dir.parent() == Some(root.join("recordings").as_path()))
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .filter(|id| safe_id(id).is_ok());
    let source_context = crate::recording_transfer::read_context(
        audio.parent().ok_or("Recording folder unavailable")?,
        &fs::read(audio).map_err(|_| "Recording unavailable")?,
    )?;
    let transcript = if let Some(context) = source_context {
        store.create_transferred(
            audio,
            &model,
            source_recording_id,
            &serde_json::to_vec(&context).map_err(|_| "Recording details unavailable")?,
            None,
        )
    } else {
        source_recording_id.map_or_else(
            || store.create(audio, &model),
            |id| store.create_for_recording(audio, &model, id),
        )
    }
    .map_err(|_| "Could not save transcript. Check storage and retry.")?;
    let _ = fs::remove_dir_all(work);
    Ok(transcript.id)
}
#[tauri::command]
pub fn start_transcription(
    state: State<'_, AsrState>,
    editing: State<'_, crate::voice_editing::EditingState>,
    audio_id: String,
) -> Result<Job, String> {
    safe_id(&audio_id)?;
    installation(&state.root)?;
    let dir = state.root.join("recordings").join(audio_id);
    let name: String = serde_json::from_slice(
        &fs::read(dir.join("name.json")).map_err(|_| "Import a recording first")?,
    )
    .map_err(|_| "Recording metadata unavailable")?;
    if name.contains(['/', '\\']) {
        return Err("Invalid recording".into());
    }
    let audio = dir.join(name);
    validate_audio(&fs::read(&audio).map_err(|_| "Recording unavailable")?)?;
    fs::create_dir_all(&state.root).map_err(|_| "Recognition storage unavailable")?;
    let process_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.root.join("recognition.lock"))
        .map_err(|_| "Recognition queue unavailable")?;
    process_lock
        .try_lock_exclusive()
        .map_err(|_| "Recognition is already running in another studio window.")?;
    editing.release_idle()?;
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "Recognition queue unavailable")?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Another recording is being transcribed. Wait or cancel it first.".into());
    }
    let job = Job {
        id: id(),
        state: "running".into(),
        stage: "Preparing local voice tools".into(),
        error: None,
        transcript_id: None,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    tasks.insert(
        job.id.clone(),
        Task {
            job: job.clone(),
            cancel: cancel.clone(),
        },
    );
    drop(tasks);
    let root = state.root.clone();
    let running = state.running.clone();
    let tasks = state.tasks.clone();
    let job_id = job.id.clone();
    std::thread::spawn(move || {
        let _process_lock = process_lock;
        let result = supervise(&root, &audio, &job_id, &cancel, &tasks);
        update(&tasks, &job_id, |job| match result {
            Ok(id) => {
                job.state = "completed".into();
                job.stage = "Transcript saved".into();
                job.transcript_id = Some(id);
            }
            Err(error) => {
                job.state = if error == "cancelled" {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                job.stage = if error == "cancelled" {
                    "Cancelled. Your recording is saved."
                } else {
                    "Recognition could not finish"
                }
                .into();
                job.error = Some(error);
            }
        });
        running.store(false, Ordering::SeqCst);
    });
    Ok(job)
}
#[tauri::command]
pub fn get_active_transcription_job(state: State<'_, AsrState>) -> Result<Option<Job>, String> {
    let tasks = state.tasks.lock().map_err(|_| "Progress unavailable")?;
    Ok(tasks
        .values()
        .find(|t| t.job.state == "running")
        .map(|t| t.job.clone()))
}
#[tauri::command]
pub fn get_transcription_job(state: State<'_, AsrState>, job_id: String) -> Result<Job, String> {
    state
        .tasks
        .lock()
        .map_err(|_| "Progress unavailable")?
        .get(&job_id)
        .map(|t| t.job.clone())
        .ok_or_else(|| {
            "This job was interrupted by an app restart. Import the recording and retry.".into()
        })
}
#[tauri::command]
pub fn cancel_transcription(state: State<'_, AsrState>, job_id: String) -> Result<(), String> {
    let tasks = state.tasks.lock().map_err(|_| "Progress unavailable")?;
    let task = tasks.get(&job_id).ok_or("Job unavailable")?;
    task.cancel.store(true, Ordering::SeqCst);
    Ok(())
}
pub fn cancel_all(app: &tauri::AppHandle) {
    let state = app.state::<AsrState>();
    if let Ok(tasks) = state.tasks.lock() {
        for task in tasks.values() {
            task.cancel.store(true, Ordering::SeqCst);
        }
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while state.running.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct BatchFixture {
        root: PathBuf,
        state: AsrState,
        saved: Transcript,
        original: PathBuf,
    }
    impl BatchFixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("batch-delete-{}", id()));
            fs::create_dir_all(&root).unwrap();
            let original = root.join("original.wav");
            fs::write(&original, b"original bytes").unwrap();
            let state = AsrState::new(root.join("app"));
            let model = transcripts::ModelTranscript {
                processing: None,
                model: None,
                text: "private text".into(),
                words: vec![],
                model_version: "fixture-v1".into(),
            };
            let saved = store(&state)
                .unwrap()
                .create_for_recording(&original, &model, "source-1")
                .unwrap();
            for id in ["source-1", "draft-1", "draft-2"] {
                let dir = state.root.join("recordings").join(id);
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join("original.wav"), b"original bytes").unwrap();
                fs::write(dir.join("name.json"), br#""original.wav""#).unwrap();
            }
            Self {
                root,
                state,
                saved,
                original,
            }
        }
        fn saved(&self) -> DeleteItem {
            DeleteItem {
                kind: DeleteKind::Transcript,
                id: self.saved.id.clone(),
                revision: Some(self.saved.revision),
            }
        }
        fn draft(id: &str) -> DeleteItem {
            DeleteItem {
                kind: DeleteKind::Recording,
                id: id.into(),
                revision: None,
            }
        }
    }
    impl Drop for BatchFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn batch_deletes_saved_and_drafts_once_including_already_removed_source() {
        let f = BatchFixture::new();
        let items = vec![
            f.saved(),
            BatchFixture::draft("source-1"),
            BatchFixture::draft("draft-1"),
        ];
        let result = delete_batch(&f.state, &items).unwrap();
        assert_eq!(result.deleted.len(), 3);
        assert!(result.failed.is_empty());
        assert_eq!(result.removed_recording_ids, vec!["draft-1", "source-1"]);
        assert!(store(&f.state).unwrap().get(&f.saved.id).is_err());
        assert!(f
            .state
            .root
            .join("recordings/draft-2/original.wav")
            .exists());
        assert_eq!(fs::read(&f.original).unwrap(), b"original bytes");
        let json = serde_json::to_value(result).unwrap();
        assert!(json.get("removedRecordingIds").is_some());
        assert_eq!(json["deleted"][0]["kind"], "transcript");
    }
    #[test]
    fn batch_stale_duplicate_and_unsafe_selections_are_rejected_before_any_deletion() {
        let f = BatchFixture::new();
        let mut stale = f.saved();
        stale.revision = Some(stale.revision.unwrap() + 1);
        for items in [
            vec![BatchFixture::draft("draft-1"), stale],
            vec![
                BatchFixture::draft("draft-1"),
                BatchFixture::draft("draft-1"),
            ],
            vec![
                BatchFixture::draft("draft-1"),
                BatchFixture::draft("../original.wav"),
            ],
            vec![
                BatchFixture::draft("draft-1"),
                DeleteItem {
                    kind: DeleteKind::Transcript,
                    id: f.saved.id.clone(),
                    revision: None,
                },
            ],
        ] {
            assert!(delete_batch(&f.state, &items).is_err());
            assert!(f
                .state
                .root
                .join("recordings/draft-1/original.wav")
                .exists());
            assert!(f
                .state
                .root
                .join("recordings/source-1/original.wav")
                .exists());
            assert!(store(&f.state).unwrap().get(&f.saved.id).is_ok());
        }
        let items = vec![BatchFixture::draft("draft-1"); 5001];
        assert!(delete_batch(&f.state, &items)
            .unwrap_err()
            .contains("5,000"));
    }
    #[test]
    fn batch_holds_the_processing_lock_across_every_item() {
        let f = BatchFixture::new();
        let items = vec![
            BatchFixture::draft("draft-1"),
            BatchFixture::draft("draft-2"),
        ];
        let mut calls = 0;
        let result = delete_batch_with(&f.state, &items, |state, item| {
            calls += 1;
            assert!(deletion_lock(&state.root).is_err());
            remove_managed_dir(&state.root, "recordings", &item.id)?;
            Ok(vec![item.id.clone()])
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(result.deleted.len(), 2);
        assert!(deletion_lock(&f.state.root).is_ok());
        assert_eq!(fs::read(&f.original).unwrap(), b"original bytes");
    }
    #[test]
    fn batch_reports_partial_failures_and_does_not_hide_successful_source_removal() {
        let f = BatchFixture::new();
        let items = vec![f.saved(), BatchFixture::draft("draft-1")];
        let result = delete_batch_with(&f.state, &items, |state, item| {
            if item.kind == DeleteKind::Transcript {
                remove_managed_dir(&state.root, "recordings", "source-1")?;
                Err("simulated filesystem failure with private content".into())
            } else {
                remove_managed_dir(&state.root, "recordings", &item.id)?;
                Ok(vec![item.id.clone()])
            }
        })
        .unwrap();
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0].id, "draft-1");
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].id, f.saved.id);
        assert!(!result.failed[0].message.contains("private content"));
        assert_eq!(result.removed_recording_ids, vec!["draft-1", "source-1"]);
        assert!(store(&f.state).unwrap().get(&f.saved.id).is_ok());
        assert_eq!(fs::read(&f.original).unwrap(), b"original bytes");
    }
    #[cfg(unix)]
    #[test]
    fn batch_preflight_rejects_symlinks_and_preserves_all_selected_data() {
        use std::os::unix::fs::symlink;
        let f = BatchFixture::new();
        let dir = f.state.root.join("recordings/draft-2");
        symlink(&f.original, dir.join("redirected.wav")).unwrap();
        assert!(delete_batch(
            &f.state,
            &[
                BatchFixture::draft("draft-1"),
                f.saved(),
                BatchFixture::draft("draft-2")
            ]
        )
        .is_err());
        assert!(f
            .state
            .root
            .join("recordings/draft-1/original.wav")
            .exists());
        assert!(store(&f.state).unwrap().get(&f.saved.id).is_ok());
        assert_eq!(fs::read(&f.original).unwrap(), b"original bytes");
    }
    #[test]
    fn pending_recordings_deduplicate_legacy_audio_but_explicit_links_preserve_new_imports() {
        let root = std::env::temp_dir().join(format!("pending-recordings-{}", id()));
        fs::create_dir_all(&root).unwrap();
        let state = AsrState::new(root.join("app"));
        let store = store(&state).unwrap();
        let model = transcripts::ModelTranscript {
            processing: None,
            model: None,
            text: "fixture".into(),
            words: vec![],
            model_version: "fixture-v1".into(),
        };
        let legacy = root.join("legacy.wav");
        fs::write(&legacy, b"legacy audio").unwrap();
        store.create(&legacy, &model).unwrap();
        let linked = root.join("linked.wav");
        fs::write(&linked, b"linked audio").unwrap();
        store
            .create_for_recording(&linked, &model, "linked-1")
            .unwrap();
        for (id, name, audio) in [
            ("legacy-done", "legacy.wav", b"legacy audio".as_slice()),
            (
                "18446744073709551615-new",
                "legacy.wav",
                b"legacy audio".as_slice(),
            ),
            (
                "same-name-new-bytes",
                "legacy.wav",
                b"different audio".as_slice(),
            ),
            ("linked-1", "linked.wav", b"linked audio".as_slice()),
            (
                "new-intentional-import",
                "linked.wav",
                b"linked audio".as_slice(),
            ),
        ] {
            let dir = state.root.join("recordings").join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(name), audio).unwrap();
            fs::write(dir.join("name.json"), serde_json::to_vec(name).unwrap()).unwrap();
        }
        let items = pending_recordings(&state).unwrap();
        let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "same-name-new-bytes",
                "new-intentional-import",
                "18446744073709551615-new"
            ]
        );
        assert!(state
            .root
            .join("recordings/legacy-done/legacy.wav")
            .exists());
        assert!(state.root.join("recordings/linked-1/linked.wav").exists());
        assert_eq!(fs::read(legacy).unwrap(), b"legacy audio");
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn pending_recordings_never_follow_external_audio_or_metadata_links() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("pending-paths-{}", id()));
        fs::create_dir_all(&root).unwrap();
        let state = AsrState::new(root.join("app"));
        let external = root.join("external");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("audio.wav"), b"external bytes").unwrap();
        fs::write(external.join("name.json"), br#""audio.wav""#).unwrap();
        let imports = state.root.join("recordings");
        fs::create_dir_all(&imports).unwrap();
        symlink(&external, imports.join("redirected-dir")).unwrap();
        let audio = imports.join("redirected-audio");
        fs::create_dir(&audio).unwrap();
        fs::write(audio.join("name.json"), br#""audio.wav""#).unwrap();
        symlink(external.join("audio.wav"), audio.join("audio.wav")).unwrap();
        let metadata = imports.join("redirected-metadata");
        fs::create_dir(&metadata).unwrap();
        symlink(external.join("name.json"), metadata.join("name.json")).unwrap();
        let escape = imports.join("metadata-escape");
        fs::create_dir(&escape).unwrap();
        fs::write(escape.join("name.json"), br#""../../external/audio.wav""#).unwrap();
        assert!(pending_recordings(&state).unwrap().is_empty());
        assert_eq!(
            fs::read(external.join("audio.wav")).unwrap(),
            b"external bytes"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn deleting_linked_project_preserves_separately_imported_identical_audio() {
        let root = std::env::temp_dir().join(format!("linked-delete-{}", id()));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("same.wav");
        fs::write(&original, b"same bytes").unwrap();
        let state = AsrState::new(root.join("app"));
        let model = transcripts::ModelTranscript {
            processing: None,
            model: None,
            text: "text".into(),
            words: vec![],
            model_version: "v0".into(),
        };
        let transcript = store(&state)
            .unwrap()
            .create_for_recording(&original, &model, "source-1")
            .unwrap();
        for id in ["source-1", "source-2"] {
            let dir = state.root.join("recordings").join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("same.wav"), b"same bytes").unwrap();
            fs::write(dir.join("name.json"), br#""same.wav""#).unwrap();
        }
        assert_eq!(
            delete_history(&state, &transcript.id, 1).unwrap(),
            vec!["source-1"]
        );
        assert!(state.root.join("recordings/source-2/same.wav").exists());
        assert_eq!(fs::read(original).unwrap(), b"same bytes");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn deleting_history_removes_managed_copies_and_refs_but_keeps_original() {
        let root = std::env::temp_dir().join(format!("history-delete-{}", id()));
        fs::create_dir_all(&root).unwrap();
        let original = root.join("original.wav");
        fs::write(&original, b"original bytes").unwrap();
        let state = AsrState::new(root.join("app"));
        let store = store(&state).unwrap();
        let model = transcripts::ModelTranscript {
            processing: None,
            model: None,
            text: "private transcript".into(),
            words: vec![],
            model_version: "fixture-v1".into(),
        };
        let first = store.create(&original, &model).unwrap();
        let other = store.create(&original, &model).unwrap();
        let imported = state.root.join("recordings/import-1");
        fs::create_dir_all(&imported).unwrap();
        fs::write(imported.join("original.wav"), b"original bytes").unwrap();
        fs::write(imported.join("name.json"), br#""original.wav""#).unwrap();
        let unrelated = state.root.join("recordings/import-2");
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(unrelated.join("original.wav"), b"different bytes").unwrap();
        fs::write(unrelated.join("name.json"), br#""original.wav""#).unwrap();
        let work = state.root.join("jobs/job-old");
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("result.json"), b"private worker output").unwrap();
        state.tasks.lock().unwrap().insert(
            "done".into(),
            Task {
                job: Job {
                    id: "done".into(),
                    state: "completed".into(),
                    stage: "saved".into(),
                    error: None,
                    transcript_id: Some(first.id.clone()),
                },
                cancel: Arc::new(AtomicBool::new(false)),
            },
        );
        let processing = deletion_lock(&state.root).unwrap();
        assert!(delete_history(&state, &first.id, first.revision).is_err());
        drop(processing);
        assert!(delete_history(&state, &first.id, first.revision + 1).is_err());
        assert!(imported.exists());
        assert_eq!(
            delete_history(&state, &first.id, first.revision).unwrap(),
            vec!["import-1"]
        );
        assert!(!imported.exists());
        assert!(unrelated.exists());
        assert!(!work.exists());
        assert!(state.tasks.lock().unwrap().is_empty());
        assert_eq!(fs::read(&original).unwrap(), b"original bytes");
        assert!(store.get(&first.id).is_err());
        assert!(store.get(&other.id).is_ok());
        assert!(delete_history(&state, &first.id, first.revision)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn deleting_imports_rejects_redirects_and_never_follows_metadata_paths() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("history-path-delete-{}", id()));
        fs::create_dir_all(root.join("recordings")).unwrap();
        let external = root.join("external");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("original.wav"), b"preserve").unwrap();
        symlink(&external, root.join("recordings/redirect")).unwrap();
        assert!(remove_managed_dir(&root, "recordings", "redirect").is_err());
        assert!(remove_managed_dir(&root, "recordings", "../external").is_err());
        fs::remove_file(root.join("recordings/redirect")).unwrap();
        fs::create_dir_all(root.join("recordings/owned")).unwrap();
        fs::write(
            root.join("recordings/owned/name.json"),
            br#""../../external/original.wav""#,
        )
        .unwrap();
        remove_managed_dir(&root, "recordings", "owned").unwrap();
        assert_eq!(
            fs::read(external.join("original.wav")).unwrap(),
            b"preserve"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_unsafe_ids_and_non_wav() {
        assert!(safe_id("../escape").is_err());
        assert!(safe_id("valid-123").is_ok());
        assert!(validate_audio(&[0; 100]).is_err());
    }
    #[test]
    fn validates_pcm_wav() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut wav = hound::WavWriter::new(
                &mut bytes,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..1600 {
                wav.write_sample(0i16).unwrap();
            }
            wav.finalize().unwrap();
        }
        assert!(validate_audio(bytes.get_ref()).is_ok());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    // Requires only local prepared resources, never a network or training repository.
    #[test]
    #[ignore = "requires locally installed ASR resources and a WAV validation recording"]
    fn real_offline_worker_and_correction_roundtrip() {
        let root =
            PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_ROOT").expect("test resource root"));
        let audio = PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_WAV").expect("validation WAV"));
        validate_audio(&fs::read(&audio).unwrap()).unwrap();
        let tasks = Mutex::new(HashMap::new());
        let transcript_id =
            supervise(&root, &audio, &id(), &AtomicBool::new(false), &tasks).unwrap();
        let store = TranscriptStore::new(root.join("transcripts")).unwrap();
        let before = store.get(&transcript_id).unwrap();
        assert_eq!(
            before.model_version,
            installation(&root).unwrap().0.model_version
        );
        assert!(!before.words.is_empty());
        let word = &before.words[0];
        let after = store
            .correct(&before.id, before.revision, 0, 1, "ሙከራ".into())
            .unwrap();
        assert_eq!(after.words[0].start, word.start);
        assert_eq!(after.words[0].end, word.end);
        assert_eq!(after.model_version, before.model_version);
        assert!(after.corrections[0].human_checked);
        assert_eq!(store.get(&after.id).unwrap().words[0].corrected, "ሙከራ");
        assert_eq!(
            fs::read(store.audio_path(&after.id).unwrap()).unwrap(),
            fs::read(audio).unwrap()
        );
        println!(
            "Verified real model {}, {} timed words, correction revision {}",
            before.model_version,
            before.words.len(),
            after.revision
        );
    }

    #[test]
    #[ignore = "requires locally installed ASR resources"]
    fn cancels_running_worker_during_model_loading() {
        let root = PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_ROOT").unwrap());
        let audio = PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_WAV").unwrap());
        let job_id = id();
        let cancel = Arc::new(AtomicBool::new(false));
        let tasks = Arc::new(Mutex::new(HashMap::from([(
            job_id.clone(),
            Task {
                job: Job {
                    id: job_id.clone(),
                    state: "running".into(),
                    stage: "queued".into(),
                    error: None,
                    transcript_id: None,
                },
                cancel: cancel.clone(),
            },
        )])));
        let observer = tasks.clone();
        let flag = cancel.clone();
        let watched_id = job_id.clone();
        let monitor = std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(30) {
                let loading = observer.lock().unwrap()[&watched_id].job.stage
                    == "Loading Amharic voice tools";
                if loading {
                    flag.store(true, Ordering::SeqCst);
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            flag.store(true, Ordering::SeqCst);
            false
        });
        let result = supervise(&root, &audio, &job_id, &cancel, &tasks);
        assert!(
            monitor.join().unwrap(),
            "worker must reach real model loading before cancellation"
        );
        assert_eq!(result.unwrap_err(), "cancelled");
        assert!(!root.join("jobs").join(job_id).exists());
        assert!(audio.exists());
    }

    #[test]
    #[ignore = "requires locally installed ASR resources"]
    fn cancelled_job_does_not_create_transcript() {
        let root = PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_ROOT").unwrap());
        let audio = PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_WAV").unwrap());
        let store = TranscriptStore::new(root.join("transcripts")).unwrap();
        let before = store.list().unwrap().len();
        let result = supervise(
            &root,
            &audio,
            &id(),
            &AtomicBool::new(true),
            &Mutex::new(HashMap::new()),
        );
        assert_eq!(result.unwrap_err(), "cancelled");
        assert_eq!(store.list().unwrap().len(), before);
    }
}

#[cfg(test)]
mod managed_bundle_tests {
    use super::*;
    use serde_json::json;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Fixture {
        let fixture = Fixture(std::env::temp_dir().join(format!("ethiopic-recognition-{}", id())));
        let active = resources(&fixture.0);
        fs::create_dir_all(active.join("bundle")).unwrap();
        let provider = |variant: &str, model: &str| json!({"variant":variant,"backend":"onnx","folder":variant,"device":"cpu","dtype":"float32","model_version":"v1.0","model":model,"threads":4});
        let selection = json!({"schema_version":1,
            "transcription":provider("asr-wav2vec_ctc-1b-amh-int8_onnx","amharic-ctc"),
            "dictation":provider("asr-wav2vec_ctc-300m-amh-int8_onnx","amharic-ctc-300m")});
        let bytes = serde_json::to_vec(&selection).unwrap();
        fs::write(active.join("bundle/recognition.json"), &bytes).unwrap();
        let bundle = crate::recognition_bundle::read(&active.join("bundle"))
            .unwrap()
            .unwrap();
        let manifest = crate::voice_editing::setup_manifest(&fixture.0).unwrap();
        let mut files = bundle.required_files(&manifest).unwrap();
        for path in files.keys() {
            let target = active.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(
                target,
                b"test-only placeholder; full checksum verification must reject it",
            )
            .unwrap();
        }
        for provider in [&bundle.transcription, &bundle.dictation] {
            let relative = format!("bundle/{}/variant.json", provider.folder);
            let metadata = serde_json::to_vec(manifest.find(&provider.variant).unwrap()).unwrap();
            fs::write(active.join(&relative), &metadata).unwrap();
            files.insert(relative, format!("{:x}", Sha256::digest(&metadata)));
        }
        files.insert(
            "bundle/recognition.json".into(),
            format!("{:x}", Sha256::digest(&bytes)),
        );
        for path in [
            "runtime/python/python.exe",
            "runtime/site-packages/numpy/__init__.py",
        ] {
            let target = active.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, b"test").unwrap();
            files.insert(path.into(), format!("{:x}", Sha256::digest(b"test")));
        }
        let entries: Vec<_> = files
            .iter()
            .map(|(path, sha)| json!({"path":path,"sha256":sha}))
            .collect();
        fs::write(
            active.join("installation.json"),
            serde_json::to_vec(&json!({
            "schema_version":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "python":"runtime/python/python.exe","site_packages":"runtime/site-packages",
            "model_version":"v1.0","files":entries}))
            .unwrap(),
        )
        .unwrap();
        fixture
    }
    #[test]
    fn installed_provider_selection_requires_authoritative_file_hashes_and_both_modes() {
        let fixture = fixture();
        let active = resources(&fixture.0);
        let (config, _, _) = installation(&fixture.0).unwrap();
        assert!(
            verify_installation(&fixture.0, &config, &AtomicBool::new(false)).is_err(),
            "Full verification must reject placeholder model bytes"
        );
        let bytes = fs::read(active.join("installation.json")).unwrap();
        let mut metadata: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let entry = metadata["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["path"] == "bundle/asr-wav2vec_ctc-300m-amh-int8_onnx/asr.int8.onnx")
            .unwrap();
        entry["sha256"] = json!("0".repeat(64));
        fs::write(
            active.join("installation.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        assert!(
            installation(&fixture.0).is_err(),
            "Live-model integrity cannot be omitted or replaced"
        );
        fs::write(active.join("installation.json"), bytes).unwrap();
        fs::remove_file(active.join("bundle/asr-wav2vec_ctc-300m-amh-int8_onnx/asr.int8.onnx"))
            .unwrap();
        assert!(installation(&fixture.0).is_err());
    }
    #[test]
    fn batch_and_live_transcripts_preserve_their_distinct_model_families() {
        let fixture = fixture();
        let mut result = transcripts::ModelTranscript {
            processing: None,
            model: Some("amharic-ctc".into()),
            text: String::new(),
            words: vec![],
            model_version: "v1.0".into(),
        };
        validate_model_identity(&fixture.0, false, &result).unwrap();
        assert!(validate_model_identity(&fixture.0, true, &result).is_err());
        result.model = Some("amharic-ctc-300m".into());
        validate_model_identity(&fixture.0, true, &result).unwrap();
        assert!(validate_model_identity(&fixture.0, false, &result).is_err());
        result.model_version = "v0".into();
        assert!(validate_model_identity(&fixture.0, true, &result).is_err());
    }

    #[test]
    fn model_metadata_must_match_its_release_even_when_its_pack_hash_is_valid() {
        let fixture = fixture();
        let active = resources(&fixture.0);
        let relative = "bundle/asr-wav2vec_ctc-300m-amh-int8_onnx/variant.json";
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&fs::read(active.join(relative)).unwrap()).unwrap();
        metadata["model"] = json!("amharic-ctc");
        let bytes = serde_json::to_vec(&metadata).unwrap();
        fs::write(active.join(relative), &bytes).unwrap();
        let mut installation: serde_json::Value =
            serde_json::from_slice(&fs::read(active.join("installation.json")).unwrap()).unwrap();
        let file = installation["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|file| file["path"] == relative)
            .unwrap();
        file["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
        fs::write(
            active.join("installation.json"),
            serde_json::to_vec(&installation).unwrap(),
        )
        .unwrap();
        let config: Installation = serde_json::from_value(installation).unwrap();
        let bundle = crate::recognition_bundle::read(&active.join("bundle"))
            .unwrap()
            .unwrap();
        let files = config
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.sha256.as_str()))
            .collect();
        let manifest = crate::voice_editing::setup_manifest(&fixture.0).unwrap();
        assert!(bundle.validate_files(&active.join("bundle"), &manifest, &files).is_err(),
            "Staged installs must reject metadata with a valid archive checksum but the wrong model family");
    }
}
