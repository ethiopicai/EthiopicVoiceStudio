//! Local editing projects, verified lazy resources and one resident serialized voice worker.
use crate::hardware::inspect as hardware;
use fs2::FileExt;
use resource_packs::studio::{
    EditingSelection, Hardware, Manifest, DEFAULT_MANIFEST_URL, DEFAULT_REPOSITORY,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

const BUNDLED: &str = include_str!("../../resources/studio-manifest.json");
const LIMIT: usize = 100 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct EditingState {
    root: PathBuf,
    busy: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    stage: Arc<Mutex<String>>,
    pool: Arc<Mutex<Option<Worker>>>,
    resident: Arc<Mutex<Option<(String, String)>>>,
}
impl EditingState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: root.join("voice-edit"),
            busy: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            stage: Arc::new(Mutex::new(String::new())),
            pool: Arc::new(Mutex::new(None)),
            resident: Arc::new(Mutex::new(None)),
        }
    }
    fn task(&self) -> Result<Task, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("A voice-editing task is already running.".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(Task {
            root: self.root.clone(),
            busy: self.busy.clone(),
            cancel: self.cancel.clone(),
            stage: self.stage.clone(),
            pool: self.pool.clone(),
            resident: self.resident.clone(),
        })
    }
    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }
    pub fn shutdown(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut pool) = self.pool.try_lock() {
            *pool = None;
        }
    }
    pub(crate) fn pause_setup(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub(crate) fn change_history<T>(
        &self,
        change: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _task = self.task()?;
        let _processing = recognition_lock(&self.root)?;
        self.release_idle()?;
        let path = self.root.join("voice-worker.lock");
        if path.is_symlink() {
            return Err("Redirected voice storage".into());
        }
        let worker = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|_| "Voice history is unavailable")?;
        worker
            .try_lock_exclusive()
            .map_err(|_| "Close other Studio windows before deleting voice history.")?;
        change()
    }
    /// Called while holding the shared recognition lock, before another model's memory check.
    pub fn release_idle(&self) -> Result<(), String> {
        let mut pool = self
            .pool
            .try_lock()
            .map_err(|_| "Wait for Voice Editing to finish or stop it first.")?;
        *pool = None;
        *self
            .resident
            .lock()
            .map_err(|_| "Voice status unavailable")? = None;
        Ok(())
    }
}
struct Task {
    root: PathBuf,
    busy: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    stage: Arc<Mutex<String>>,
    pool: Arc<Mutex<Option<Worker>>>,
    resident: Arc<Mutex<Option<(String, String)>>>,
}
impl Drop for Task {
    fn drop(&mut self) {
        self.busy.store(false, Ordering::SeqCst);
    }
}
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn identifier() -> String {
    format!(
        "{}-{}-{}",
        now(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::SeqCst)
    )
}
fn safe_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Err("Invalid project identifier".into())
    } else {
        Ok(())
    }
}
fn read_json(path: &Path) -> Result<Value, String> {
    if fs::symlink_metadata(path)
        .map_err(|_| "Saved project is unavailable")?
        .file_type()
        .is_symlink()
    {
        return Err("Redirected storage is not allowed".into());
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|_| "Could not open saved data")?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read saved data")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Saved data is too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Saved data could not be read".into())
}
fn project_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    safe_id(id)?;
    let base = root.join("projects");
    owned_dir(root, &base)?;
    let p = base.join(id);
    if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Redirected project storage".into());
    }
    if p.exists()
        && p.canonicalize().map_err(|_| "Project unavailable")?
            != base
                .canonicalize()
                .map_err(|_| "Project unavailable")?
                .join(id)
    {
        return Err("Redirected project storage".into());
    }
    Ok(p)
}
fn metadata(root: &Path, id: &str) -> Result<Value, String> {
    read_json(&project_dir(root, id)?.join("project.json"))
}
fn current(p: &Value) -> Result<&Value, String> {
    p["versions"]
        .get(p["head"].as_u64().ok_or("Invalid project head")? as usize)
        .ok_or("Invalid project revision".into())
}
fn consent(root: &Path) -> bool {
    read_json(&root.join("consent.json"))
        .ok()
        .is_some_and(|v| v["version"] == 1)
}
fn opt_in(root: &Path) -> bool {
    read_json(&root.join("preferences.json"))
        .ok()
        .is_some_and(|v| v["slow_editing"] == true)
}
fn save_small(path: &Path, value: &Value) -> Result<(), String> {
    if path.is_symlink() {
        return Err("Redirected saved data".into());
    }
    fs::create_dir_all(path.parent().ok_or("Storage unavailable")?)
        .map_err(|_| "Storage unavailable")?;
    let pending = path.with_extension(format!("{}.pending", identifier()));
    let result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "Could not save data")?;
        file.write_all(&serde_json::to_vec(value).map_err(|_| "Could not save data")?)
            .map_err(|_| "Could not save data")?;
        file.sync_all().map_err(|_| "Could not save data")?;
        fs::rename(&pending, path).map_err(|_| "Could not save data")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(pending);
    }
    result
}
fn owned_dir(root: &Path, directory: &Path) -> Result<(), String> {
    if root.is_symlink() {
        return Err("Redirected voice storage".into());
    }
    fs::create_dir_all(root).map_err(|_| "Voice storage unavailable")?;
    let mut current = root.to_path_buf();
    for component in directory
        .strip_prefix(root)
        .map_err(|_| "Invalid voice storage")?
        .components()
    {
        let std::path::Component::Normal(name) = component else {
            return Err("Invalid voice storage".into());
        };
        current.push(name);
        if current.is_symlink() {
            return Err("Redirected voice storage".into());
        }
        if !current.exists() {
            fs::create_dir(&current).map_err(|_| "Voice storage unavailable")?;
        }
    }
    Ok(())
}
pub(crate) fn setup_manifest(app_data: &Path) -> Result<Manifest, String> {
    manifest(&app_data.join("voice-edit"), false)
}
fn manifest(root: &Path, refresh: bool) -> Result<Manifest, String> {
    let cached = root.join("manifest.json");
    let valid = |text: &str| -> Result<Manifest, String> {
        let value = Manifest::parse(text)?;
        if value.manifest_version != 2
            || value.hf_repo.as_deref() != Some(DEFAULT_REPOSITORY)
            || value.hf_revision.is_none()
        {
            return Err("Unapproved model source".into());
        }
        let supported = Manifest::parse(BUNDLED)?;
        // A model refresh is not permission to execute a new arbitrary Python implementation.
        for variant in &value.variants {
            for file in &variant.files {
                if file.path.ends_with(".py")
                    && !supported.find(&variant.id).is_some_and(|v| {
                        v.files
                            .iter()
                            .any(|f| f.path == file.path && f.sha256 == file.sha256)
                    })
                {
                    return Err("These updated voice tools need a Studio app update.".into());
                }
            }
        }
        Ok(value)
    };
    let due = fs::metadata(&cached)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_none_or(|d| d > Duration::from_secs(86400));
    if refresh && due {
        let fetch = || -> Result<Manifest, String> {
            let mut response = resource_packs::client()?
                .get(DEFAULT_MANIFEST_URL)
                .send()
                .map_err(|_| "No connection")?
                .error_for_status()
                .map_err(|_| "Model list unavailable")?;
            let mut bytes = vec![];
            Read::by_ref(&mut response)
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Model list unavailable")?;
            if bytes.len() > 1024 * 1024 {
                return Err("Model list too large".into());
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| "Invalid model list")?;
            let value = valid(text)?;
            fs::create_dir_all(root).map_err(|_| "Storage unavailable")?;
            fs::write(&cached, bytes).map_err(|_| "Could not cache model list")?;
            Ok(value)
        };
        if let Ok(value) = fetch() {
            return Ok(value);
        }
    }
    fs::read_to_string(&cached)
        .ok()
        .and_then(|s| valid(&s).ok())
        .map(Ok)
        .unwrap_or_else(|| valid(BUNDLED))
}
fn selection(state: &EditingState, m: &Manifest) -> Result<EditingSelection, String> {
    selection_for_hardware(state, m, &hardware())
}
fn selection_for_hardware(
    state: &EditingState,
    m: &Manifest,
    source: &Hardware,
) -> Result<EditingSelection, String> {
    let mut h = source.clone();
    if let Ok(resident) = state.resident.lock() {
        if resident
            .as_ref()
            .is_some_and(|(revision, _)| Some(revision) == m.hf_revision.as_ref())
        {
            let small = resident
                .as_ref()
                .is_some_and(|(_, id)| id == "tts-flow_lm-90m-amh-fp32");
            // Matching resident allocations are already paid; don't charge them twice.
            h.free_ram_bytes = h
                .free_ram_bytes
                .saturating_add((if small { 1700 } else { 6200 }) * 1024 * 1024)
                .min(h.total_ram_bytes);
            if !small {
                for gpu in &mut h.nvidia {
                    gpu.free_vram_bytes = gpu
                        .free_vram_bytes
                        .saturating_add(4300 * 1024 * 1024)
                        .min(gpu.total_vram_bytes);
                }
            }
        }
    }
    m.select_editing(&h, opt_in(&state.root))
}
#[derive(Deserialize)]
struct Integrity {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
struct Runtime {
    schema_version: u32,
    os: String,
    arch: String,
    python: String,
    site_packages: String,
    validated: bool,
    capabilities: Vec<String>,
    files: Vec<Integrity>,
}
fn runtime(root: &Path, cancel: &AtomicBool) -> Result<(PathBuf, PathBuf), String> {
    let base = root
        .parent()
        .ok_or("Voice runtime unavailable")?
        .join("studio-runtime/active");
    runtime_at(&base, cancel)
}
fn runtime_at(base: &Path, cancel: &AtomicBool) -> Result<(PathBuf, PathBuf), String> {
    runtime_at_capability(base, cancel, "edit-codec_llm-0.6b-amh")
}
fn runtime_at_capability(
    base: &Path,
    cancel: &AtomicBool,
    capability: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let mut descriptor = read_json(&base.join("installation.json"))?;
    if resource_packs::naming::normalize_metadata(&mut descriptor) {
        save_small(&base.join("installation.json"), &descriptor)?;
    }
    let config: Runtime =
        serde_json::from_value(descriptor).map_err(|_| "Voice runtime needs repair")?;
    if config.schema_version != 1
        || config.os != std::env::consts::OS
        || config.arch != std::env::consts::ARCH
        || !config.validated
        || !config.capabilities.iter().any(|c| c == capability)
    {
        return Err("A compatible private voice-editing runtime must be installed.".into());
    }
    if !resource_packs::safe_path(&config.python)
        || !resource_packs::safe_path(&config.site_packages)
    {
        return Err("Invalid runtime paths".into());
    }
    let entries: Vec<_> = config
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.sha256.as_str()))
        .collect();
    if !config.files.iter().any(|f| f.path == config.python) {
        return Err("Runtime interpreter is not verified".into());
    }
    crate::resource_verification::verify(
        base,
        &entries,
        &base.parent().unwrap().join("verified.json"),
        cancel,
    )?;
    let canonical = base
        .canonicalize()
        .map_err(|_| "Voice runtime unavailable")?;
    let python = base
        .join(config.python)
        .canonicalize()
        .map_err(|_| "Private Python unavailable")?;
    let site = base
        .join(config.site_packages)
        .canonicalize()
        .map_err(|_| "Private packages unavailable")?;
    if !python.starts_with(&canonical) || !site.starts_with(&canonical) {
        return Err("Redirected runtime paths".into());
    }
    Ok((python, site))
}
#[derive(Deserialize)]
struct RuntimeRelease {
    os: String,
    arch: String,
    tiers: Vec<String>,
    version: String,
    artifact: resource_packs::Artifact,
}
#[derive(Deserialize)]
struct RuntimeCatalog {
    schema_version: u32,
    releases: Vec<RuntimeRelease>,
}
fn runtime_catalog() -> Result<RuntimeCatalog, String> {
    let catalog: RuntimeCatalog =
        serde_json::from_str(include_str!("../../resources/editing-runtimes.json"))
            .map_err(|_| "Invalid voice-runtime catalog")?;
    if catalog.schema_version != 1 {
        return Err("Voice tools need an app update.".into());
    }
    Ok(catalog)
}
pub(crate) fn download_quote(
    state: &EditingState,
    kind: &str,
) -> Result<crate::download_setup::Plan, String> {
    use crate::download_setup::{Item, Plan};
    let m = manifest(&state.root, false)?;
    let h = hardware();
    let small = kind == "speech" && small_tts_selected(&m, &h);
    let selection = if small {
        small_tts_plan(&m, &h)?
    } else {
        selection_for_hardware(state, &m, &h)?
    };
    let catalog: RuntimeCatalog = serde_json::from_str(if small {
        include_str!("../../resources/tts-runtimes.json")
    } else {
        include_str!("../../resources/editing-runtimes.json")
    })
    .map_err(|_| "Voice runtime download details unavailable")?;
    let release = compatible_release(&catalog, &selection);
    let base = state.root.parent().ok_or("Studio storage unavailable")?;
    let runtime_info = read_json(&base.join(if small {
        "tts-runtime/active/installation.json"
    } else {
        "studio-runtime/active/installation.json"
    }));
    let installed = runtime_info.is_ok_and(|v| {
        v["validated"] == true
            && v["os"] == h.os
            && v["arch"] == h.arch
            && v["capabilities"].as_array().is_some_and(|caps| {
                caps.iter().filter_map(serde_json::Value::as_str).any(|c| {
                    resource_packs::naming::canonical(c)
                        == if small {
                            "tts-flow_lm-90m-amh-fp32"
                        } else {
                            "edit-codec_llm-0.6b-amh"
                        }
                })
            })
    });
    let platform =
        crate::hardware_setup::platform_check(&h, sysinfo::System::os_version().as_deref());
    let available = platform.is_ok() && selection.available && (installed || release.is_some());
    let reason = platform.err().unwrap_or_else(|| {
        if !selection.available {
            selection.reason.clone()
        } else if !available {
            "A compatible voice runtime download has not been released for this computer yet."
                .into()
        } else {
            "".into()
        }
    });
    let mut items = crate::download_setup::model_items(&m, &selection.downloads)?;
    if let Some(r) = release {
        items.push(Item {
            name: "Speech engine runtime".into(),
            purpose: "Speech tools for this computer".into(),
            bytes: r.artifact.size,
            url: r.artifact.url.clone(),
            sha256: r.artifact.sha256.clone(),
        });
    }
    Plan::new(
        kind,
        if kind == "speech" {
            "Text to Speech"
        } else {
            "Voice Editing"
        },
        available,
        reason,
        items,
    )
}
fn compatible_release<'a>(
    catalog: &'a RuntimeCatalog,
    plan: &EditingSelection,
) -> Option<&'a RuntimeRelease> {
    catalog.releases.iter().find(|r| {
        r.os == std::env::consts::OS
            && r.arch == std::env::consts::ARCH
            && r.tiers.iter().any(|t| t == plan.tier.name())
    })
}
fn ensure_runtime(task: &Task, m: &Manifest, plan: &EditingSelection) -> Result<(), String> {
    let small = plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32");
    let check_runtime = || {
        if small {
            small_runtime(&task.root, &task.cancel)
        } else {
            runtime(&task.root, &task.cancel)
        }
    };
    if check_runtime().is_ok() {
        return Ok(());
    }
    let catalog = if small {
        serde_json::from_str::<RuntimeCatalog>(include_str!("../../resources/tts-runtimes.json"))
            .map_err(|_| "Invalid speech runtime catalog")?
    } else {
        runtime_catalog()?
    };
    let release=compatible_release(&catalog,plan)
        .ok_or("Voice model files are ready. A compatible voice-runtime pack for this computer has not been published yet. Check Settings for availability.")?;
    if release.artifact.kind != "runtime" {
        return Err("Invalid voice-runtime pack".into());
    }
    let base = task
        .root
        .parent()
        .ok_or("Voice storage unavailable")?
        .join(if small {
            "tts-runtime"
        } else {
            "studio-runtime"
        });
    fs::create_dir_all(&base).map_err(|_| "Voice storage unavailable")?;
    let _install_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(base.join("install.lock"))
        .map_err(|_| "Voice setup unavailable")?;
    _install_lock
        .try_lock_exclusive()
        .map_err(|_| "Another Studio window is preparing voice tools")?;
    storage_budget(
        &base,
        release
            .artifact
            .size
            .saturating_add(release.artifact.unpacked_size),
    )?;
    let archive = resource_packs::download(
        &release.artifact,
        &task.root.join("downloads"),
        &task.cancel,
        |bytes| {
            if let Ok(mut stage) = task.stage.lock() {
                *stage = format!(
                    "Downloading voice runtime · {:.0}%",
                    bytes as f64 / release.artifact.size.max(1) as f64 * 100.0
                );
            }
        },
    )?;
    *task.pool.lock().map_err(|_| "Voice worker unavailable")? = None;
    *task.resident.lock().map_err(|_| "Status unavailable")? = None;
    let _recognition = recognition_lock(&task.root)?;
    crate::asr_runtime::release_for_setup()?;
    resource_packs::recover(&base)?;
    let stage = base.join("staging");
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|_| "Voice staging unavailable")?;
    }
    resource_packs::extract(
        &release.artifact,
        &archive,
        &stage,
        &release.version,
        &task.cancel,
    )?;
    let mut config = read_json(&stage.join(if small {
        "runtime/tts-installation.json"
    } else {
        "runtime/voice-edit-installation.json"
    }))?;
    resource_packs::naming::normalize_metadata(&mut config);
    save_small(&stage.join("installation.json"), &config)?;
    let (python, site) = runtime_at_capability(
        &stage,
        &task.cancel,
        if small {
            "tts-flow_lm-90m-amh-fp32"
        } else {
            "edit-codec_llm-0.6b-amh"
        },
    )?;
    run_generation_probe(task, m, plan, &python, &site, &stage)?;
    resource_packs::activate(&base)?;
    check_runtime()?;
    Ok(())
}

fn run_generation_probe(
    task: &Task,
    m: &Manifest,
    plan: &EditingSelection,
    python: &Path,
    site: &Path,
    scratch: &Path,
) -> Result<(), String> {
    let scripts = scratch.join("probe-provider");
    if scripts.exists() {
        fs::remove_dir_all(&scripts).map_err(|_| "Could not restart the voice check")?;
    }
    fs::create_dir(&scripts).map_err(|_| "Voice self-test unavailable")?;
    for (name, data) in [
        (
            "runtime_bootstrap.py",
            include_str!("../../../../workers/common/runtime_bootstrap.py"),
        ),
        (
            "speech_generation.py",
            include_str!("../../../../workers/common/speech_generation.py"),
        ),
        (
            "audio.py",
            include_str!("../../../../workers/editing/audio.py"),
        ),
        (
            "engine.py",
            include_str!("../../../../workers/editing/engine.py"),
        ),
        (
            "worker.py",
            if plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32") {
                include_str!("../../../../workers/tts/worker.py")
            } else {
                include_str!("../../../../workers/editing/worker.py")
            },
        ),
    ] {
        fs::write(scripts.join(name), data).map_err(|_| "Voice self-test unavailable")?;
    }
    let models = models_dir(&task.root, m)?;
    *task.stage.lock().map_err(|_| "Status unavailable")? = "Checking voice tools offline…".into();
    let result_path = scratch.join("probe-result.jsonl");
    if result_path.is_symlink() {
        return Err("Redirected voice check".into());
    }
    let result_file =
        fs::File::create(&result_path).map_err(|_| "Could not prepare the voice check")?;
    let reference = scratch.join("preview-reference.wav");
    if reference.is_symlink() {
        return Err("Redirected test recording".into());
    }
    let private_cache = scratch.join("private-cache");
    if private_cache.is_symlink() {
        return Err("Redirected voice check cache".into());
    }
    fs::create_dir_all(&private_cache).map_err(|_| "Could not prepare the voice check")?;
    fs::write(
        &reference,
        include_bytes!("../../ui/public/reference-voices/preview-v3/tesfaye_1_am.wav"),
    )
    .map_err(|_| "Could not prepare a test recording")?;
    let mut child=crate::background_process::command(python).args(["-I","-X","utf8","-B","-u","-c","import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')"])
        .arg(site).arg(&scripts).arg(scripts.join("worker.py")).arg("--root").arg(scratch.join("probe"))
        .arg("--models").arg(&models).arg("--manifest").arg(models.join("manifest.json"))
        .arg("--tts").arg(plan.tts.as_ref().ok_or("Voice model missing")?).arg("--steps").arg(plan.steps.to_string()).arg("--device").arg(plan.device.as_ref().ok_or("Voice device unavailable")?).arg("--probe").arg("--probe-reference").arg(&reference)
        .env("ORT_DISABLE_TELEMETRY","1").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1").env("ETHIOPIC_MANAGED_WORKER","1")
        .env("HF_HOME",private_cache.join("hf")).env("HF_HUB_CACHE",private_cache.join("hub"))
        .env("TORCH_HOME",private_cache.join("torch")).env("XDG_CACHE_HOME",private_cache.join("xdg"))
        .stdin(Stdio::piped()).stdout(Stdio::from(result_file)).stderr(Stdio::null()).spawn().map_err(|_|"Voice self-test could not start")?;
    let started = Instant::now();
    loop {
        if task.cancel.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if task.cancel.load(Ordering::SeqCst) {
                "paused"
            } else {
                "Voice tools took too long to start. Close other applications and retry."
            }
            .into());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "Voice self-test interrupted")?
        {
            if !status.success() {
                return Err("Voice tools failed their offline check. Your earlier installation is unchanged.".into());
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let receipt = fs::read_to_string(&result_path)
        .map_err(|_| "The speech check did not finish. Try setup again.")?;
    if receipt.len() > 16384
        || !receipt
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|v| v["probe"] == true && v["synthesis_checked"] == true)
    {
        return Err("Speech tools did not produce a valid test recording. Try setup again.".into());
    }
    let _ = fs::remove_file(result_path);
    let _ = fs::remove_file(reference);
    fs::remove_dir_all(&scripts).map_err(|_| "Voice setup cleanup failed")?;
    let _ = fs::remove_dir_all(scratch.join("probe"));
    let _ = fs::remove_dir_all(private_cache);
    Ok(())
}
fn models_dir(root: &Path, m: &Manifest) -> Result<PathBuf, String> {
    Ok(root
        .join("models")
        .join(m.hf_revision.as_ref().ok_or("Model revision missing")?))
}
fn entries(m: &Manifest, plan: &EditingSelection) -> Vec<(String, String)> {
    plan.downloads
        .iter()
        .filter_map(|id| m.find(id))
        .flat_map(|v| {
            v.files
                .iter()
                .map(move |f| (format!("{}/{}", v.folder, f.path), f.sha256.clone()))
        })
        .collect()
}
fn verify_models(
    root: &Path,
    m: &Manifest,
    plan: &EditingSelection,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let directory = models_dir(root, m)?;
    let files = entries(m, plan);
    for (path, sha) in files {
        verify_model_file(root, &directory, &path, &sha, cancel)?;
    }
    // Recreate this trusted model descriptor even after an interrupted earlier setup.
    save_small(
        &directory.join("manifest.json"),
        &serde_json::to_value(m).map_err(|_| "Invalid model list")?,
    )?;
    save_small(
        &directory.join("verified.json"),
        &json!({"revision":m.hf_revision,"tts":plan.tts}),
    )?;
    Ok(())
}
fn verify_model_file(
    root: &Path,
    directory: &Path,
    path: &str,
    sha: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    owned_dir(root, &directory.join("receipts"))?;
    let key = format!("{:x}", Sha256::digest(format!("{path}:{sha}").as_bytes()));
    crate::resource_verification::verify(
        directory,
        &[(path, sha)],
        &directory.join("receipts").join(format!("{key}.json")),
        cancel,
    )?;
    Ok(())
}
pub(crate) fn small_tts_selected(m: &Manifest, h: &Hardware) -> bool {
    if m.select_for_setup(h, resource_packs::studio::Task::Tts { opt_in: false })
        .is_ok_and(|s| s.variant.as_deref() == Some("tts-flow_lm-90m-amh-fp32"))
    {
        return true;
    }
    // An older GPU must not prevent the basic CPU workflow from working. Only
    // choose a GPU provider when a compatible, qualified runtime can be delivered.
    let preferred = serde_json::from_str::<RuntimeCatalog>(include_str!(
        "../../resources/editing-runtimes.json"
    ));
    let gpu_available = preferred.is_ok_and(|c| {
        c.releases.iter().any(|r| {
            r.os == h.os
                && r.arch == h.arch
                && r.tiers
                    .iter()
                    .any(|tier| tier == resource_packs::studio::tier(h).name())
        })
    });
    if gpu_available {
        return false;
    }
    let mut cpu = h.clone();
    cpu.nvidia.clear();
    serde_json::from_str::<RuntimeCatalog>(include_str!("../../resources/tts-runtimes.json"))
        .is_ok_and(|c| {
            c.releases.iter().any(|r| {
                r.os == h.os
                    && r.arch == h.arch
                    && r.tiers
                        .iter()
                        .any(|tier| tier == resource_packs::studio::tier(&cpu).name())
            })
        })
}

fn small_tts_plan(m: &Manifest, h: &Hardware) -> Result<EditingSelection, String> {
    let variant = m
        .find("tts-flow_lm-90m-amh-fp32")
        .ok_or("Speech model missing from the release")?;
    let mut runtime_hardware = h.clone();
    runtime_hardware.nvidia.clear();
    Ok(EditingSelection {
        tier: resource_packs::studio::tier(&runtime_hardware),
        available: true,
        opt_in_required: false,
        slow: false,
        reason: "Speech generation runs locally on the CPU.".into(),
        tts: Some(variant.id.clone()),
        device: Some("cpu".into()),
        steps: 16,
        downloads: vec![variant.id.clone()],
        download_bytes: variant.total_bytes,
        licenses: vec![variant.license.clone()],
    })
}
fn small_runtime(root: &Path, cancel: &AtomicBool) -> Result<(PathBuf, PathBuf), String> {
    runtime_at_capability(
        &root
            .parent()
            .ok_or("Speech storage unavailable")?
            .join("tts-runtime/active"),
        cancel,
        "tts-flow_lm-90m-amh-fp32",
    )
}
fn verify_small_models(root: &Path, m: &Manifest, cancel: &AtomicBool) -> Result<(), String> {
    let directory = models_dir(root, m)?;
    let variant = m
        .find("tts-flow_lm-90m-amh-fp32")
        .ok_or("Speech model missing")?;
    for file in &variant.files {
        verify_model_file(
            root,
            &directory,
            &format!("{}/{}", variant.folder, file.path),
            &file.sha256,
            cancel,
        )?;
    }
    save_small(
        &directory.join("manifest.json"),
        &serde_json::to_value(m).map_err(|_| "Invalid model list")?,
    )?;
    save_small(
        &directory.join("tts-verified.json"),
        &json!({"revision":m.hf_revision,"tts":"tts-flow_lm-90m-amh-fp32"}),
    )
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationStatus {
    pub(crate) ready: bool,
    pub(crate) consent: bool,
    busy: bool,
    stage: String,
    loaded: bool,
    runtime_present: bool,
    runtime_download_bytes: u64,
    runtime_unpacked_bytes: u64,
    pub(crate) can_prepare: bool,
    provider: Option<String>,
    model_version: Option<String>,
    requires_reference_text: bool,
    default_voice: bool,
    controls: crate::generation_controls::Capabilities,
    reason: String,
}
pub(crate) fn generation_status(state: &EditingState) -> Result<GenerationStatus, String> {
    let m = manifest(&state.root, false)?;
    generation_status_for_hardware(state, &m, &hardware())
}
pub(crate) fn generation_status_for_hardware(
    state: &EditingState,
    m: &Manifest,
    h: &Hardware,
) -> Result<GenerationStatus, String> {
    let small = small_tts_selected(m, h);
    let plan = if small {
        small_tts_plan(m, h)?
    } else {
        selection_for_hardware(state, m, h)?
    };
    let base = state
        .root
        .parent()
        .ok_or("Speech storage unavailable")?
        .join(if small {
            "tts-runtime/active"
        } else {
            "studio-runtime/active"
        });
    let runtime_present = read_json(&base.join("installation.json")).is_ok_and(|v| {
        v["validated"] == true
            && v["os"] == std::env::consts::OS
            && v["arch"] == std::env::consts::ARCH
            && v["capabilities"].as_array().is_some_and(|caps| {
                caps.iter().filter_map(serde_json::Value::as_str).any(|c| {
                    resource_packs::naming::canonical(c)
                        == if small {
                            "tts-flow_lm-90m-amh-fp32"
                        } else {
                            "edit-codec_llm-0.6b-amh"
                        }
                })
            })
    });
    let marker = models_dir(&state.root, m).ok().and_then(|p| {
        read_json(&p.join(if small {
            "tts-verified.json"
        } else {
            "verified.json"
        }))
        .ok()
    });
    let models_ready = marker.is_some_and(|v| {
        v["revision"].as_str() == m.hf_revision.as_deref()
            && v["tts"].as_str() == plan.tts.as_deref()
    });
    let catalog: RuntimeCatalog = serde_json::from_str(if small {
        include_str!("../../resources/tts-runtimes.json")
    } else {
        include_str!("../../resources/editing-runtimes.json")
    })
    .map_err(|_| "Speech setup catalog needs repair")?;
    let runtime_release = compatible_release(&catalog, &plan);
    let runtime_download_bytes = if runtime_present {
        0
    } else {
        runtime_release.map(|r| r.artifact.size).unwrap_or(0)
    };
    let runtime_unpacked_bytes = if runtime_present {
        0
    } else {
        runtime_release
            .map(|r| r.artifact.unpacked_size)
            .unwrap_or(0)
    };
    let can_prepare =
        plan.available && (runtime_present || compatible_release(&catalog, &plan).is_some());
    let ready = plan.available && runtime_present && models_ready;
    let reason = if ready {
        "Speech tools are ready on this computer.".into()
    } else if !plan.available {
        plan.reason.clone()
    } else if !can_prepare {
        "The speech model is available, but a compatible speech runtime has not been installed for this preview.".into()
    } else {
        "Download the speech tools once, then generate offline.".into()
    };
    let provider = plan.tts.clone();
    let version = provider
        .as_deref()
        .and_then(|id| m.find(id))
        .and_then(|v| v.model_version.clone());
    Ok(GenerationStatus {
        ready,
        consent: consent(&state.root),
        busy: state.busy.load(Ordering::SeqCst),
        stage: state
            .stage
            .lock()
            .map_err(|_| "Speech status unavailable")?
            .clone(),
        loaded: state
            .resident
            .lock()
            .map_err(|_| "Speech status unavailable")?
            .as_ref()
            .is_some_and(|(_, id)| Some(id) == provider.as_ref()),
        runtime_present,
        runtime_download_bytes,
        runtime_unpacked_bytes,
        can_prepare,
        provider,
        model_version: version,
        requires_reference_text: !small,
        default_voice: small,
        controls: crate::generation_controls::capabilities_with_steps(small, plan.steps),
        reason,
    })
}
#[tauri::command]
pub fn get_voice_generation_activity(state: State<'_, EditingState>) -> Result<Value, String> {
    Ok(
        json!({"busy":state.busy.load(Ordering::SeqCst),"stage":state.stage.lock().map_err(|_|"Speech status unavailable")?.clone(),"loaded":state.resident.lock().map_err(|_|"Speech status unavailable")?.is_some()}),
    )
}
#[tauri::command]
pub async fn get_voice_generation_status(
    state: State<'_, EditingState>,
) -> Result<GenerationStatus, String> {
    generation_status(&state)
}
#[tauri::command]
pub async fn prepare_voice_generation(
    app: tauri::AppHandle,
    state: State<'_, EditingState>,
    approved_plan: String,
) -> Result<(), String> {
    crate::download_setup::require(&app, "speech", &approved_plan)?;
    let m = manifest(&state.root, false)?;
    let h = hardware();
    if !small_tts_selected(&m, &h) {
        return prepare_voice_edit_inner(state).await;
    }
    if !consent(&state.root) {
        return Err("Accept the recording-rights note before setting up speech generation.".into());
    }
    let route = small_tts_plan(&m, &h)?;
    let status = generation_status_for_hardware(&state, &m, &h)?;
    crate::hardware_setup::validate_download(
        state.root.parent().ok_or("Speech storage unavailable")?,
        &m,
        &h,
        &route.downloads,
        status
            .runtime_download_bytes
            .saturating_add(status.runtime_unpacked_bytes),
    )?;
    let task = state.task()?;
    tauri::async_runtime::spawn_blocking(move || prepare_small_tts(&task))
        .await
        .map_err(|_| "Speech setup interrupted")?
}
fn prepare_small_tts(task: &Task) -> Result<(), String> {
    owned_dir(&task.root, &task.root)?;
    let setup_path = task.root.join("prepare.lock");
    if setup_path.is_symlink() {
        return Err("Redirected speech setup".into());
    }
    let setup = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(setup_path)
        .map_err(|_| "Speech setup unavailable")?;
    setup
        .try_lock_exclusive()
        .map_err(|_| "Another Studio window is preparing speech tools")?;
    let m = manifest(&task.root, false)?;
    let plan = small_tts_plan(&m, &hardware())?;
    if small_runtime(&task.root, &task.cancel).is_err() {
        let catalog: RuntimeCatalog =
            serde_json::from_str(include_str!("../../resources/tts-runtimes.json"))
                .map_err(|_| "Speech runtime catalog needs repair")?;
        if compatible_release(&catalog, &plan).is_none() {
            return Err("The speech runtime pack for this computer has not been published yet. Existing voice tools are unchanged.".into());
        }
    }
    let directory = models_dir(&task.root, &m)?;
    owned_dir(&task.root, &directory)?;
    storage_budget(&task.root, plan.download_bytes.saturating_mul(2))?;
    let v = m
        .find("tts-flow_lm-90m-amh-fp32")
        .ok_or("Speech model missing")?;
    let mut completed = 0;
    for f in &v.files {
        let relative = format!("{}/{}", v.folder, f.path);
        if verify_model_file(&task.root, &directory, &relative, &f.sha256, &task.cancel).is_ok() {
            completed += f.bytes;
            continue;
        }
        let base = completed;
        owned_dir(&task.root, &task.root.join("downloads").join(&f.sha256))?;
        let cached = m.download_file(
            &v.id,
            &f.path,
            &task.root.join("downloads"),
            &task.cancel,
            |bytes| {
                if let Ok(mut stage) = task.stage.lock() {
                    *stage = format!(
                        "Downloading speech tools · {:.0}%",
                        (base + bytes) as f64 / plan.download_bytes.max(1) as f64 * 100.
                    );
                }
            },
        )?;
        let target = directory.join(&relative);
        owned_dir(&task.root, target.parent().unwrap())?;
        if target.is_symlink() {
            return Err("Redirected speech model".into());
        }
        fs::copy(cached, target).map_err(|_| "Could not install speech file")?;
        completed += f.bytes;
    }
    verify_small_models(&task.root, &m, &task.cancel)?;
    ensure_runtime(task, &m, &plan)?;
    *task.stage.lock().map_err(|_| "Speech status unavailable")? = "Speech tools are ready.".into();
    Ok(())
}

fn guided_selection(state: &EditingState, m: &Manifest) -> Result<EditingSelection, String> {
    let h = hardware();
    if small_tts_selected(m, &h) {
        small_tts_plan(m, &h)
    } else {
        selection_for_hardware(state, m, &h)
    }
}

pub(crate) fn guided_storage_required(state: &EditingState) -> Result<u64, String> {
    let m = manifest(&state.root, false)?;
    let plan = guided_selection(state, &m)?;
    let root = state.root.parent().ok_or("Speech storage unavailable")?;
    let missing = crate::hardware_setup::missing_model_bytes(root, &m, &plan.downloads);
    let status = generation_status_for_hardware(state, &m, &hardware())?;
    Ok(crate::hardware_setup::storage_required(missing)
        .saturating_add(status.runtime_download_bytes)
        .saturating_add(status.runtime_unpacked_bytes))
}

/// Stage all speech assets, including the matching private engine, before installation.
pub(crate) fn prefetch_guided(
    state: &EditingState,
    cancel: Arc<AtomicBool>,
    report: impl Fn(u64),
) -> Result<(), String> {
    let mut task = state.task()?;
    task.cancel = cancel;
    let m = manifest(&task.root, false)?;
    let plan = guided_selection(state, &m)?;
    let status = generation_status_for_hardware(state, &m, &hardware())?;
    if !status.can_prepare {
        return Err(status.reason);
    }
    crate::hardware_setup::validate_download(
        task.root.parent().ok_or("Speech storage unavailable")?,
        &m,
        &hardware(),
        &plan.downloads,
        status
            .runtime_download_bytes
            .saturating_add(status.runtime_unpacked_bytes),
    )?;
    let directory = models_dir(&task.root, &m)?;
    owned_dir(&task.root, &directory)?;
    let mut completed = 0;
    for id in &plan.downloads {
        let variant = m.find(id).ok_or("Missing speech resource")?;
        for file in &variant.files {
            let relative = format!("{}/{}", variant.folder, file.path);
            if verify_model_file(
                &task.root,
                &directory,
                &relative,
                &file.sha256,
                &task.cancel,
            )
            .is_err()
            {
                let cache = m.download_file(
                    id,
                    &file.path,
                    &task.root.join("downloads"),
                    &task.cancel,
                    |bytes| report(completed + bytes),
                )?;
                let target = directory.join(&relative);
                owned_dir(
                    &task.root,
                    target.parent().ok_or("Speech storage unavailable")?,
                )?;
                if target.is_symlink() {
                    return Err("Redirected speech resource".into());
                }
                fs::copy(cache, target).map_err(|_| "Could not prepare a speech file")?;
                verify_model_file(
                    &task.root,
                    &directory,
                    &relative,
                    &file.sha256,
                    &task.cancel,
                )?;
            }
            completed += file.bytes;
            report(completed);
        }
    }
    let catalog: RuntimeCatalog =
        serde_json::from_str(if plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32") {
            include_str!("../../resources/tts-runtimes.json")
        } else {
            include_str!("../../resources/editing-runtimes.json")
        })
        .map_err(|_| "Speech setup needs an app update")?;
    if let Some(release) = compatible_release(&catalog, &plan) {
        let installed = if plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32") {
            small_runtime(&task.root, &task.cancel).is_ok()
        } else {
            runtime(&task.root, &task.cancel).is_ok()
        };
        if !installed {
            resource_packs::download(
                &release.artifact,
                &task.root.join("downloads"),
                &task.cancel,
                |bytes| report(completed + bytes),
            )?;
        }
        report(completed + release.artifact.size);
    }
    Ok(())
}

pub(crate) fn guided_integrity(state: &EditingState) -> Result<(), String> {
    let task = state.task()?;
    let m = manifest(&task.root, false)?;
    let plan = guided_selection(state, &m)?;
    for (path, sha) in entries(&m, &plan) {
        verify_model_file(
            &task.root,
            &models_dir(&task.root, &m)?,
            &path,
            &sha,
            &task.cancel,
        )?;
    }
    if plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32") {
        small_runtime(&task.root, &task.cancel)?;
    } else {
        runtime(&task.root, &task.cancel)?;
    }
    Ok(())
}

pub(crate) fn install_guided(state: &EditingState, cancel: Arc<AtomicBool>) -> Result<(), String> {
    if !consent(&state.root) {
        return Err("Accept the recording-rights note before setup.".into());
    }
    let mut task = state.task()?;
    task.cancel = cancel;
    let m = manifest(&task.root, false)?;
    if small_tts_selected(&m, &hardware()) {
        prepare_small_tts(&task)
    } else {
        prepare_editing_task(&task)
    }
}

pub(crate) fn check_guided(state: &EditingState, cancel: Arc<AtomicBool>) -> Result<(), String> {
    let mut task = state.task()?;
    task.cancel = cancel;
    let _processing = recognition_lock(&task.root)?;
    state.release_idle()?;
    crate::asr_runtime::release_for_setup()?;
    let m = manifest(&task.root, false)?;
    let plan = guided_selection(state, &m)?;
    let small = plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32");
    if small {
        verify_small_models(&task.root, &m, &task.cancel)?;
    } else {
        verify_models(&task.root, &m, &plan, &task.cancel)?;
    }
    let (python, site) = if small {
        small_runtime(&task.root, &task.cancel)?
    } else {
        runtime(&task.root, &task.cancel)?
    };
    let scratch = task.root.join("guided-check");
    owned_dir(&task.root, &scratch)?;
    let result = run_generation_probe(&task, &m, &plan, &python, &site, &scratch);
    let _ = fs::remove_dir_all(scratch);
    result
}

#[tauri::command]
pub async fn get_default_voice(state: State<'_, EditingState>) -> Result<Value, String> {
    let m = manifest(&state.root, false)?;
    if !small_tts_selected(&m, &hardware()) {
        return Err(
            "No default voice is supplied for this model. Choose a reference recording.".into(),
        );
    }
    let directory = models_dir(&state.root, &m)?;
    let f = m
        .find("tts-flow_lm-90m-amh-fp32")
        .and_then(|v| v.files.iter().find(|f| f.path == "voices/default.wav"))
        .ok_or("Default voice missing")?;
    verify_model_file(
        &state.root,
        &directory,
        "tts-flow_lm-90m-amh-fp32/voices/default.wav",
        &f.sha256,
        &AtomicBool::new(false),
    )?;
    let bytes = fs::read(directory.join("tts-flow_lm-90m-amh-fp32/voices/default.wav"))
        .map_err(|_| "Default voice needs setup")?;
    Ok(json!({"name":"Default voice.wav","bytes":bytes}))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    selection: EditingSelection,
    consent: bool,
    opt_in: bool,
    ready: bool,
    runtime_present: bool,
    busy: bool,
    stage: String,
    loaded: bool,
}
#[tauri::command]
pub fn get_voice_edit_busy(state: State<'_, EditingState>) -> bool {
    state.busy.load(Ordering::SeqCst)
}
#[tauri::command]
pub async fn get_voice_edit_status(state: State<'_, EditingState>) -> Result<Status, String> {
    let m = manifest(&state.root, false)?;
    let plan = selection(&state, &m)?;
    let model_ready = models_dir(&state.root, &m)
        .ok()
        .and_then(|p| read_json(&p.join("verified.json")).ok())
        .is_some_and(|v| {
            v["tts"].as_str() == plan.tts.as_deref()
                && v["revision"].as_str() == m.hf_revision.as_deref()
        });
    let runtime_present = state
        .root
        .parent()
        .unwrap()
        .join("studio-runtime/active/installation.json")
        .is_file();
    Ok(Status {
        consent: consent(&state.root),
        opt_in: opt_in(&state.root),
        ready: plan.available && model_ready && runtime_present,
        runtime_present,
        busy: state.busy.load(Ordering::SeqCst),
        stage: state
            .stage
            .lock()
            .map_err(|_| "Status unavailable")?
            .clone(),
        loaded: state
            .resident
            .lock()
            .map_err(|_| "Status unavailable")?
            .is_some(),
        selection: plan,
    })
}
#[tauri::command]
pub fn accept_voice_edit_consent(state: State<'_, EditingState>) -> Result<(), String> {
    accept_setup_consent(&state)
}
pub(crate) fn accept_setup_consent(state: &EditingState) -> Result<(), String> {
    owned_dir(&state.root, &state.root)?;
    save_small(
        &state.root.join("consent.json"),
        &json!({"version":1,"accepted_at":now().to_string()}),
    )
}
#[tauri::command]
pub fn set_voice_edit_opt_in(state: State<'_, EditingState>, enabled: bool) -> Result<(), String> {
    if state.busy.load(Ordering::SeqCst) {
        return Err("Wait for the current task first".into());
    }
    owned_dir(&state.root, &state.root)?;
    save_small(
        &state.root.join("preferences.json"),
        &json!({"slow_editing":enabled}),
    )
}
#[tauri::command]
pub fn cancel_voice_edit(state: State<'_, EditingState>) {
    state.cancel.store(true, Ordering::SeqCst);
}
#[tauri::command]
pub async fn prepare_voice_edit(
    app: tauri::AppHandle,
    state: State<'_, EditingState>,
    approved_plan: String,
) -> Result<(), String> {
    crate::download_setup::require(&app, "editing", &approved_plan)?;
    prepare_voice_edit_inner(state).await
}
async fn prepare_voice_edit_inner(state: State<'_, EditingState>) -> Result<(), String> {
    if !consent(&state.root) {
        return Err("Please read and accept the recording-rights note first.".into());
    }
    let task = state.task()?;
    tauri::async_runtime::spawn_blocking(move || prepare_editing_task(&task))
        .await
        .map_err(|_| "Voice setup interrupted")?
}
fn prepare_editing_task(task: &Task) -> Result<(), String> {
    owned_dir(&task.root, &task.root)?;
    let setup_path = task.root.join("prepare.lock");
    if setup_path.is_symlink() {
        return Err("Redirected voice setup".into());
    }
    let setup_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(setup_path)
        .map_err(|_| "Voice setup unavailable")?;
    setup_lock
        .try_lock_exclusive()
        .map_err(|_| "Another Studio window is preparing Voice Editing. Wait and retry.")?;
    let m = manifest(&task.root, false)?;
    let plan = m.select_editing(&hardware(), opt_in(&task.root))?;
    if !plan.available {
        return Err(plan.reason);
    }
    let runtime_extra = if runtime(&task.root, &task.cancel).is_ok() {
        0
    } else {
        compatible_release(&runtime_catalog()?, &plan)
            .map(|r| r.artifact.size.saturating_add(r.artifact.unpacked_size))
            .unwrap_or(0)
    };
    crate::hardware_setup::validate_download(
        task.root.parent().ok_or("Voice storage unavailable")?,
        &m,
        &hardware(),
        &plan.downloads,
        runtime_extra,
    )?;
    // Do not spend a slow connection downloading gigabytes for a runtime we cannot deliver.
    if runtime(&task.root, &task.cancel).is_err()
        && compatible_release(&runtime_catalog()?, &plan).is_none()
    {
        return Err("Voice Editing tools for this platform have not been released yet. No model files were downloaded.".into());
    }
    let directory = models_dir(&task.root, &m)?;
    if verify_models(&task.root, &m, &plan, &task.cancel).is_ok() {
        return ensure_runtime(task, &m, &plan);
    }
    owned_dir(&task.root, &directory)?;
    let missing = entries(&m, &plan)
        .into_iter()
        .filter(|(path, sha)| {
            verify_model_file(&task.root, &directory, path, sha, &task.cancel).is_err()
        })
        .filter_map(|(path, _)| {
            m.variants
                .iter()
                .flat_map(|v| {
                    v.files
                        .iter()
                        .map(move |f| (format!("{}/{}", v.folder, f.path), f.bytes))
                })
                .find(|(p, _)| p == &path)
                .map(|(_, bytes)| bytes)
        })
        .sum::<u64>();
    storage_budget(&task.root, missing.saturating_mul(2))?;
    let mut completed = 0;
    for id in &plan.downloads {
        let v = m.find(id).ok_or("Missing voice dependency")?;
        for f in &v.files {
            let relative = format!("{}/{}", v.folder, f.path);
            if verify_model_file(&task.root, &directory, &relative, &f.sha256, &task.cancel).is_ok()
            {
                completed += f.bytes;
                continue;
            }
            let base = completed;
            owned_dir(&task.root, &task.root.join("downloads").join(&f.sha256))?;
            let cached = m.download_file(
                id,
                &f.path,
                &task.root.join("downloads"),
                &task.cancel,
                |bytes| {
                    if let Ok(mut stage) = task.stage.lock() {
                        *stage = format!(
                            "Preparing voice tools · {:.0}%",
                            (base + bytes) as f64 / plan.download_bytes.max(1) as f64 * 100.0
                        );
                    }
                },
            )?;
            let target = directory.join(&v.folder).join(&f.path);
            owned_dir(&task.root, target.parent().unwrap())?;
            if target.is_symlink() {
                return Err("Redirected model file".into());
            }
            fs::copy(cached, &target).map_err(|_| "Could not install voice file")?;
            verify_model_file(&task.root, &directory, &relative, &f.sha256, &task.cancel)?;
            completed += f.bytes;
        }
    }
    fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec(&m).map_err(|_| "Invalid model list")?,
    )
    .map_err(|_| "Could not save model list")?;
    verify_models(&task.root, &m, &plan, &task.cancel)?;
    *task.stage.lock().map_err(|_| "Status unavailable")? = "Voice models are ready.".into();
    ensure_runtime(task, &m, &plan)
}

struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    events: mpsc::Receiver<Value>,
    identity: (String, String),
    _lock: fs::File,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn launch(task: &Task, m: &Manifest, plan: &EditingSelection) -> Result<Worker, String> {
    let small = plan.tts.as_deref() == Some("tts-flow_lm-90m-amh-fp32");
    if small {
        verify_small_models(&task.root, m, &task.cancel)?;
    } else {
        verify_models(&task.root, m, plan, &task.cancel)?;
    }
    let (python, site) = if small {
        small_runtime(&task.root, &task.cancel)
    } else {
        runtime(&task.root, &task.cancel)
    }
    .map_err(|_| {
        "Voice Editing needs its compatible private runtime. Open Settings to check voice tools."
            .to_string()
    })?;
    let scripts = task.root.join("provider");
    owned_dir(&task.root, &scripts)?;
    for (name, data) in [
        (
            "runtime_bootstrap.py",
            include_str!("../../../../workers/common/runtime_bootstrap.py"),
        ),
        (
            "speech_generation.py",
            include_str!("../../../../workers/common/speech_generation.py"),
        ),
        (
            "audio.py",
            include_str!("../../../../workers/editing/audio.py"),
        ),
        (
            "engine.py",
            include_str!("../../../../workers/editing/engine.py"),
        ),
        (
            "worker.py",
            if small {
                include_str!("../../../../workers/tts/worker.py")
            } else {
                include_str!("../../../../workers/editing/worker.py")
            },
        ),
    ] {
        if scripts.join(name).is_symlink() {
            return Err("Redirected voice worker".into());
        }
        fs::write(scripts.join(name), data).map_err(|_| "Could not prepare voice worker")?;
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(task.root.join("voice-worker.lock"))
        .map_err(|_| "Voice worker unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "Another Studio window is using voice editing. Close it and retry.")?;
    let models = models_dir(&task.root, m)?;
    let manifest_path = models.join("manifest.json");
    if !manifest_path.is_file() {
        return Err("Voice models need setup".into());
    }
    let tts = plan.tts.clone().ok_or("Voice model missing")?;
    let private_cache = task.root.join("runtime-cache");
    owned_dir(&task.root, &private_cache)?;
    let mut child=crate::background_process::command(python).args(["-I","-X","utf8","-B","-u","-c","import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')"])
        .arg(site).arg(&scripts).arg(scripts.join("worker.py")).arg("--root").arg(&task.root).arg("--models").arg(&models).arg("--manifest").arg(manifest_path)
        .arg("--tts").arg(&tts).arg("--steps").arg(plan.steps.to_string()).arg("--threads").arg(if plan.slow {"6"}else{"4"})
        .arg("--device").arg(plan.device.as_ref().ok_or("Voice device unavailable")?)
        .env("ORT_DISABLE_TELEMETRY","1").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1")
        .env("HF_HOME",private_cache.join("hf")).env("HF_HUB_CACHE",private_cache.join("hub"))
        .env("TORCH_HOME",private_cache.join("torch")).env("XDG_CACHE_HOME",private_cache.join("xdg"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_|"Private voice runtime could not start")?;
    let input = child.stdin.take().ok_or("Voice input unavailable")?;
    let output = child.stdout.take().ok_or("Voice output unavailable")?;
    let (send, events) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut line = String::new();
            let read = Read::by_ref(&mut reader)
                .take(8 * 1024 * 1024 + 1)
                .read_line(&mut line);
            if !read.is_ok_and(|n| n > 0 && n <= 8 * 1024 * 1024) {
                break;
            }
            let Ok(value) = serde_json::from_str(&line) else {
                break;
            };
            if send.send(value).is_err() {
                break;
            }
        }
    });
    Ok(Worker {
        child,
        input: Some(input),
        events,
        identity: (m.hf_revision.clone().unwrap(), tts),
        _lock: lock,
    })
}
fn request(task: &Task, req: Value) -> Result<Value, String> {
    let m = manifest(&task.root, false)?;
    let mut pool = task.pool.lock().map_err(|_| "Voice worker unavailable")?;
    // The native parent serializes editing and normal synthesis through this same process.
    let mut h = hardware();
    let desired = if req["action"] == "synthesize" {
        if small_tts_selected(&m, &h) {
            Some("tts-flow_lm-90m-amh-fp32".into())
        } else {
            m.select_for_setup(&h, resource_packs::studio::Task::Tts { opt_in: false })?
                .variant
        }
    } else {
        m.bundles
            .get(resource_packs::studio::tier(&h).name())
            .and_then(|b| {
                if b.edit.is_some() {
                    b.tts.clone()
                } else {
                    b.tts_optin.clone()
                }
            })
    };
    if req["action"] == "synthesize" {
        let options: crate::generation_controls::Options = serde_json::from_value(
            req.get("generationOptions")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| json!({})),
        )
        .map_err(|_| "Invalid advanced speech controls")?;
        options.validate(desired.as_deref() == Some("tts-flow_lm-90m-amh-fp32"))?;
        crate::generation_controls::validate_seed(
            req.get("seed")
                .map(|v| v.as_u64().ok_or("Invalid speech seed"))
                .transpose()?
                .unwrap_or(0),
        )?;
    }
    if pool.as_ref().is_some_and(|w| {
        Some(&w.identity.0) != m.hf_revision.as_ref() || Some(&w.identity.1) != desired.as_ref()
    }) {
        *pool = None;
        *task.resident.lock().map_err(|_| "Status unavailable")? = None;
        h = hardware();
    }
    // Loading a new CPU voice model can reclaim idle ASR under memory pressure,
    // but this is not an admission limit: still attempt model loading afterwards.
    if pool.is_none()
        && desired.as_deref() == Some("tts-flow_lm-90m-amh-fp32")
        && h.free_ram_bytes < 2 * 1024 * 1024 * 1024
        && crate::asr_runtime::release_idle()?
    {
        h = hardware();
    }
    // Preserve the resident ASR unless this task actually needs its memory.
    // Credit an already-loaded TTS only when evaluating its existing footprint.
    let mut released_asr = false;
    let plan = loop {
        let mut capacity = h.clone();
        if pool
            .as_ref()
            .is_some_and(|w| Some(&w.identity.0) == m.hf_revision.as_ref())
        {
            let small = pool
                .as_ref()
                .is_some_and(|w| w.identity.1 == "tts-flow_lm-90m-amh-fp32");
            capacity.free_ram_bytes = h
                .free_ram_bytes
                .saturating_add((if small { 1700 } else { 6200 }) * 1024 * 1024)
                .min(capacity.total_ram_bytes);
            if !small {
                for g in &mut capacity.nvidia {
                    g.free_vram_bytes = g
                        .free_vram_bytes
                        .saturating_add(4300 * 1024 * 1024)
                        .min(g.total_vram_bytes);
                }
            }
        }
        let plan = if req["action"] == "synthesize" && small_tts_selected(&m, &capacity) {
            small_tts_plan(&m, &capacity)?
        } else {
            m.select_editing(&capacity, opt_in(&task.root))?
        };
        if plan.available {
            break plan;
        }
        let mut host_capacity = capacity.clone();
        host_capacity.free_ram_bytes = host_capacity.total_ram_bytes;
        let host_memory_could_help = m
            .select_editing(&host_capacity, opt_in(&task.root))?
            .available;
        if released_asr || !host_memory_could_help || !crate::asr_runtime::release_idle()? {
            return Err(plan.reason);
        }
        released_asr = true;
        h = hardware();
    };
    let identity = (m.hf_revision.clone().unwrap(), plan.tts.clone().unwrap());
    if pool.as_ref().is_some_and(|w| w.identity != identity) {
        *pool = None;
        *task.resident.lock().map_err(|_| "Status unavailable")? = None;
    }
    if pool.is_none() {
        *pool = Some(launch(task, &m, &plan)?);
        *task.resident.lock().map_err(|_| "Status unavailable")? = Some(identity);
    }
    let worker = pool.as_mut().unwrap();
    let token = SEQUENCE.fetch_add(1, Ordering::SeqCst);
    let mut req = req;
    req["request"] = json!(token);
    let mut data = serde_json::to_vec(&req).map_err(|_| "Invalid voice request")?;
    if data.len() > 2 * 1024 * 1024 {
        return Err("Voice request too large".into());
    }
    data.push(b'\n');
    if worker
        .input
        .as_mut()
        .ok_or("Voice worker unavailable")?
        .write_all(&data)
        .is_err()
    {
        *pool = None;
        *task.resident.lock().map_err(|_| "Status unavailable")? = None;
        return Err("Voice worker stopped. Your saved recording is unchanged.".into());
    }
    let started = Instant::now();
    loop {
        if task.cancel.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(600) {
            *pool = None;
            *task.resident.lock().map_err(|_| "Status unavailable")? = None;
            return Err("Voice task stopped. Reopen the project to see its saved revision.".into());
        }
        match worker.events.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => {
                if value["protocol"] != 1 {
                    return Err("Incompatible voice worker".into());
                }
                if value["request"].as_u64() != Some(token) {
                    continue;
                }
                if let Some(stage) = value["stage"].as_str() {
                    *task.stage.lock().map_err(|_| "Status unavailable")? = stage.into();
                }
                if let Some(result) = value.get("result") {
                    *task.stage.lock().map_err(|_| "Status unavailable")? =
                        "Ready for your next change".into();
                    return Ok(result.clone());
                }
                if let Some(error) = value.get("error") {
                    let message = error["message"]
                        .as_str()
                        .unwrap_or("Voice editing could not finish")
                        .to_string();
                    // Unknown model/runtime failures may leave a partial load or CUDA state.
                    // Refused edits keep their healthy engine; runtime failures start clean next time.
                    if error["kind"] == "failed" {
                        *pool = None;
                        *task.resident.lock().map_err(|_| "Status unavailable")? = None;
                    }
                    return Err(message);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if worker
                    .child
                    .try_wait()
                    .map_err(|_| "Voice worker unavailable")?
                    .is_some()
                {
                    *pool = None;
                    *task.resident.lock().map_err(|_| "Status unavailable")? = None;
                    return Err("Voice worker stopped. Your saved recording is unchanged.".into());
                }
            }
            Err(_) => {
                *pool = None;
                *task.resident.lock().map_err(|_| "Status unavailable")? = None;
                return Err("Voice worker stopped. Retry after checking voice tools.".into());
            }
        }
    }
}

fn recognition_lock(root: &Path) -> Result<fs::File, String> {
    owned_dir(root, root)?;
    let path = root
        .parent()
        .ok_or("Voice storage unavailable")?
        .join("recognition.lock");
    if path.is_symlink() {
        return Err("Redirected voice queue".into());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| "Voice queue unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "Finish or stop the current recording, cleanup or voice task before starting another voice task.")?;
    Ok(lock)
}

fn storage_budget(directory: &Path, required: u64) -> Result<(), String> {
    let required = required.saturating_add(256 * 1024 * 1024);
    if fs2::available_space(directory).map_err(|_| "Could not check free storage")? < required {
        return Err(format!(
            "Free at least {:.1} GB of storage and retry. Verified downloads are kept.",
            required as f64 / 1_000_000_000.0
        ));
    }
    Ok(())
}
#[tauri::command]
// Keep the existing flat Tauri payload compatible with editing clients.
#[allow(clippy::too_many_arguments)]
pub async fn voice_edit_action(
    state: State<'_, EditingState>,
    action: String,
    project_id: String,
    revision: u64,
    text: Option<String>,
    seed: Option<u64>,
    reference_text: Option<String>,
    generation_options: Option<crate::generation_controls::Options>,
) -> Result<Value, String> {
    if !matches!(
        action.as_str(),
        "align" | "fix" | "preview" | "keep" | "undo" | "synthesize"
    ) {
        return Err("Unknown voice action".into());
    }
    if !consent(&state.root) {
        return Err("Please accept the recording-rights note first.".into());
    }
    safe_id(&project_id)?;
    if action == "synthesize" {
        if let Some(options) = &generation_options {
            options.validate_values()?;
        }
        crate::generation_controls::validate_seed(seed.unwrap_or(0))?;
    } else if generation_options.is_some() {
        return Err("Speech controls apply only to speech generation.".into());
    }
    if text.as_ref().is_some_and(|t| t.len() > 200000)
        || reference_text.as_ref().is_some_and(|t| t.len() > 200000)
    {
        return Err("Text is too long".into());
    }
    if matches!(action.as_str(), "preview" | "synthesize") {
        crate::speech_text::verify(text.as_deref().unwrap_or_default(), action == "preview")?;
    }
    let task = state.task()?;
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move||{
        // Hold both guards through metadata publication, so deletion cannot race it.
        let _processing=recognition_lock(&task.root)?;
        if action=="synthesize" && project_dir(&root,&project_id)?.join("synthesis-context.json").exists(){return Err("Saved audio cannot be overwritten. Regenerate it as a new version.".into())}
        let wording=text.unwrap_or_default();
        let result=request(&task,json!({"action":action,"projectId":project_id,"revision":revision,"text":wording,"seed":seed.unwrap_or(0),"referenceText":reference_text.unwrap_or_default(),"generationOptions":generation_options}))?;
        if action=="synthesize" {save_small(&project_dir(&root,&project_id)?.join("synthesis-context.json"),&json!({"text":wording,"seed":seed.unwrap_or(0),"settings":result.get("settings"),"projectId":project_id,"result":result,"kind":"synthesis"}))?;}
        Ok(result)
    }).await.map_err(|_|"Voice task interrupted")?
}
#[tauri::command]
pub fn list_voice_edit_projects(state: State<'_, EditingState>) -> Result<Vec<Value>, String> {
    let base = state.root.join("projects");
    if !base.exists() {
        return Ok(vec![]);
    }
    let mut values = vec![];
    for entry in fs::read_dir(base)
        .map_err(|_| "Project storage unavailable")?
        .flatten()
    {
        if let Some(id) = entry.file_name().to_str() {
            if let Ok(value) = metadata(&state.root, id) {
                if !entry.path().join("speech-group.json").exists() {
                    values.push(value)
                }
            }
        }
    }
    values.sort_by(|a, b| {
        b["created"]
            .as_f64()
            .partial_cmp(&a["created"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(values)
}
#[tauri::command]
pub async fn import_voice_edit_project(
    state: State<'_, EditingState>,
    name: String,
    bytes: Vec<u8>,
    text: Option<String>,
    words: Option<Value>,
    source_media: Option<crate::recording_transfer::Context>,
) -> Result<Value, String> {
    if bytes.len() > LIMIT || bytes.len() < 44 || text.as_ref().is_some_and(|t| t.len() > 200000) {
        return Err("Choose a recording under 100 MB.".into());
    }
    if !consent(&state.root) {
        return Err("Please accept the recording-rights note first.".into());
    }
    let task = state.task()?;
    tauri::async_runtime::spawn_blocking(move || {
        create_speech_reference(&task.root, name, bytes, text, words, source_media)
    })
    .await
    .map_err(|_| "Import interrupted")?
}
fn create_speech_reference(
    root: &Path,
    name: String,
    bytes: Vec<u8>,
    text: Option<String>,
    words: Option<Value>,
    source_media: Option<crate::recording_transfer::Context>,
) -> Result<Value, String> {
    let (name, bytes) = crate::audio_import::prepare(name, bytes)?;
    let wav = hound::WavReader::new(std::io::Cursor::new(&bytes))
        .map_err(|_| "Use an uncompressed PCM WAV recording")?;
    let spec = wav.spec();
    if spec.sample_format != hound::SampleFormat::Int
        || !matches!(spec.bits_per_sample, 8 | 16 | 24 | 32)
        || !(1..=8).contains(&spec.channels)
        || !(8000..=192000).contains(&spec.sample_rate)
    {
        return Err("Use an uncompressed integer PCM WAV recording.".into());
    }
    let id = identifier();
    let directory = project_dir(root, &id)?;
    fs::create_dir(&directory).map_err(|_| "Could not save project")?;
    let result = (|| {
        crate::recording_transfer::write_context(&directory, source_media.as_ref(), &bytes)?;
        fs::write(directory.join("original.wav"), &bytes)
            .map_err(|_| "Could not save project audio")?;
        let regions = source_media
            .as_ref()
            .map(|context| {
                context
                    .edited_regions
                    .iter()
                    .map(|region| {
                        let mut mark = region.clone();
                        mark["span_in"] = mark["span_out"].clone();
                        mark
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let version = json!({"audio":"original.wav","text":text.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" "),"words":words.unwrap_or(json!([])),"regions":regions,"kind":"import","parent":null,"seed":null});
        let project = json!({"id":id,"name":name.chars().filter(|c|!c.is_control()).take(240).collect::<String>(),"created":now() as f64/1000.,"revision":0,"head":0,"rate":spec.sample_rate,"channels":spec.channels,"width":spec.bits_per_sample/8,"duration":wav.duration() as f64/spec.sample_rate as f64,"versions":[version],"consent":true});
        save_small(&directory.join("project.json"), &project)?;
        Ok(project)
    })();
    if result.is_err() {
        if let Some(app_root) = root.parent() {
            let _ = crate::asr::remove_managed_dir(app_root, "voice-edit/projects", &id);
        }
    }
    result
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpeechInput {
    group_id: Option<String>,
    expected_version: Option<String>,
    base_version: Option<String>,
    name: String,
    bytes: Vec<u8>,
    text: String,
    original_text: Option<String>,
    reference_text: String,
    words: Option<Value>,
    source_media: Option<crate::recording_transfer::Context>,
    voice: SpeechVoice,
    seed: u64,
    generation_options: Option<crate::generation_controls::Options>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpeechVoice {
    kind: String,
    label: String,
    id: Option<String>,
}

fn render_speech(task: &Task, input: SpeechInput) -> Result<Value, String> {
    render_speech_with(task, input, request)
}
fn render_speech_with(
    task: &Task,
    input: SpeechInput,
    render: impl FnOnce(&Task, Value) -> Result<Value, String>,
) -> Result<Value, String> {
    if input.bytes.len() < 44
        || input.bytes.len() > LIMIT
        || input.text.trim().is_empty()
        || input.text.chars().count() > 12000
        || input.reference_text.len() > 200000
    {
        return Err("Choose a reference under 100 MB and a script up to 12,000 characters.".into());
    }
    crate::speech_text::verify(&input.text, false)?;
    if input
        .original_text
        .as_ref()
        .is_some_and(|text| text.chars().count() > 12000)
    {
        return Err("Use a script up to 12,000 characters.".into());
    }
    if !matches!(
        input.voice.kind.as_str(),
        "default" | "profile" | "reference"
    ) || input.voice.label.is_empty()
        || input.voice.label.chars().count() > 240
        || input.voice.label.chars().any(char::is_control)
        || input
            .voice
            .id
            .as_ref()
            .is_some_and(|id| safe_id(id).is_err())
    {
        return Err("Invalid voice selection".into());
    }
    crate::generation_controls::validate_seed(input.seed)?;
    if let Some(options) = &input.generation_options {
        options.validate_values()?;
    }
    let app_root = task.root.parent().ok_or("Speech storage unavailable")?;
    if let Some(group) = &input.group_id {
        let versions = crate::speech_versions::versions(app_root, group)?;
        if versions.len() >= 100 {
            return Err("This recording has 100 versions. Create a new speech card.".into());
        }
        if versions.last().and_then(|v| v["projectId"].as_str())
            != input.expected_version.as_deref()
            || !versions
                .iter()
                .any(|v| v["projectId"].as_str() == input.base_version.as_deref())
        {
            return Err("This speech card changed. Reopen it before regenerating.".into());
        }
    } else if input.expected_version.is_some() || input.base_version.is_some() {
        return Err("Invalid speech version selection".into());
    }
    let project = create_speech_reference(
        &task.root,
        input.name,
        input.bytes,
        Some(input.reference_text.clone()),
        input.words,
        input.source_media,
    )?;
    let id = project["id"]
        .as_str()
        .ok_or("Speech reference unavailable")?;
    let group = input.group_id.as_deref().unwrap_or(id);
    let directory = project_dir(&task.root, id)?;
    // Every render owns a fresh reference and output. Only the context file publishes it.
    let result = (|| {
        save_small(
            &directory.join("speech-group.json"),
            &json!({"groupId":group}),
        )?;
        let mut rendered = render(
            task,
            json!({"action":"synthesize","projectId":id,"revision":0,"text":input.text,"seed":input.seed,"referenceText":input.reference_text,"generationOptions":input.generation_options}),
        )?;
        if task.cancel.load(Ordering::SeqCst) {
            return Err("Speech generation stopped.".into());
        }
        if let Some(worker) = task
            .pool
            .lock()
            .map_err(|_| "Speech worker unavailable")?
            .as_ref()
        {
            rendered["model"] = json!(worker.identity.1);
            rendered["revision"] = json!(worker.identity.0);
            if rendered.get("model_version").is_none() {
                let m = manifest(&task.root, false)?;
                rendered["model_version"] = json!(m
                    .variants
                    .iter()
                    .find(|v| v.id == worker.identity.1)
                    .and_then(|v| v.model_version.clone()));
            }
        }
        let output = directory.join("synthesis.wav");
        let audio = fs::symlink_metadata(&output).map_err(|_| "Generated audio unavailable")?;
        if !audio.is_file()
            || audio.file_type().is_symlink()
            || audio.len() < 44
            || audio.len() > LIMIT as u64
        {
            return Err("Generated audio is invalid".into());
        }
        let context = json!({"schemaVersion":1,"groupId":group,"projectId":id,"baseVersion":input.base_version,"created":project["created"],"text":input.text,"originalText":input.original_text.as_deref().unwrap_or(&input.text),"textPreparation":{"version":crate::speech_text::version()},"seed":input.seed,"settings":rendered.get("settings"),"reference":{"voice":input.voice,"name":project["name"],"text":input.reference_text,"sha256":format!("{:x}",Sha256::digest(fs::read(directory.join("original.wav")).map_err(|_|"Reference unavailable")?))},"result":rendered,"kind":"synthesis"});
        save_small(&directory.join("synthesis-context.json"), &context)?;
        Ok(json!({"groupId":group,"projectId":id}))
    })();
    if result.is_err() {
        let _ = crate::asr::remove_managed_dir(app_root, "voice-edit/projects", id);
    }
    result
}
#[tauri::command]
pub async fn synthesize_speech(
    state: State<'_, EditingState>,
    input: SpeechInput,
) -> Result<Value, String> {
    crate::speech_text::verify(&input.text, false)?;
    if !consent(&state.root) {
        return Err("Please accept the recording-rights note first.".into());
    }
    let task = state.task()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _queue = recognition_lock(&task.root)?;
        render_speech(&task, input)
    })
    .await
    .map_err(|_| "Speech generation interrupted")?
}
#[derive(Serialize)]
pub struct Audio {
    name: String,
    bytes: Vec<u8>,
}
#[tauri::command]
pub async fn get_voice_edit_audio(
    state: State<'_, EditingState>,
    project_id: String,
    kind: String,
) -> Result<Audio, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = metadata(&root, &project_id)?;
        let version = current(&p)?;
        let name = match kind.as_str() {
            "original" => "original.wav",
            "current" => version["audio"].as_str().ok_or("Audio revision missing")?,
            "preview" => "preview.wav",
            "synthesis" => "synthesis.wav",
            _ => return Err("Invalid audio selection".into()),
        };
        if name.contains(['/', '\\']) || !name.ends_with(".wav") {
            return Err("Invalid audio revision".into());
        }
        let path = project_dir(&root, &project_id)?.join(name);
        let meta = fs::symlink_metadata(&path).map_err(|_| "Recording unavailable")?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > LIMIT as u64 {
            return Err("Invalid recording".into());
        }
        Ok(Audio {
            name: name.into(),
            bytes: fs::read(path).map_err(|_| "Could not read recording")?,
        })
    })
    .await
    .map_err(|_| "Audio read interrupted")?
}
#[tauri::command]
pub async fn export_voice_edit_project(
    app: tauri::AppHandle,
    state: State<'_, EditingState>,
    project_id: String,
) -> Result<String, String> {
    let task = state.task()?;
    let root = state.root.clone();
    let downloads = app
        .path()
        .download_dir()
        .map_err(|_| "Downloads folder unavailable")?;
    tauri::async_runtime::spawn_blocking(move||{
        let _processing=recognition_lock(&task.root)?;
        let p=metadata(&root,&project_id)?;let v=current(&p)?;let audio=v["audio"].as_str().ok_or("Audio unavailable")?;
        if audio.contains(['/', '\\'])||!audio.ends_with(".wav"){return Err("Invalid audio revision".into())}
        let source=project_dir(&root,&project_id)?.join(audio);if source.is_symlink(){return Err("Redirected audio".into())}
        let folder=downloads.join(format!("Ethiopic-Voice-Edit-{}",identifier()));fs::create_dir(&folder).map_err(|_|"Could not create export folder")?;
        fs::copy(source,folder.join("recording.wav")).map_err(|_|"Could not export audio")?;
        let mut history=vec![];let mut head=p["head"].as_u64();let versions=p["versions"].as_array().ok_or("History unavailable")?;
        for _ in 0..versions.len(){let Some(i)=head else{break};let item=versions.get(i as usize).ok_or("History unavailable")?;history.push(item.clone());head=item["parent"].as_u64();}
        history.reverse();let log=json!({"schema_version":1,"project":project_id,"name":p["name"],"sample_rate":p["rate"],"channels":p["channels"],"synthetic":v["regions"].as_array().is_some_and(|v|!v.is_empty()),"text":v["text"],"words":v["words"],"regenerated_regions":v["regions"],"history":history,"usage":"Studio application: PolyForm Noncommercial. Model weights have separate licenses. Reference voices require permission."});
        save_small(&folder.join("edit-log.json"),&log)?;Ok(folder.to_string_lossy().into())
    }).await.map_err(|_|"Export interrupted")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn older_windows_gpu_uses_qualified_cpu_speech_instead_of_missing_cuda_tools() {
        let m = Manifest::parse(BUNDLED).unwrap();
        let h = Hardware {
            os: "windows".into(),
            arch: "x86_64".into(),
            total_ram_bytes: 16 * 1024 * 1024 * 1024,
            free_ram_bytes: 8 * 1024 * 1024 * 1024,
            physical_cores: Some(8),
            avx2: Some(true),
            nvidia: vec![resource_packs::studio::Nvidia {
                index: 0,
                compute_major: 6,
                compute_minor: 1,
                total_vram_bytes: 6 * 1024 * 1024 * 1024,
                free_vram_bytes: 6 * 1024 * 1024 * 1024,
            }],
        };
        assert_eq!(
            m.select_for_setup(&h, resource_packs::studio::Task::Tts { opt_in: false })
                .unwrap()
                .variant
                .as_deref(),
            Some("tts-codec_llm-0.6b-amh-fp32")
        );
        assert!(small_tts_selected(&m, &h));
        let plan = small_tts_plan(&m, &h).unwrap();
        assert_eq!(plan.device.as_deref(), Some("cpu"));
        assert_eq!(plan.downloads, vec!["tts-flow_lm-90m-amh-fp32"]);
        let mut modern = h.clone();
        modern.nvidia[0].compute_major = 7;
        modern.nvidia[0].compute_minor = 5;
        modern.nvidia[0].total_vram_bytes = 11 * 1024 * 1024 * 1024;
        modern.nvidia[0].free_vram_bytes = modern.nvidia[0].total_vram_bytes;
        assert!(
            !small_tts_selected(&m, &modern),
            "The qualified modern GPU keeps its GPU speech provider"
        );
    }
    fn speech_input(
        bytes: Vec<u8>,
        group: Option<&str>,
        latest: Option<&str>,
        base: Option<&str>,
    ) -> SpeechInput {
        serde_json::from_value(json!({"name":"reference.wav","bytes":bytes,"text":"ሰላም","referenceText":"ጤና","voice":{"kind":"reference","label":"My voice"},"seed":42,"generationOptions":{"temperature":0.4,"sentencePause":0.5},"groupId":group,"expectedVersion":latest,"baseVersion":base})).unwrap()
    }
    fn speech_fixture(root: &Path) -> Vec<u8> {
        let path = root.join("input.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16000 {
            writer.write_sample(100i16).unwrap();
        }
        writer.finalize().unwrap();
        fs::read(path).unwrap()
    }
    fn simulated_render(task: &Task, req: Value) -> Result<Value, String> {
        let path = project_dir(&task.root, req["projectId"].as_str().unwrap())?;
        fs::copy(path.join("original.wav"), path.join("synthesis.wav")).unwrap();
        Ok(
            json!({"settings":{"schemaVersion":1,"seed":req["seed"],"temperature":req["generationOptions"]["temperature"],"sentencePause":req["generationOptions"]["sentencePause"]},"model":"test-engine","model_version":"test","revision":"pinned"}),
        )
    }
    #[test]
    fn unprepared_speech_never_renders_and_prepared_versions_keep_the_original_script() {
        let storage = Storage::new();
        let state = EditingState::new(storage.0.clone());
        let bytes = speech_fixture(&storage.0);
        let task = state.task().unwrap();
        let mut input = speech_input(bytes.clone(), None, None, None);
        input.text = "ሰላም 12".into();
        assert!(render_speech_with(&task, input, |_, _| panic!(
            "raw numbers must not reach inference"
        ))
        .unwrap_err()
        .contains("numbers"));
        let mut abbreviation = speech_input(bytes.clone(), None, None, None);
        abbreviation.text = "ዶ/ር አበበ እና ኢ/ር ሀና".into();
        assert!(render_speech_with(&task, abbreviation, |_, _| panic!(
            "unexpanded abbreviations must not reach inference"
        ))
        .unwrap_err()
        .contains("abbreviations"));
        let mut input = speech_input(bytes, None, None, None);
        input.text = "ዶክተር አበበ እና ኢንጂነር ሀና አስራ ሁለት".into();
        input.original_text = Some("ዶ/ር አበበ እና ኢ/ር ሀና 12".into());
        let result = render_speech_with(&task, input, |task, request| {
            assert_eq!(request["text"], "ዶክተር አበበ እና ኢንጂነር ሀና አስራ ሁለት");
            simulated_render(task, request)
        })
        .unwrap();
        let versions =
            crate::speech_versions::versions(&storage.0, result["groupId"].as_str().unwrap())
                .unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0]["text"], "ዶክተር አበበ እና ኢንጂነር ሀና አስራ ሁለት");
        assert_eq!(versions[0]["originalText"], "ዶ/ር አበበ እና ኢ/ር ሀና 12");
        assert_eq!(versions[0]["textPreparation"]["version"], "am-speech-v2");
    }
    #[test]
    fn speech_regeneration_preserves_audio_metadata_and_saved_reference_for_every_version() {
        let storage = Storage::new();
        let state = EditingState::new(storage.0.clone());
        let bytes = speech_fixture(&storage.0);
        let task = state.task().unwrap();
        let first = render_speech_with(
            &task,
            speech_input(bytes.clone(), None, None, None),
            simulated_render,
        )
        .unwrap();
        let id = first["projectId"].as_str().unwrap();
        let path = project_dir(&task.root, id).unwrap();
        let before = [
            "original.wav",
            "synthesis.wav",
            "project.json",
            "synthesis-context.json",
        ]
        .map(|name| fs::read(path.join(name)).unwrap());
        let mut input = speech_input(bytes.clone(), Some(id), Some(id), Some(id));
        input.text = "እንዴት ነህ".into();
        input.seed = 99;
        input.voice.label = "Another voice".into();
        let second = render_speech_with(&task, input, simulated_render).unwrap();
        assert_eq!(second["groupId"], id);
        assert_ne!(second["projectId"], id);
        assert_eq!(
            before,
            [
                "original.wav",
                "synthesis.wav",
                "project.json",
                "synthesis-context.json"
            ]
            .map(|name| fs::read(path.join(name)).unwrap())
        );
        let versions = crate::speech_versions::versions(&storage.0, id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0]["text"], "ሰላም");
        assert_eq!(versions[1]["seed"], 99);
        assert_eq!(versions[1]["reference"]["voice"]["label"], "Another voice");
        assert_eq!(versions[0]["result"]["revision"], "pinned");
        fs::remove_file(storage.0.join("input.wav")).unwrap();
        assert_eq!(fs::read(path.join("original.wav")).unwrap(), bytes);
        let stale = render_speech_with(
            &task,
            speech_input(bytes.clone(), Some(id), Some(id), Some(id)),
            |_, _| panic!("stale request must not render"),
        );
        assert!(stale.unwrap_err().contains("changed"));
        let latest = second["projectId"].as_str().unwrap();
        let failed = render_speech_with(
            &task,
            speech_input(bytes, Some(id), Some(latest), Some(id)),
            |_, _| Err("worker failed".into()),
        );
        assert!(failed.is_err());
        assert_eq!(
            crate::speech_versions::versions(&storage.0, id).unwrap(),
            versions
        );
        assert_eq!(fs::read_dir(task.root.join("projects")).unwrap().count(), 2);
    }
    #[test]
    #[ignore = "requires the prepared Mac CPU speech runtime"]
    fn real_speech_versions_keep_prior_audio_and_share_the_resident_model() {
        let root = PathBuf::from(std::env::var("ETHIOPIC_TTS_TEST_ROOT").unwrap());
        let state = EditingState::new(root.clone());
        let m = manifest(&state.root, false).unwrap();
        let bytes = fs::read(
            models_dir(&state.root, &m)
                .unwrap()
                .join("tts-flow_lm-90m-amh-fp32/voices/default.wav"),
        )
        .unwrap();
        let task = state.task().unwrap();
        let _queue = recognition_lock(&task.root).unwrap();
        let first = render_speech(&task, speech_input(bytes.clone(), None, None, None)).unwrap();
        let id = first["projectId"].as_str().unwrap();
        let path = project_dir(&task.root, id).unwrap();
        let audio = fs::read(path.join("synthesis.wav")).unwrap();
        let context = fs::read(path.join("synthesis-context.json")).unwrap();
        let pid = task.pool.lock().unwrap().as_ref().unwrap().child.id();
        let mut input = speech_input(bytes, Some(id), Some(id), Some(id));
        input.text = "ዛሬ አስራ ሁለት ሰዎች መጡ።".into();
        input.original_text = Some("ዛሬ 12 ሰዎች መጡ።".into());
        input.seed = 43;
        render_speech(&task, input).unwrap();
        assert_eq!(pid, task.pool.lock().unwrap().as_ref().unwrap().child.id());
        assert_eq!(audio, fs::read(path.join("synthesis.wav")).unwrap());
        assert_eq!(
            context,
            fs::read(path.join("synthesis-context.json")).unwrap()
        );
        let versions = crate::speech_versions::versions(&root, id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[1]["result"]["model"], "tts-flow_lm-90m-amh-fp32");
        assert_eq!(versions[1]["settings"]["seed"], 43);
        assert_eq!(versions[1]["originalText"], "ዛሬ 12 ሰዎች መጡ።");
        assert_eq!(versions[1]["text"], "ዛሬ አስራ ሁለት ሰዎች መጡ።");
        println!(
            "{}",
            json!({"groupId":id,"versions":versions.len(),"workerPid":pid,"priorAudioUnchanged":true,"model":versions[1]["result"]})
        );
        drop(task);
        state.shutdown();
    }
    struct Storage(PathBuf);
    impl Storage {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("voice-edit-test-{}", identifier()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Storage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn editing_uses_the_same_cross_process_queue_as_recognition() {
        let storage = Storage::new();
        let state = EditingState::new(storage.0.clone());
        let first = recognition_lock(&state.root).unwrap();
        assert!(recognition_lock(&state.root).is_err());
        drop(first);
        assert!(recognition_lock(&state.root).is_ok());
        let task = state.task().unwrap();
        assert!(state.task().is_err());
        state.cancel.store(true, Ordering::SeqCst);
        drop(task);
        let next = state.task().unwrap();
        assert!(!next.cancel.load(Ordering::SeqCst));
    }
    #[test]
    fn history_changes_are_blocked_by_processing_and_hold_the_queue_until_complete() {
        let storage = Storage::new();
        let state = EditingState::new(storage.0.clone());
        let task = state.task().unwrap();
        assert!(state.change_history(|| Ok(())).is_err());
        drop(task);
        let processing = recognition_lock(&state.root).unwrap();
        assert!(state.change_history(|| Ok(())).is_err());
        drop(processing);
        state
            .change_history(|| {
                assert!(state.task().is_err());
                assert!(recognition_lock(&state.root).is_err());
                Ok(())
            })
            .unwrap();
        assert!(state.task().is_ok());
        assert!(recognition_lock(&state.root).is_ok());
    }

    #[test]
    fn changed_or_cancelled_model_files_are_never_accepted_from_a_receipt() {
        let storage = Storage::new();
        let root = &storage.0;
        let directory = root.join("models/revision");
        owned_dir(root, &directory.join("fixture")).unwrap();
        let file = directory.join("fixture/weights.bin");
        fs::write(&file, b"abc").unwrap();
        let sha = format!("{:x}", Sha256::digest(b"abc"));
        let cancel = AtomicBool::new(false);
        verify_model_file(root, &directory, "fixture/weights.bin", &sha, &cancel).unwrap();
        verify_model_file(root, &directory, "fixture/weights.bin", &sha, &cancel).unwrap();
        fs::write(&file, b"bad").unwrap();
        assert!(verify_model_file(root, &directory, "fixture/weights.bin", &sha, &cancel).is_err());
        fs::write(&file, b"abc").unwrap();
        cancel.store(true, Ordering::SeqCst);
        assert!(verify_model_file(root, &directory, "fixture/weights.bin", &sha, &cancel).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn redirected_project_and_worker_directories_are_refused() {
        use std::os::unix::fs::symlink;
        let storage = Storage::new();
        let outside = Storage::new();
        let root = storage.0.join("voice-edit");
        owned_dir(&root, &root).unwrap();
        symlink(&outside.0, root.join("projects")).unwrap();
        assert!(project_dir(&root, "safe-id").is_err());
        symlink(&outside.0, root.join("provider")).unwrap();
        assert!(owned_dir(&root, &root.join("provider")).is_err());
        assert!(project_dir(&root, "../outside").is_err());
    }
    #[test]
    fn apple_generation_does_not_depend_on_editor_support_or_an_8gb_gate() {
        let m = Manifest::parse(BUNDLED).unwrap();
        let h = Hardware {
            os: "macos".into(),
            arch: "aarch64".into(),
            total_ram_bytes: 4 * 1024 * 1024 * 1024,
            free_ram_bytes: 2 * 1024 * 1024 * 1024,
            physical_cores: Some(4),
            avx2: None,
            nvidia: vec![],
        };
        assert!(small_tts_selected(&m, &h));
        assert!(!m.select_editing(&h, false).unwrap().available);
        let p = small_tts_plan(&m, &h).unwrap();
        assert!(p.available);
        assert_eq!(p.tts.as_deref(), Some("tts-flow_lm-90m-amh-fp32"));
        assert_eq!(p.downloads, vec!["tts-flow_lm-90m-amh-fp32"]);
        assert_eq!(p.device.as_deref(), Some("cpu"));
    }
    #[test]
    #[ignore = "requires a separately prepared, real Mac CPU TTS runtime and HF files"]
    fn real_small_tts_native_queue_and_model_verification() {
        let root = PathBuf::from(
            std::env::var("ETHIOPIC_TTS_TEST_ROOT").expect("prepared native test data"),
        );
        let state = EditingState::new(root.clone());
        owned_dir(&state.root, &state.root).unwrap();
        save_small(&state.root.join("consent.json"), &json!({"version":1})).unwrap();
        // Force the native HF/CDN path to fetch both the voice and complete checkpoint.
        let m = manifest(&state.root, false).unwrap();
        let models = models_dir(&state.root, &m).unwrap();
        let default = models.join("tts-flow_lm-90m-amh-fp32/voices/default.wav");
        fs::remove_file(&default).unwrap();
        fs::remove_file(models.join("tts-flow_lm-90m-amh-fp32/model.safetensors")).unwrap();
        {
            let task = state.task().unwrap();
            prepare_small_tts(&task).unwrap();
        }
        let status = generation_status(&state).unwrap();
        assert!(status.ready);
        assert!(status.default_voice);
        assert!(!status.requires_reference_text);
        let id = "native-tts-proof";
        let directory = project_dir(&state.root, id).unwrap();
        owned_dir(&state.root, &directory).unwrap();
        let source = fs::read(&default).unwrap();
        fs::write(directory.join("original.wav"), &source).unwrap();
        save_small(&directory.join("project.json"),&json!({"id":id,"name":"Native TTS","revision":0,"head":0,"created":1,"duration":3.6,"channels":1,"versions":[{"audio":"original.wav","text":"","words":[],"regions":[],"kind":"import","parent":null}]})).unwrap();
        let task = state.task().unwrap();
        let _queue = recognition_lock(&state.root).unwrap();
        let result = request(
            &task,
            json!({"action":"synthesize","projectId":id,"revision":0,"text":"ሰላም ጤና ይስጥልኝ።"}),
        )
        .unwrap();
        assert_eq!(result["model"], "tts-flow_lm-90m-amh-fp32");
        assert_eq!(result["sample_rate"], 24000);
        let pid = task.pool.lock().unwrap().as_ref().unwrap().child.id();
        let second = request(
            &task,
            json!({"action":"synthesize","projectId":id,"revision":0,"text":"ዛሬ እንዴት ናችሁ።","seed":42,"generationOptions":{"temperature":0.4,"sentencePause":0.5}}),
        )
        .unwrap();
        assert!(second["duration"].as_f64().unwrap() > 0.);
        assert_eq!(
            second["settings"],
            json!({"schemaVersion":1,"temperature":0.4,"sentencePause":0.5,"seed":42})
        );
        assert_eq!(pid, task.pool.lock().unwrap().as_ref().unwrap().child.id());
        assert_eq!(fs::read(directory.join("original.wav")).unwrap(), source);
        assert!(directory.join("synthesis.wav").is_file());
        let before = fs::read(directory.join("project.json")).unwrap();
        let cancel = task.cancel.clone();
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            cancel.store(true, Ordering::SeqCst);
        });
        let stopped = request(
            &task,
            json!({"action":"synthesize","projectId":id,"revision":0,"text":"ሰላም ጤና ይስጥልኝ። ".repeat(100)}),
        );
        cancel_thread.join().unwrap();
        assert!(stopped.unwrap_err().contains("stopped"));
        assert_eq!(before, fs::read(directory.join("project.json")).unwrap());
        assert_eq!(source, fs::read(directory.join("original.wav")).unwrap());
        assert!(task.pool.lock().unwrap().is_none());
        drop(task);
        state.shutdown();
    }
    #[test]
    fn small_gpu_fallback_uses_the_cpu_runtime_profile_and_does_not_claim_gpu_memory_credit() {
        let m = Manifest::parse(BUNDLED).unwrap();
        let mut h = Hardware {
            os: "windows".into(),
            arch: "x86_64".into(),
            total_ram_bytes: 32 * 1024 * 1024 * 1024,
            free_ram_bytes: 20 * 1024 * 1024 * 1024,
            physical_cores: Some(8),
            avx2: Some(true),
            nvidia: vec![resource_packs::studio::Nvidia {
                index: 0,
                compute_major: 7,
                compute_minor: 5,
                total_vram_bytes: 4 * 1024 * 1024 * 1024,
                free_vram_bytes: 4 * 1024 * 1024 * 1024,
            }],
        };
        assert!(small_tts_selected(&m, &h));
        assert_eq!(
            small_tts_plan(&m, &h).unwrap().tier,
            resource_packs::studio::Tier::CpuHigh
        );
        h.nvidia[0].compute_major = 8;
        h.nvidia[0].total_vram_bytes = 8 * 1024 * 1024 * 1024;
        h.nvidia[0].free_vram_bytes = 1024 * 1024 * 1024;
        let state = EditingState::new(std::env::temp_dir().join("ethiopic-routing-readonly"));
        *state.resident.lock().unwrap() = Some((
            m.hf_revision.clone().unwrap(),
            "tts-flow_lm-90m-amh-fp32".into(),
        ));
        assert!(
            !selection_for_hardware(&state, &m, &h).unwrap().available,
            "a CPU model must not make externally occupied GPU memory look free"
        );
    }
    #[test]
    #[ignore = "requires the locally installed preview resources; read-only hardware/routing smoke check"]
    fn real_hardware_routing_snapshot() {
        let root = PathBuf::from(
            std::env::var("ETHIOPIC_SETUP_TEST_ROOT").expect("installed preview data"),
        );
        let h = hardware();
        let m = setup_manifest(&root).unwrap();
        let plan = resource_packs::routing::route(&m, &h, false, false).unwrap();
        crate::hardware_setup::platform_check(&h, sysinfo::System::os_version().as_deref())
            .unwrap();
        assert_eq!(
            plan.speech.variant.as_deref(),
            Some("tts-flow_lm-90m-amh-fp32")
        );
        let state = EditingState::new(root.clone());
        let status = generation_status_for_hardware(&state, &m, &h).unwrap();
        assert!(status.ready);
        crate::hardware_setup::validate_download(&root, &m, &h, &plan.speech.downloads, 0).unwrap();
        println!(
            "{}",
            json!({"profile":plan.tier,"os":h.os,"arch":h.arch,"totalRamBytes":h.total_ram_bytes,"physicalCores":h.physical_cores,"transcription":plan.transcription.variant,"dictation":plan.dictation.variant,"speech":plan.speech.variant,"installedSpeechReady":status.ready,"warnings":plan.warnings})
        );
    }
}
