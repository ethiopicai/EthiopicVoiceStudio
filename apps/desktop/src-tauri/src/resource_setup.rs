//! Guided resource setup, separate from the strictly offline inference workers.
use fs2::FileExt;
use resource_packs::studio::{Hardware, Manifest};
use resource_packs::{Catalog, Release};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::State;

const CATALOG: &str = include_str!("../../resources/asr-catalog.json");
const ROUTES: &str = include_str!("../../resources/asr-routes.json");
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecognitionRoutes {
    schema_version: u32,
    releases: Vec<RecognitionRoute>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecognitionRoute {
    #[serde(default)]
    profile: Option<String>,
    model_version: String,
    hf_revision: String,
    transcription: String,
    dictation: String,
}
fn selected_release<'a>(
    catalog: &'a Catalog,
    routes: &RecognitionRoutes,
    m: &Manifest,
    h: &Hardware,
) -> Option<&'a Release> {
    if routes.schema_version != 1 {
        return None;
    }
    let batch = m
        .select_for_setup(h, resource_packs::studio::Task::Transcription)
        .ok()?
        .variant?;
    let live = m
        .select_for_setup(h, resource_packs::studio::Task::Dictation)
        .ok()?
        .variant?;
    catalog.releases.iter().find(|release| {
        release.os == h.os
            && release.arch == h.arch
            && release.profile.as_deref().is_none_or(|profile| {
                h.os != "windows" || profile == resource_packs::studio::tier(h).name()
            })
            && routes.releases.iter().any(|route| {
                route.profile == release.profile
                    && route.model_version == release.model_version
                    && Some(route.hf_revision.as_str()) == m.hf_revision.as_deref()
                    && route.transcription == batch
                    && route.dictation == live
            })
    })
}
pub(crate) fn has_hardware_release(m: &Manifest, h: &Hardware) -> bool {
    let (Ok(catalog), Ok(routes)) = (
        Catalog::parse(CATALOG),
        serde_json::from_str::<RecognitionRoutes>(ROUTES),
    ) else {
        return false;
    };
    selected_release(&catalog, &routes, m, h).is_some()
}
pub(crate) fn required_storage_for_hardware(
    root: &Path,
    m: &Manifest,
    h: &Hardware,
) -> Option<u64> {
    let catalog = Catalog::parse(CATALOG).ok()?;
    let routes = serde_json::from_str::<RecognitionRoutes>(ROUTES).ok()?;
    selected_release(&catalog, &routes, m, h).map(|release| required_storage(root, release))
}
fn required_storage(root: &Path, release: &Release) -> u64 {
    release
        .packs
        .iter()
        .fold(0_u64, |total, pack| {
            total
                .saturating_add(pack.unpacked_size)
                .saturating_add(pack.size)
        })
        .saturating_sub(cached_bytes(root, release))
        .saturating_add(256 * 1024 * 1024)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    state: String,
    stage: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    can_install: bool,
    model_version: Option<String>,
    error: Option<String>,
}
pub struct SetupState {
    root: PathBuf,
    progress: Arc<Mutex<Progress>>,
    paused: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}
impl SetupState {
    pub fn new(root: PathBuf) -> Self {
        let selected = Catalog::parse(CATALOG).ok().and_then(|catalog| {
            if catalog.releases.is_empty() {
                return None;
            }
            let routes = serde_json::from_str::<RecognitionRoutes>(ROUTES).ok()?;
            let m = crate::voice_editing::setup_manifest(&root).ok()?;
            selected_release(&catalog, &routes, &m, &crate::hardware::inspect()).cloned()
        });
        let total = selected
            .as_ref()
            .map(|r| r.packs.iter().map(|p| p.size).sum())
            .unwrap_or(0);
        let bytes = selected
            .as_ref()
            .map(|r| cached_bytes(&root, r))
            .unwrap_or(0);
        let installed = selected.as_ref().is_some_and(|release| {
            fs::read(root.join("asr/active/installation.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|config| {
                    config["schema_version"] == 1
                        && (config["model_version"] == release.model_version
                            || config["model_version"] == "v1.0")
                        && config["hardware_profile"].as_str() == release.profile.as_deref()
                        && config["os"] == release.os
                        && config["arch"] == release.arch
                })
        });
        let supported = crate::hardware_setup::platform_check(
            &crate::hardware::inspect(),
            sysinfo::System::os_version().as_deref(),
        )
        .is_ok();
        Self {
            root,
            progress: Arc::new(Mutex::new(Progress {
                state: if installed { "installed" } else if bytes > 0 { "paused" } else { "idle" }.into(),
                stage: if !supported { "Voice setup supports Apple Silicon Macs and 64-bit Windows 10 or later." }
                    else if selected.is_none() { "Downloads are not published for this build yet. Existing installed voice tools still work offline." }
                    else if installed { "Voice tools are installed on this computer. Choose Repair if something needs attention." }
                    else if bytes > 0 { "Your download is saved. Continue whenever you are ready." }
                    else { "Set up Amharic transcription and dictation on this computer." }.into(),
                downloaded_bytes: if installed { total } else { bytes }, total_bytes: total,
                can_install: supported && selected.is_some(),
                model_version: selected.map(|r| r.model_version), error: None,
            })),
            paused: Arc::new(AtomicBool::new(false)), running: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }
}
fn cache(root: &Path) -> PathBuf {
    root.join("resource-downloads")
}
fn cached_bytes(root: &Path, release: &Release) -> u64 {
    release
        .packs
        .iter()
        .map(|p| {
            let base = cache(root).join(&p.sha256);
            ["download.tar.gz", "download.part"]
                .iter()
                .filter_map(|name| {
                    fs::metadata(base.join(name))
                        .ok()
                        .map(|m| m.len().min(p.size))
                })
                .max()
                .unwrap_or(0)
        })
        .sum()
}
fn update(progress: &Mutex<Progress>, state: &str, stage: &str, downloaded: Option<u64>) {
    if let Ok(mut p) = progress.lock() {
        p.state = state.into();
        p.stage = stage.into();
        if let Some(bytes) = downloaded {
            p.downloaded_bytes = bytes;
        }
    }
}
#[tauri::command]
pub fn get_resource_setup(state: State<'_, SetupState>) -> Result<Progress, String> {
    state
        .progress
        .lock()
        .map(|p| p.clone())
        .map_err(|_| "Setup status is unavailable".into())
}
#[tauri::command]
pub fn pause_resource_setup(state: State<'_, SetupState>) {
    state.pause();
}
#[tauri::command]
pub fn start_resource_setup(
    app: tauri::AppHandle,
    state: State<'_, SetupState>,
    approved_plan: String,
) -> Result<(), String> {
    crate::download_setup::require(&app, "recognition", &approved_plan)?;
    let h = crate::hardware::inspect();
    let m = crate::voice_editing::setup_manifest(&state.root)?;
    crate::hardware_setup::platform_check(&h, sysinfo::System::os_version().as_deref())?;
    let catalog = Catalog::parse(CATALOG)?;
    let routes: RecognitionRoutes = serde_json::from_str(ROUTES)
        .map_err(|_| "Recognition download routes need an app update")?;
    let release=selected_release(&catalog,&routes,&m,&h).cloned().ok_or("A compatible recognition runtime for this computer's selected models has not been published yet. Existing installed tools are unchanged.")?;
    let mut downloads = m
        .select_for_setup(&h, resource_packs::studio::Task::Transcription)?
        .downloads;
    for id in m
        .select_for_setup(&h, resource_packs::studio::Task::Dictation)?
        .downloads
    {
        if !downloads.contains(&id) {
            downloads.push(id);
        }
    }
    crate::hardware_setup::validate_download(&state.root, &m, &h, &downloads, 0)?;
    if state
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("Setup is already running".into());
    }
    state.paused.store(false, Ordering::SeqCst);
    if let Ok(mut p) = state.progress.lock() {
        p.error = None;
        p.total_bytes = release.packs.iter().map(|pack| pack.size).sum();
        p.downloaded_bytes = cached_bytes(&state.root, &release);
        p.model_version = Some(release.model_version.clone());
        p.can_install = true;
    }
    update(
        &state.progress,
        "preparing",
        "Making a little room for your voice tools…",
        None,
    );
    let root = state.root.clone();
    let progress = state.progress.clone();
    let paused = state.paused.clone();
    let running = state.running.clone();
    std::thread::spawn(move || {
        let result = install(&root, &release, &paused, &progress);
        if let Err(error) = result {
            let is_pause = error == "paused";
            update(
                &progress,
                if is_pause { "paused" } else { "failed" },
                if is_pause {
                    "Your download is saved. Continue whenever you are ready."
                } else {
                    "Setup stopped. Your recordings and previous voice tools are safe."
                },
                Some(cached_bytes(&root, &release)),
            );
            if !is_pause {
                if let Ok(mut p) = progress.lock() {
                    p.error = Some(error);
                }
            }
        }
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}
pub(crate) fn download_quote(root: &Path) -> Result<crate::download_setup::Plan, String> {
    use crate::download_setup::{Item, Plan};
    let h = crate::hardware::inspect();
    let m = crate::voice_editing::setup_manifest(root)?;
    let catalog = Catalog::parse(CATALOG)?;
    let routes: RecognitionRoutes =
        serde_json::from_str(ROUTES).map_err(|_| "Recognition download details unavailable")?;
    let platform =
        crate::hardware_setup::platform_check(&h, sysinfo::System::os_version().as_deref());
    let release = selected_release(&catalog, &routes, &m, &h);
    let available = platform.is_ok() && release.is_some();
    let reason = platform.err().unwrap_or_else(|| {
        if release.is_none() {
            "Compatible recognition downloads have not been released for this computer yet.".into()
        } else {
            "".into()
        }
    });
    let items = release
        .map(|r| {
            r.packs
                .iter()
                .map(|pack| Item {
                    name: format!("{} pack", pack.kind),
                    purpose: if pack.kind == "runtime" {
                        "Speech tools for this computer"
                    } else {
                        "Amharic transcription and dictation resources"
                    }
                    .into(),
                    bytes: pack.size,
                    url: pack.url.clone(),
                    sha256: pack.sha256.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    Plan::new(
        "recognition",
        "Transcription and Dictation",
        available,
        reason,
        items,
    )
}

fn guided_release(root: &Path) -> Result<Release, String> {
    let h = crate::hardware::inspect();
    crate::hardware_setup::platform_check(&h, sysinfo::System::os_version().as_deref())?;
    let manifest = crate::voice_editing::setup_manifest(root)?;
    let catalog = Catalog::parse(CATALOG)?;
    let routes: RecognitionRoutes = serde_json::from_str(ROUTES)
        .map_err(|_| "Recognition download details need an app update")?;
    selected_release(&catalog, &routes, &manifest, &h)
        .cloned()
        .ok_or("The recognition tools for this computer are not available yet.".into())
}

pub(crate) fn prefetch_guided(
    root: &Path,
    cancel: &AtomicBool,
    report: impl Fn(u64),
) -> Result<(), String> {
    let release = guided_release(root)?;
    fs::create_dir_all(root).map_err(|_| "Could not prepare voice storage")?;
    if fs2::available_space(root).map_err(|_| "Could not check free storage")?
        < required_storage(root, &release)
    {
        return Err(
            "There is not enough storage to prepare the voice tools. Free some space and retry."
                .into(),
        );
    }
    let mut completed = 0;
    for artifact in &release.packs {
        resource_packs::download(artifact, &cache(root), cancel, |bytes| {
            report(completed + bytes)
        })?;
        completed += artifact.size;
        report(completed);
    }
    Ok(())
}

pub(crate) fn install_guided(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let release = guided_release(root)?;
    let progress = Mutex::new(Progress {
        state: "preparing".into(),
        stage: String::new(),
        downloaded_bytes: 0,
        total_bytes: release.packs.iter().map(|p| p.size).sum(),
        can_install: true,
        model_version: Some(release.model_version.clone()),
        error: None,
    });
    install(root, &release, cancel, &progress)
}

pub(crate) fn check_guided(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("recognition.lock"))
        .map_err(|_| "Could not prepare the voice check")?;
    lock.try_lock_exclusive()
        .map_err(|_| "Finish your current recording, then continue setup.")?;
    crate::asr_runtime::release_for_setup()?;
    let (_version, _, _, _) = crate::asr::verified_dictation_runtime(root, cancel)?;
    let base = root.join("asr/active");
    let config: serde_json::Value = serde_json::from_slice(
        &fs::read(base.join("installation.json")).map_err(|_| "Recognition setup needs repair")?,
    )
    .map_err(|_| "Recognition setup needs repair")?;
    let work = base.join("setup-check");
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|_| "Could not restart the voice check")?;
    }
    self_test(
        &base,
        config["python"]
            .as_str()
            .ok_or("Recognition runtime is missing")?,
        config["site_packages"]
            .as_str()
            .ok_or("Recognition runtime is missing")?,
        config["model_version"]
            .as_str()
            .ok_or("Recognition model is missing")?,
        cancel,
    )
}

fn install(
    root: &Path,
    release: &Release,
    paused: &AtomicBool,
    progress: &Mutex<Progress>,
) -> Result<(), String> {
    fs::create_dir_all(root)
        .map_err(|_| "Could not prepare storage. Check free space and retry.")?;
    let setup_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("resource-setup.lock"))
        .map_err(|_| "Could not prepare voice tools")?;
    setup_lock
        .try_lock_exclusive()
        .map_err(|_| "Voice setup is already open in another studio window.")?;
    let base = root.join("asr");
    fs::create_dir_all(&base).map_err(|_| "Could not prepare voice storage")?;
    let required = required_storage(root, release);
    if fs2::available_space(root).map_err(|_| "Could not check free storage")? < required {
        return Err(format!(
            "Free at least {:.1} GB of storage, then try setup again.",
            required as f64 / 1_000_000_000.0
        ));
    }
    let mut paths = Vec::new();
    let mut completed = 0;
    for artifact in &release.packs {
        let path = resource_packs::download(artifact, &cache(root), paused, |bytes| {
            update(
                progress,
                "downloading",
                "A few bytes closer. You can pause and continue later.",
                Some(completed + bytes),
            );
        })?;
        completed += artifact.size;
        paths.push(path);
    }
    // A slow download must not interrupt existing offline recognition. Acquire its lock only
    // when all downloads are verified and we are ready to replace resources.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("recognition.lock"))
        .map_err(|_| "Could not prepare voice tools")?;
    update(
        progress,
        "preparing",
        "Downloads are ready. Finishing any current recording before setup…",
        None,
    );
    let wait_start = Instant::now();
    while lock.try_lock_exclusive().is_err() {
        if paused.load(Ordering::SeqCst) {
            return Err("paused".into());
        }
        if wait_start.elapsed() > Duration::from_secs(600) {
            return Err("Finish or cancel your current recording, then choose Continue setup. Your downloads are saved.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    crate::asr_runtime::release_for_setup()?;
    resource_packs::recover(&base)?;
    let stage = base.join("staging");
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|_| "Could not restart setup safely")?;
    }
    fs::create_dir(&stage).map_err(|_| "Could not prepare voice tools")?;
    let result = (|| {
        update(
            progress,
            "installing",
            "Downloads checked. Unpacking your voice tools…",
            Some(completed),
        );
        let mut packs = Vec::new();
        for (artifact, archive) in release.packs.iter().zip(&paths) {
            packs.push(resource_packs::extract(
                artifact,
                archive,
                &stage,
                &release.model_version,
                paused,
            )?);
        }
        let runtime = packs
            .iter()
            .find(|p| p.kind == "runtime")
            .ok_or("Private runtime is missing")?;
        let python = runtime
            .python
            .as_ref()
            .ok_or("Private runtime is incomplete")?;
        let site = runtime
            .site_packages
            .as_ref()
            .ok_or("Private runtime is incomplete")?;
        update(
            progress,
            "checking",
            "One quick practice run. Making sure everything works offline…",
            None,
        );
        let files: Vec<_> = packs
            .iter()
            .flat_map(|p| p.files.iter())
            .map(|f| serde_json::json!({"path":f.path,"sha256":f.sha256}))
            .collect();
        let config = serde_json::json!({"schema_version":1,"os":release.os,"arch":release.arch,
            "python":python,"site_packages":site,"model_version":release.model_version,
            "hardware_profile":release.profile,"files":files,"development_installation":false});
        fs::write(
            stage.join("installation.json"),
            serde_json::to_vec_pretty(&config).map_err(|_| "Invalid voice installation")?,
        )
        .map_err(|_| "Could not save voice installation")?;
        let manifest = crate::voice_editing::setup_manifest(root)?;
        crate::naming_migration::recognition(&stage, &manifest, paused)?;
        let config: serde_json::Value = serde_json::from_slice(
            &fs::read(stage.join("installation.json"))
                .map_err(|_| "Voice installation unavailable")?,
        )
        .map_err(|_| "Invalid voice installation")?;
        if let Some(profile) = crate::recognition_bundle::read(&stage.join("bundle"))? {
            let files = config["files"]
                .as_array()
                .ok_or("Voice installation incomplete")?
                .iter()
                .filter_map(|f| Some((f["path"].as_str()?, f["sha256"].as_str()?)))
                .collect();
            profile.validate_files(&stage.join("bundle"), &manifest, &files)?;
        }
        self_test(
            &stage,
            python,
            site,
            config["model_version"]
                .as_str()
                .ok_or("Model version missing")?,
            paused,
        )?;
        if paused.load(Ordering::SeqCst) {
            return Err("paused".into());
        }
        update(
            progress,
            "activating",
            "Almost there. Saving your offline voice tools…",
            None,
        );
        resource_packs::activate(&base)?;
        let _ = fs::remove_file(base.join("verification-cache.json"));
        // Completed archives are disposable; keep the previous working install for recovery.
        for path in paths {
            let _ = fs::remove_file(path);
        }
        update(
            progress,
            "completed",
            "Ready! Transcription and dictation now work on this computer offline.",
            Some(completed),
        );
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
fn self_test(
    stage: &Path,
    python: &str,
    site: &str,
    version: &str,
    paused: &AtomicBool,
) -> Result<(), String> {
    let work = stage.join("setup-check");
    fs::create_dir(&work).map_err(|_| "Could not prepare voice check")?;
    let audio = work.join("silence.wav");
    let output = work.join("result.json");
    let worker = work.join("setup_check.py");
    crate::recognition_bundle::stage_workers(&work)?;
    for (name, source) in [
        (
            "worker.py",
            include_str!("../../../../workers/asr/worker.py"),
        ),
        (
            "dictation_worker.py",
            include_str!("../../../../workers/asr/dictation_worker.py"),
        ),
        (
            "resident_worker.py",
            include_str!("../../../../workers/asr/resident_worker.py"),
        ),
        (
            "setup_check.py",
            include_str!("../../../../workers/asr/setup_check.py"),
        ),
    ] {
        fs::write(work.join(name), source).map_err(|_| "Could not prepare voice check")?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav =
        hound::WavWriter::create(&audio, spec).map_err(|_| "Could not prepare voice check")?;
    for _ in 0..4000 {
        wav.write_sample(0_i16)
            .map_err(|_| "Could not prepare voice check")?;
    }
    wav.finalize()
        .map_err(|_| "Could not prepare voice check")?;
    if paused.load(Ordering::SeqCst) {
        return Err("paused".into());
    }
    let mut child = crate::background_process::command(stage.join(python)).args(["-I","-X","utf8","-B","-u","-c",
        "import runpy,sys,os;sys.path.insert(0,sys.argv.pop(1));worker=sys.argv.pop(1);sys.path.insert(0,os.path.dirname(worker));runpy.run_path(worker,run_name='__main__')"])
        .arg(stage.join(site)).arg(worker).arg("--bundle").arg(stage.join("bundle"))
        .arg("--audio").arg(audio).arg("--output").arg(&output)
        .env("ORT_DISABLE_TELEMETRY", "1").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1")
        .env("HF_HOME",work.join("private-cache/hf")).env("HF_HUB_CACHE",work.join("private-cache/hub"))
        .env("TORCH_HOME",work.join("private-cache/torch")).env("XDG_CACHE_HOME",work.join("private-cache/xdg"))
        .env("ETHIOPIC_MANAGED_WORKER","1").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|_| "The private voice engine could not start. Choose Retry to repair it.")?;
    let start = Instant::now();
    let success = loop {
        if paused.load(Ordering::SeqCst) || start.elapsed() > Duration::from_secs(180) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if paused.load(Ordering::SeqCst) {
                "paused"
            } else {
                "Voice tools took too long to start. Close other applications and retry."
            }
            .into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The voice check stopped. Please retry.".into());
            }
        }
    };
    let result = fs::read(&output)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    if let Some(message) = result
        .as_ref()
        .and_then(|r| r["error"].as_str())
        .and_then(crate::recognition_bundle::provider_error)
    {
        return Err(message.into());
    }
    if !success
        || !result.is_some_and(|r| {
            r["model_version"].as_str() == Some(version)
                && r["words"].is_array()
                && r["streaming_checked"] == true
        })
    {
        return Err("These voice tools did not pass the offline check. Your previous installation is safe; choose Retry.".into());
    }
    fs::remove_dir_all(work).map_err(|_| "Could not finish voice check")?;
    Ok(())
}

pub fn recover_on_start(root: &Path) -> Result<(), String> {
    if !root.join("asr/previous").is_dir() || root.join("asr/active").exists() {
        return Ok(());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("recognition.lock"))
        .map_err(|_| "Voice recovery is unavailable")?;
    // An active setup in another window owns recovery; never race it.
    if lock.try_lock_exclusive().is_ok() {
        resource_packs::recover(&root.join("asr"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_catalog_has_no_mutable_or_unknown_resources() {
        Catalog::parse(CATALOG).expect("The app must never embed a malformed resource catalogue");
    }
    #[test]
    fn storage_review_includes_runtime_and_unpacking_and_reuses_cached_bytes() {
        let root =
            std::env::temp_dir().join(format!("ethiopic-setup-space-{}", std::process::id()));
        let release = Release {
            profile: Some("cpu-low".into()),
            os: "windows".into(),
            arch: "x86_64".into(),
            minimum_macos: String::new(),
            minimum_windows: Some("10.0.19041".into()),
            model_version: "v0.3".into(),
            packs: vec![resource_packs::Artifact {
                kind: "runtime".into(),
                url: String::new(),
                sha256: "a".repeat(64),
                size: 100,
                unpacked_size: 300,
            }],
        };
        assert_eq!(required_storage(&root, &release), 400 + 256 * 1024 * 1024);
        let saved = cache(&root).join(&release.packs[0].sha256);
        fs::create_dir_all(&saved).unwrap();
        fs::write(saved.join("download.part"), [0_u8; 40]).unwrap();
        assert_eq!(required_storage(&root, &release), 360 + 256 * 1024 * 1024);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "downloads the actual published recognition archives into a fresh temporary installation"]
    fn published_recognition_downloads_install_and_run_offline() {
        assert_eq!(
            std::env::var("ETHIOPIC_TEST_PUBLIC_DOWNLOADS").as_deref(),
            Ok("1"),
            "Explicitly enable the public download integration test"
        );
        let manifest =
            Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        let hardware = crate::hardware::inspect();
        crate::hardware_setup::platform_check(&hardware, sysinfo::System::os_version().as_deref())
            .unwrap();
        let catalog = Catalog::parse(CATALOG).unwrap();
        let routes: RecognitionRoutes = serde_json::from_str(ROUTES).unwrap();
        let release = match std::env::var("ETHIOPIC_ASR_PUBLIC_TEST_PROFILE") {
            Ok(profile) => catalog.releases.iter().find(|release| {
                release.os == hardware.os && release.arch == hardware.arch
                    && release.profile.as_deref() == Some(profile.as_str())
            }).expect("The requested public test profile must match this operating system and architecture"),
            Err(_) => selected_release(&catalog, &routes, &manifest, &hardware)
                .expect("A published release must match this computer"),
        };
        let root = std::env::temp_dir().join(format!(
            "ethiopic-public-recognition-{}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("Use a fresh public download test directory");
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let state = SetupState::new(root.clone());
        install(&root, release, &AtomicBool::new(false), &state.progress)
            .expect("Public downloads, hashes, extraction and offline checks must all succeed");
        assert_eq!(state.progress.lock().unwrap().state, "completed");
        crate::asr::verified_dictation_runtime(&root, &AtomicBool::new(false)).unwrap();
    }
    #[test]
    fn windows_profiles_select_the_exact_routed_models_and_runtime() {
        let manifest =
            Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        for (profile, ram, cores, avx2) in [
            ("cpu-low", 4, 2, false),
            ("cpu-mid", 8, 4, true),
            ("cpu-high", 32, 8, true),
        ] {
            let hardware = Hardware {
                os: "windows".into(),
                arch: "x86_64".into(),
                total_ram_bytes: ram * 1024 * 1024 * 1024,
                free_ram_bytes: ram * 1024 * 1024 * 1024,
                physical_cores: Some(cores),
                avx2: Some(avx2),
                nvidia: vec![],
            };
            let batch = manifest
                .select_for_setup(&hardware, resource_packs::studio::Task::Transcription)
                .unwrap()
                .variant
                .unwrap();
            let live = manifest
                .select_for_setup(&hardware, resource_packs::studio::Task::Dictation)
                .unwrap()
                .variant
                .unwrap();
            let mut catalog = Catalog {
                schema_version: 1,
                releases: vec![Release {
                    profile: Some(profile.into()),
                    os: "windows".into(),
                    arch: "x86_64".into(),
                    minimum_macos: String::new(),
                    minimum_windows: Some("10.0.19041".into()),
                    model_version: "v0.3".into(),
                    packs: vec![],
                }],
            };
            let routes = RecognitionRoutes {
                schema_version: 1,
                releases: vec![RecognitionRoute {
                    profile: Some(profile.into()),
                    model_version: "v0.3".into(),
                    hf_revision: manifest.hf_revision.clone().unwrap(),
                    transcription: batch,
                    dictation: live,
                }],
            };
            assert!(
                selected_release(&catalog, &routes, &manifest, &hardware).is_some(),
                "{profile}"
            );
            catalog.releases[0].profile = Some("gpu-modern".into());
            assert!(selected_release(&catalog, &routes, &manifest, &hardware).is_none());
        }
    }
    #[test]
    #[ignore = "requires locally prepared release archives; loads the real ASR model offline"]
    fn local_release_packs_pass_guided_install_and_offline_model_check() {
        let packs = PathBuf::from(
            std::env::var("ETHIOPIC_ASR_RELEASE_PACKS").expect("Set the local pack directory"),
        );
        let metadata_path = if packs.join("pack-build.json").exists() {
            packs.join("pack-build.json")
        } else {
            packs.join("pack-build-windows.json")
        };
        let raw: serde_json::Value =
            serde_json::from_slice(&fs::read(metadata_path).unwrap()).unwrap();
        let metadata = if raw["releases"].is_array() {
            let profile = std::env::var("ETHIOPIC_ASR_TEST_PROFILE")
                .expect("Select the Windows profile being qualified");
            raw["releases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["profile"] == profile)
                .expect("Profile missing from release metadata")
                .clone()
        } else {
            raw
        };
        let version = metadata["model_version"].as_str().unwrap().to_string();
        let artifacts: Vec<resource_packs::Artifact> = metadata["packs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                resource_packs::Artifact {
                    kind: p["kind"].as_str().unwrap().into(),
                    // Immutable-looking test URL is never fetched: verified local cache is seeded below.
                    url: format!(
                        "https://huggingface.co/ethiopic/local-test/resolve/{}/{}",
                        "a".repeat(40),
                        p["filename"].as_str().unwrap()
                    ),
                    sha256: p["sha256"].as_str().unwrap().into(),
                    size: p["size"].as_u64().unwrap(),
                    unpacked_size: p["unpacked_size"].as_u64().unwrap(),
                }
            })
            .collect();
        let root =
            std::env::temp_dir().join(format!("ethiopic-managed-setup-{}", std::process::id()));
        fs::create_dir(&root).expect("Use a fresh integration-test directory");
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        for (artifact, source) in artifacts.iter().zip(metadata["packs"].as_array().unwrap()) {
            let target = cache(&root).join(&artifact.sha256);
            fs::create_dir_all(&target).unwrap();
            fs::hard_link(
                packs.join(source["filename"].as_str().unwrap()),
                target.join("download.tar.gz"),
            )
            .unwrap();
        }
        let windows = std::env::consts::OS == "windows";
        let release = Release {
            profile: metadata["profile"].as_str().map(str::to_owned),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            minimum_macos: if windows { "" } else { "14.0" }.into(),
            minimum_windows: windows.then(|| "10.0.19041".into()),
            model_version: version.clone(),
            packs: artifacts,
        };
        let state = SetupState::new(root.clone());
        install(&root, &release, &AtomicBool::new(false), &state.progress)
            .expect("Real resource extraction/load must pass before public setup is qualified");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("asr/active/installation.json")).unwrap())
                .unwrap();
        // The archive's transport version stays immutable; v2 naming records v1.0
        // after verifying the unchanged v0.3 weights and upgrading local metadata.
        assert_eq!(
            manifest["model_version"],
            if version == "v0.3" {
                "v1.0"
            } else {
                version.as_str()
            }
        );
        assert_eq!(manifest["development_installation"], false);
        assert!(!root.join("asr/active/setup-check").exists());
        assert_eq!(state.progress.lock().unwrap().state, "completed");
        // The same production readiness and integrity checks used by dictation accept the result.
        crate::asr::verified_dictation_runtime(&root, &AtomicBool::new(false)).unwrap();
    }
    #[test]
    fn recognition_archives_must_match_the_routed_models_and_revision() {
        let m = Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        let mut h = Hardware {
            os: "macos".into(),
            arch: "aarch64".into(),
            total_ram_bytes: 8 * 1024 * 1024 * 1024,
            free_ram_bytes: 8 * 1024 * 1024 * 1024,
            physical_cores: Some(4),
            avx2: None,
            nvidia: vec![],
        };
        let catalog = Catalog {
            schema_version: 1,
            releases: vec![Release {
                profile: None,
                os: "macos".into(),
                arch: "aarch64".into(),
                model_version: "qualified-onnx-bridge".into(),
                minimum_macos: "14.0".into(),
                minimum_windows: None,
                packs: vec![],
            }],
        };
        let mut routes = RecognitionRoutes {
            schema_version: 1,
            releases: vec![RecognitionRoute {
                profile: None,
                model_version: "qualified-onnx-bridge".into(),
                hf_revision: m.hf_revision.clone().unwrap(),
                transcription: "asr-wav2vec_ctc-1b-amh-int8_onnx".into(),
                dictation: "asr-wav2vec_ctc-300m-amh-int8_onnx".into(),
            }],
        };
        assert!(selected_release(&catalog, &routes, &m, &h).is_some());
        h.os = "windows".into();
        assert!(selected_release(&catalog, &routes, &m, &h).is_none());
        h.os = "macos".into();
        routes.releases[0].dictation = "asr-wav2vec_ctc-1b-amh-fp16".into();
        assert!(selected_release(&catalog, &routes, &m, &h).is_none());
        routes.releases[0].dictation = "asr-wav2vec_ctc-300m-amh-int8_onnx".into();
        routes.releases[0].hf_revision = "0".repeat(40);
        assert!(selected_release(&catalog, &routes, &m, &h).is_none());
    }
}
