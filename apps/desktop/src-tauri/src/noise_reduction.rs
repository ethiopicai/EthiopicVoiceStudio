//! Optional, pinned recognition preprocessor. A separate wheel overlay leaves the
//! installed ASR interpreter/packages untouched. Setup alone may access the network.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

const CATALOG: &str = include_str!("../../resources/noise-reduction.json");
const REPAIR: &str = "Noise reduction needs setup or repair. Open Settings, or turn it off to use ordinary recognition.";
const MODEL: &str = "denoiser_realtime.safetensors";
const REVISION: &str = "9b0285b3fe8403aa8e2f1dbb2165a80a4294c970";
const MODEL_HASH: &str = "76598f05523c0669e76562a8b2f31c9d395fca316e80c907ee11775e7497ae0f";
static JOB: Mutex<Option<Status>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);
static PREFS: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    release: String,
    os: String,
    arch: String,
    files: Vec<File>,
}
#[derive(Deserialize)]
struct File {
    name: String,
    url: String,
    bytes: u64,
    sha256: String,
    kind: String,
}
#[derive(Deserialize, Serialize)]
struct Integrity {
    path: String,
    sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Installation {
    release: String,
    base_identity: String,
    device: String,
    live_ready: bool,
    real_time_factor: f64,
    files: Vec<Integrity>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: String,
    pub supported: bool,
    pub ready: bool,
    pub enabled: bool,
    pub stage: String,
    pub received: u64,
    pub total: u64,
    pub error: Option<String>,
    pub device: Option<String>,
    pub live_ready: bool,
}

fn catalog() -> Catalog {
    serde_json::from_str(CATALOG).expect("bundled noise reduction catalog")
}
fn base_identity(root: &Path) -> Result<String, String> {
    let data = fs::read(root.join("asr/active/installation.json"))
        .map_err(|_| "Set up speech recognition before noise reduction.")?;
    Ok(format!("{:x}", Sha256::digest(data)))
}
pub fn enabled(root: &Path) -> bool {
    fs::read(root.join("noise-reduction-enabled.json"))
        .ok()
        .and_then(|v| serde_json::from_slice::<bool>(&v).ok())
        .unwrap_or(false)
}
fn save_enabled(root: &Path, value: bool) -> Result<(), String> {
    let _lock = PREFS
        .lock()
        .map_err(|_| "Could not save noise reduction preference")?;
    fs::create_dir_all(root).map_err(|_| "Check free storage")?;
    let pending = root.join("noise-reduction-enabled.pending");
    let mut file =
        fs::File::create(&pending).map_err(|_| "Could not save noise reduction preference")?;
    file.write_all(if value { b"true" } else { b"false" })
        .and_then(|_| file.sync_all())
        .map_err(|_| "Could not save noise reduction preference")?;
    fs::rename(pending, root.join("noise-reduction-enabled.json"))
        .map_err(|_| "Could not save noise reduction preference".into())
}
fn installation(root: &Path) -> Result<Installation, String> {
    let base = root.join("noise-reduction/active");
    let data = fs::read(base.join("installation.json")).map_err(|_| REPAIR)?;
    if data.len() > 4 * 1024 * 1024 {
        return Err(REPAIR.into());
    }
    let info: Installation = serde_json::from_slice(&data).map_err(|_| REPAIR)?;
    if info.release != catalog().release
        || info.base_identity != base_identity(root)?
        || !matches!(info.device.as_str(), "cpu" | "mps")
        || !info.real_time_factor.is_finite()
        || info.real_time_factor < 0.0
        || info.files.len() > 10000
        || !info
            .files
            .iter()
            .any(|f| f.path == MODEL && f.sha256 == MODEL_HASH)
        || !base
            .join("site-packages/inworld/voice_isolation/isolator.py")
            .is_file()
    {
        return Err(REPAIR.into());
    }
    Ok(info)
}
fn status(root: &Path) -> Status {
    let catalog = catalog();
    let supported = catalog.os == std::env::consts::OS && catalog.arch == std::env::consts::ARCH;
    let info = installation(root).ok();
    let mut result = Status {
        state: if info.is_some() {
            "ready"
        } else {
            "not_installed"
        }
        .into(),
        supported,
        ready: info.is_some(),
        enabled: enabled(root),
        stage: if !supported {
            "Noise reduction is not packaged for this computer yet."
        } else if info.is_some() {
            "Noise reduction is ready on this computer."
        } else {
            "Optional download for noisy recordings. Your original audio stays unchanged."
        }
        .into(),
        received: 0,
        total: catalog.files.iter().map(|f| f.bytes).sum(),
        error: None,
        device: info.as_ref().map(|v| v.device.clone()),
        live_ready: info.as_ref().is_some_and(|v| v.live_ready),
    };
    if let Ok(job) = JOB.lock() {
        if let Some(job) = job.as_ref() {
            result.state = job.state.clone();
            result.stage = job.stage.clone();
            result.received = job.received;
            result.error = job.error.clone();
        }
    }
    result
}
#[tauri::command]
pub fn get_noise_reduction_status(app: tauri::AppHandle) -> Result<Status, String> {
    Ok(status(&app.path().app_data_dir().map_err(|_| REPAIR)?))
}
#[tauri::command]
pub fn set_noise_reduction_enabled(app: tauri::AppHandle, enabled: bool) -> Result<Status, String> {
    let root = app.path().app_data_dir().map_err(|_| REPAIR)?;
    if enabled {
        installation(&root)?;
    }
    save_enabled(&root, enabled)?;
    Ok(status(&root))
}
fn progress(stage: &str, received: u64) {
    if let Ok(mut job) = JOB.lock() {
        if let Some(job) = job.as_mut() {
            job.stage = stage.into();
            job.received = received;
        }
    }
}
#[tauri::command]
pub fn start_noise_reduction_setup(
    app: tauri::AppHandle,
    approved_plan: String,
) -> Result<Status, String> {
    crate::download_setup::require(&app, "noise", &approved_plan)?;
    let root = app.path().app_data_dir().map_err(|_| REPAIR)?;
    let view = status(&root);
    if !view.supported {
        return Err(view.stage);
    }
    base_identity(&root)?;
    let mut job = JOB.lock().map_err(|_| REPAIR)?;
    if job.as_ref().is_some_and(|v| v.state == "installing") {
        return Ok(status_without_lock(view, job.as_ref().unwrap()));
    }
    CANCEL.store(false, Ordering::SeqCst);
    let mut initial = view;
    initial.state = "installing".into();
    initial.stage = "Downloading noise reduction…".into();
    initial.error = None;
    *job = Some(initial.clone());
    std::thread::spawn(move || {
        let result = install(&root, &CANCEL);
        if let Ok(mut job) = JOB.lock() {
            if let Some(job) = job.as_mut() {
                match result {
                    Ok(()) => {
                        job.state = "ready".into();
                        job.stage = "Noise reduction is ready. Turn it on when needed.".into();
                        job.received = job.total;
                    }
                    Err(error) => {
                        job.state = if CANCEL.load(Ordering::SeqCst) {
                            "paused"
                        } else {
                            "failed"
                        }
                        .into();
                        job.stage = "Setup stopped. Downloaded progress is kept for retry.".into();
                        job.error = if CANCEL.load(Ordering::SeqCst) {
                            None
                        } else {
                            Some(error)
                        };
                    }
                }
            }
        }
    });
    Ok(initial)
}
pub(crate) fn download_quote(root: &Path) -> Result<crate::download_setup::Plan, String> {
    use crate::download_setup::{Item, Plan};
    let view = status(root);
    let available = view.supported && base_identity(root).is_ok();
    let items = catalog()
        .files
        .into_iter()
        .map(|file| Item {
            name: file.name,
            purpose: if file.kind == "model" {
                "Background noise reduction model"
            } else {
                "Noise reduction engine dependency"
            }
            .into(),
            bytes: file.bytes,
            url: file.url,
            sha256: file.sha256,
        })
        .collect();
    Plan::new(
        "noise",
        "Background noise reduction",
        available,
        if available {
            "".into()
        } else {
            "Set up compatible speech recognition tools before adding noise reduction.".into()
        },
        items,
    )
}
fn status_without_lock(mut result: Status, job: &Status) -> Status {
    result.state = job.state.clone();
    result.stage = job.stage.clone();
    result.received = job.received;
    result.error = job.error.clone();
    result
}
#[tauri::command]
pub fn pause_noise_reduction_setup() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Capture this before starting a job; preference changes affect the next job.
pub fn configuration(
    root: &Path,
    cancel: &AtomicBool,
    live: bool,
) -> Result<Option<Value>, String> {
    if !enabled(root) {
        return Ok(None);
    }
    let info = installation(root)?;
    if live && !info.live_ready {
        return Err("Noise reduction is too slow for live words on this computer. Choose Transcribe after stopping, or turn noise reduction off.".into());
    }
    let base = root.join("noise-reduction/active");
    let entries: Vec<_> = info
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    crate::resource_verification::verify(
        &base,
        &entries,
        &root.join("noise-reduction/verification-cache.json"),
        cancel,
    )?;
    Ok(Some(
        json!({"site":base.join("site-packages"),"checkpoint":base.join(MODEL),"device":info.device,"revision":REVISION}),
    ))
}
pub fn stage_adapter(directory: &Path) -> Result<(), String> {
    fs::write(
        directory.join("noise_reduction.py"),
        include_str!("../../../../workers/asr/noise_reduction.py"),
    )
    .map_err(|_| "Could not prepare noise reduction".into())
}
pub fn verify_result(
    model: &transcripts::ModelTranscript,
    config: Option<&Value>,
) -> Result<(), String> {
    match (&model.processing,config) {
        (None,None)=>Ok(()),
        (Some(p),Some(config)) if p.noise_reduction.provider=="inworld-voice-isolator-v1-realtime"
            && p.noise_reduction.revision==REVISION && p.noise_reduction.sha256==MODEL_HASH
            && p.noise_reduction.lookahead_ms==20 && p.noise_reduction.adapter_version==1
            && config["device"].as_str()==Some(p.noise_reduction.device.as_str())=>Ok(()),
        _=>Err("Recognition noise reduction metadata did not match this recording. Your original audio is safe; retry.".into())
    }
}
fn install(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let (_, python, site, _) = crate::asr::verified_dictation_runtime(root, cancel)?;
    let base = root.join("noise-reduction");
    fs::create_dir_all(&base).map_err(|_| "Check free storage")?;
    let pending = base.join("pending");
    // Recover a prior installation if a previous activation was interrupted.
    if !base.join("active").exists() && base.join("previous").is_dir() {
        fs::rename(base.join("previous"), base.join("active")).map_err(|_| REPAIR)?;
    }
    if pending.exists() {
        fs::remove_dir_all(&pending).map_err(|_| REPAIR)?;
    }
    fs::create_dir_all(&pending).map_err(|_| "Check free storage")?;
    let mut wheels = Vec::new();
    let mut received = 0;
    for file in catalog().files {
        let mut stalled = 0;
        let partial = base.join("cache").join(&file.sha256).join("download.part");
        let downloaded = loop {
            let before = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
            let attempt = resource_packs::download_inference_file(
                &file.url,
                &file.sha256,
                file.bytes,
                &base.join("cache"),
                cancel,
                |amount| progress("Downloading noise reduction…", received + amount),
            );
            match attempt {
                Ok(path) => break path,
                Err(error) => {
                    if cancel.load(Ordering::SeqCst) {
                        return Err("Setup paused".into());
                    }
                    let after = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
                    stalled = if after > before { 0 } else { stalled + 1 };
                    if stalled >= 3 {
                        return Err(error);
                    }
                    progress(
                        "Connection interrupted. Resuming download…",
                        received + after,
                    );
                    for _ in 0..20 {
                        if cancel.load(Ordering::SeqCst) {
                            return Err("Setup paused".into());
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        };
        if file.kind == "model" {
            fs::copy(&downloaded, pending.join(&file.name)).map_err(|_| "Check free storage")?;
        } else {
            wheels.push(downloaded);
        }
        received += file.bytes;
    }
    progress("Waiting for current voice task to finish…", received);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("recognition.lock"))
        .map_err(|_| REPAIR)?;
    let wait = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("Setup paused".into());
        }
        if lock.try_lock_exclusive().is_ok() {
            break;
        }
        if wait.elapsed() > Duration::from_secs(1800) {
            return Err("Finish your voice task and retry setup.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    crate::asr_runtime::release_for_setup()?;
    progress(
        "Preparing and checking noise reduction on this computer…",
        received,
    );
    stage_adapter(&base)?;
    let script = base.join("noise_setup.py");
    fs::write(
        &script,
        include_str!("../../../../workers/asr/noise_setup.py"),
    )
    .map_err(|_| REPAIR)?;
    let output = base.join("probe.json");
    let mut child=Command::new(python).args(["-I","-B","-u","-c","import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')"])
        .arg(site).arg(&base).arg(script).arg("--destination").arg(&pending).arg("--wheels").args(wheels)
        .env("ETHIOPIC_MANAGED_WORKER","1").env("HF_HUB_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1").env("TRANSFORMERS_OFFLINE","1")
        .stdin(Stdio::piped()).stdout(Stdio::from(fs::File::create(&output).map_err(|_|REPAIR)?)).stderr(Stdio::null()).spawn().map_err(|_|REPAIR)?;
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Noise reduction setup stopped. Retry when ready.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err("Noise reduction could not run on this computer. Ordinary recognition is still available.".into());
                }
                break;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(REPAIR.into());
            }
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let mut data = Vec::new();
    fs::File::open(&output)
        .map_err(|_| REPAIR)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|_| REPAIR)?;
    if data.len() > 4 * 1024 * 1024 {
        return Err(REPAIR.into());
    }
    let probe: Value = serde_json::from_slice(&data).map_err(|_| REPAIR)?;
    let info = Installation {
        release: catalog().release,
        base_identity: base_identity(root)?,
        device: probe["device"].as_str().ok_or(REPAIR)?.into(),
        live_ready: probe["liveReady"].as_bool().ok_or(REPAIR)?,
        real_time_factor: probe["realTimeFactor"].as_f64().ok_or(REPAIR)?,
        files: serde_json::from_value(probe["files"].clone()).map_err(|_| REPAIR)?,
    };
    if !matches!(info.device.as_str(), "cpu" | "mps")
        || !info.real_time_factor.is_finite()
        || !info
            .files
            .iter()
            .any(|f| f.path == MODEL && f.sha256 == MODEL_HASH)
    {
        return Err(REPAIR.into());
    }
    let receipt = serde_json::to_vec(&info).map_err(|_| REPAIR)?;
    fs::write(pending.join("installation.json"), receipt).map_err(|_| REPAIR)?;
    let active = base.join("active");
    let backup = base.join("previous");
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|_| REPAIR)?;
    }
    if active.exists() {
        fs::rename(&active, &backup).map_err(|_| REPAIR)?;
    }
    if fs::rename(&pending, &active).is_err() {
        if backup.exists() {
            let _ = fs::rename(&backup, &active);
        }
        return Err(REPAIR.into());
    }
    let _ = fs::remove_dir_all(backup);
    let _ = fs::remove_file(output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_preprocessing_is_off_without_setup_and_metadata_must_match_request() {
        let root = std::env::temp_dir().join(format!("noise-off-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert!(!enabled(&root));
        assert!(configuration(&root, &AtomicBool::new(false), true)
            .unwrap()
            .is_none());
        save_enabled(&root, true).unwrap();
        assert!(configuration(&root, &AtomicBool::new(false), false).is_err());
        save_enabled(&root, false).unwrap();
        let mut model: transcripts::ModelTranscript =
            serde_json::from_value(json!({"text":"test","words":[],"model_version":"v0.2"}))
                .unwrap();
        let config = json!({"device":"mps"});
        assert!(verify_result(&model, None).is_ok());
        assert!(verify_result(&model, Some(&config)).is_err());
        model.processing=Some(serde_json::from_value(json!({"noise_reduction":{"provider":"inworld-voice-isolator-v1-realtime","revision":REVISION,"sha256":MODEL_HASH,"adapter_version":1,"lookahead_ms":20,"device":"mps"}})).unwrap());
        assert!(verify_result(&model, Some(&config)).is_ok());
        assert!(verify_result(&model, None).is_err());
        model.processing.as_mut().unwrap().noise_reduction.device = "cpu".into();
        assert!(verify_result(&model, Some(&config)).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pinned_catalog_has_one_model_and_matching_private_runtime_dependencies() {
        let c = catalog();
        assert_eq!(c.os, "macos");
        assert_eq!(c.arch, "aarch64");
        assert_eq!(c.files.len(), 6);
        let model = c.files.iter().find(|f| f.kind == "model").unwrap();
        assert_eq!(model.name, MODEL);
        assert_eq!(model.sha256, MODEL_HASH);
        assert!(model.url.contains(REVISION));
        assert_eq!(c.files.iter().filter(|f| f.kind == "wheel").count(), 5);
        assert!(c
            .files
            .iter()
            .any(|f| f.name.starts_with("torchaudio-2.9.1-cp311")));
    }
    #[test]
    #[ignore = "downloads pinned noise reduction resources and probes an isolated local ASR installation"]
    fn real_noise_setup_and_verification_preserve_base_runtime() {
        let root = std::path::PathBuf::from(
            std::env::var("ETHIOPIC_NOISE_TEST_ROOT").expect("isolated noise test root"),
        );
        assert!(
            root.to_string_lossy().contains(".local-noise"),
            "Use a workspace validation installation"
        );
        let before = base_identity(&root).unwrap();
        install(&root, &AtomicBool::new(false)).unwrap();
        let view = status(&root);
        assert!(view.ready);
        assert_eq!(before, base_identity(&root).unwrap());
        save_enabled(&root, true).unwrap();
        let config = configuration(&root, &AtomicBool::new(false), false)
            .unwrap()
            .unwrap();
        assert_eq!(config["revision"], REVISION);
        println!(
            "Noise reduction ready; device={:?}, live={}; base runtime unchanged",
            view.device, view.live_ready
        );
    }
}
