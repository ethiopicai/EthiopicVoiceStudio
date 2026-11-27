use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Cursor, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

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
    pub audio_id: String,
    pub name: String,
    pub state: String,
    pub stage: String,
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub result_id: Option<String>,
}
struct Task {
    job: Job,
    cancel: Arc<AtomicBool>,
}
pub struct CleanupState {
    root: PathBuf,
    tasks: Arc<Mutex<HashMap<String, Task>>>,
    running: Arc<AtomicBool>,
}
impl CleanupState {
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
    backend: String,
    #[serde(default)]
    validated: bool,
    adapter_sha256: String,
    files: Vec<IntegrityFile>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStatus {
    state: String,
    message: String,
    model_version: Option<String>,
}
fn resources(root: &Path) -> PathBuf {
    root.join("cleanup/active")
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
    if config.backend != "torch-cpu-reference"
        || !config.validated
        || config.model_version.trim().is_empty()
    {
        return Err("Unsupported cleanup resource package".into());
    }
    if config.files.is_empty() {
        return Err("Cleanup package checksums are missing".into());
    }
    for file in &config.files {
        inside(&root, &file.path)?;
    }
    for path in [
        "bundle/model.safetensors",
        "bundle/config.json",
        "bundle/models/generator_SEMamba_time_d4.py",
        "bundle/models/mamba_block2_SEMamba.py",
        "bundle/models/codec_module_time_d4.py",
        "bundle/models/stfts.py",
        "bundle/LICENSE",
        "bundle/version.json",
    ] {
        inside(&root, path)?;
        if !config.files.iter().any(|f| f.path == path) {
            return Err("Cleanup resource integrity manifest is incomplete".into());
        }
    }
    let version: serde_json::Value = serde_json::from_slice(
        &fs::read(inside(&root, "bundle/version.json")?)
            .map_err(|_| "Cleanup model needs repair")?,
    )
    .map_err(|_| "Cleanup model needs repair")?;
    if version["model_version"].as_str() != Some(&config.model_version)
        || version["backend"] != "torch-cpu-reference"
    {
        return Err("Cleanup model version mismatch".into());
    }
    if !config.files.iter().any(|f| f.path == config.python)
        || !config.files.iter().any(|f| {
            f.path
                .starts_with(&format!("{}/", config.site_packages.trim_end_matches('/')))
        })
    {
        return Err("Cleanup runtime integrity manifest is incomplete".into());
    }
    if config.adapter_sha256
        != format!(
            "{:x}",
            Sha256::digest(include_bytes!(
                "../../../../workers/cleanup/mamba_reference.py"
            ))
        )
    {
        return Err(
            "Voice Cleanup needs a local runtime check after this update. Repair its resources."
                .into(),
        );
    }
    let python = inside(&root, &config.python)?;
    let site = inside(&root, &config.site_packages)?;
    Ok((config, python, site))
}
#[tauri::command]
pub fn get_cleanup_status(state: State<'_, CleanupState>) -> CleanupStatus {
    match installation(&state.root) {
        Ok((config, _, _)) => {
            let mut system = sysinfo::System::new();
            system.refresh_memory();
            if system.total_memory() < 4 * 1024 * 1024 * 1024 {
                return CleanupStatus {state:"unsupported".into(),message:"This first model needs a computer with at least 4 GB of RAM. Your recordings and corrections remain available.".into(),model_version:Some(config.model_version)};
            }
            CleanupStatus {
                state: "ready".into(),
                message: "Voice Cleanup runs locally with an experimental CPU engine. Start with a short clip; processing may take several minutes.".into(),
                model_version: Some(config.model_version),
            }
        }
        Err(message) => CleanupStatus {
            state: "not_installed".into(),
            message,
            model_version: None,
        },
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
struct AudioSpec {
    sample_rate: u32,
    channels: u16,
    frames: u32,
}
fn validate_audio(bytes: &[u8]) -> Result<AudioSpec, String> {
    if bytes.len() > MAX_AUDIO || bytes.len() < 44 {
        return Err("Choose a PCM WAV recording up to 100 MB".into());
    }
    let mut reader = hound::WavReader::new(Cursor::new(bytes))
        .map_err(|_| "Choose a readable PCM WAV recording")?;
    let s = reader.spec();
    let frames = reader.duration();
    if s.sample_format != hound::SampleFormat::Int
        || ![8, 16, 24, 32].contains(&s.bits_per_sample)
        || !(1..=2).contains(&s.channels)
        || !(8000..=48000).contains(&s.sample_rate)
        || frames == 0
    {
        return Err("Use mono or stereo PCM WAV audio at 8–48 kHz".into());
    }
    let mut count = 0u64;
    for sample in reader.samples::<i32>() {
        sample.map_err(|_| "Recording contains unreadable samples")?;
        count += 1;
    }
    if count != frames as u64 * s.channels as u64 {
        return Err("Recording is incomplete".into());
    }
    Ok(AudioSpec {
        sample_rate: s.sample_rate,
        channels: s.channels,
        frames,
    })
}
fn durable(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "Could not save audio; check storage")?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "Could not save audio; check storage".into())
}
fn sync_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|_| "Could not commit audio")?;
    }
    let _ = path;
    Ok(())
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
        && name.to_lowercase().ends_with(".wav")
}
#[derive(Serialize)]
pub struct Imported {
    id: String,
    name: String,
}
#[tauri::command]
pub async fn import_cleanup_audio(
    state: State<'_, CleanupState>,
    name: String,
    bytes: Vec<u8>,
    source_media: Option<crate::recording_transfer::Context>,
) -> Result<Imported, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (name, bytes) = crate::audio_import::prepare(name, bytes)?;
        let spec = validate_audio(&bytes)?;
        if spec.frames < spec.sample_rate / 10 {
            return Err("Choose a recording at least 0.1 seconds long.".into());
        }
        if !valid_name(&name) {
            return Err("Choose a valid WAV filename".into());
        }
        let id = id();
        let parent = root.join("cleanup-recordings");
        fs::create_dir_all(&parent).map_err(|_| "Recording storage unavailable")?;
        let staging = parent.join(format!(".pending-{id}"));
        fs::create_dir(&staging).map_err(|_| "Recording storage unavailable")?;
        let guard = JobFiles(staging.clone());
        crate::recording_transfer::write_context(&staging, source_media.as_ref(), &bytes)?;
        durable(&staging.join("audio.wav"), &bytes)?;
        durable(
            &staging.join("name.json"),
            &serde_json::to_vec(&name).unwrap(),
        )?;
        sync_dir(&staging)?;
        fs::rename(&staging, parent.join(&id)).map_err(|_| "Could not commit recording")?;
        sync_dir(&parent)?;
        drop(guard);
        Ok(Imported { id, name })
    })
    .await
    .map_err(|_| "Import interrupted")?
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CleanupResult {
    pub id: String,
    pub name: String,
    pub model_version: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u32,
    pub original_file: String,
    pub cleaned_file: String,
    pub original_sha256: String,
}
fn read_result(root: &Path, id: &str) -> Result<CleanupResult, String> {
    safe_id(id)?;
    let r: CleanupResult = serde_json::from_slice(
        &fs::read(root.join("cleanup-results").join(id).join("result.json"))
            .map_err(|_| "Cleanup result unavailable")?,
    )
    .map_err(|_| "Cleanup result needs repair")?;
    if r.id != id
        || r.original_file != "original.wav"
        || r.cleaned_file != "cleaned.wav"
        || !valid_name(&r.name)
    {
        return Err("Cleanup result needs repair".into());
    }
    Ok(r)
}
#[tauri::command]
pub async fn list_cleanup_results(
    state: State<'_, CleanupState>,
) -> Result<Vec<CleanupResult>, String> {
    let parent = state.root.join("cleanup-results");
    if !parent.exists() {
        return Ok(vec![]);
    }
    let mut results = vec![];
    for e in fs::read_dir(parent).map_err(|_| "Results unavailable")? {
        let e = e.map_err(|_| "Results unavailable")?;
        let id = e.file_name().to_string_lossy().into_owned();
        if e.file_type().map_err(|_| "Results unavailable")?.is_dir() && !id.starts_with('.') {
            results.push(read_result(&state.root, &id)?);
        }
    }
    results.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(results)
}
#[derive(Serialize)]
pub struct Audio {
    name: String,
    bytes: Vec<u8>,
    #[serde(rename = "sourceMedia", skip_serializing_if = "Option::is_none")]
    source_media: Option<crate::recording_transfer::Context>,
}
#[tauri::command]
pub async fn get_cleanup_audio(
    state: State<'_, CleanupState>,
    id: String,
    kind: String,
) -> Result<Audio, String> {
    let r = read_result(&state.root, &id)?;
    let file = match kind.as_str() {
        "original" => r.original_file,
        "cleaned" => r.cleaned_file,
        _ => return Err("Invalid audio kind".into()),
    };
    let path = state.root.join("cleanup-results").join(id).join(file);
    let bytes = read_audio(&path)?;
    Ok(Audio {
        name: r.name,
        source_media: if kind == "original" {
            crate::recording_transfer::read_context(
                path.parent().ok_or("Recording folder unavailable")?,
                &bytes,
            )?
        } else {
            None
        },
        bytes,
    })
}
#[tauri::command]
pub async fn list_cleanup_recordings(
    state: State<'_, CleanupState>,
) -> Result<Vec<Imported>, String> {
    let root = state.root.join("cleanup-recordings");
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut items = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| "Recordings could not be listed")? {
        let entry = entry.map_err(|_| "Recordings could not be listed")?;
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if safe_id(&id).is_err() {
            continue;
        }
        if let Ok(bytes) = fs::read(entry.path().join("name.json")) {
            if let Ok(name) = serde_json::from_slice::<String>(&bytes) {
                if valid_name(&name) {
                    items.push(Imported { id, name });
                }
            }
        }
    }
    items.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(items)
}
#[tauri::command]
pub async fn get_cleanup_recording_audio(
    state: State<'_, CleanupState>,
    audio_id: String,
) -> Result<Audio, String> {
    safe_id(&audio_id)?;
    let dir = state.root.join("cleanup-recordings").join(audio_id);
    let name: String = serde_json::from_slice(
        &fs::read(dir.join("name.json")).map_err(|_| "Recording unavailable")?,
    )
    .map_err(|_| "Recording unavailable")?;
    if !valid_name(&name) {
        return Err("Invalid recording".into());
    }
    let bytes = read_audio(&dir.join("audio.wav"))?;
    let source_media = crate::recording_transfer::read_context(&dir, &bytes)?;
    Ok(Audio {
        name,
        bytes,
        source_media,
    })
}
#[derive(Serialize)]
pub struct Exported {
    name: String,
    message: String,
}
fn export_to(root: &Path, destination: &Path, result_id: &str) -> Result<Exported, String> {
    let result = read_result(root, result_id)?;
    let bytes = read_audio(
        &root
            .join("cleanup-results")
            .join(result_id)
            .join("cleaned.wav"),
    )?;
    fs::create_dir_all(destination).map_err(|_| {
        "Downloads could not be opened. Your cleaned recording is still saved in the app."
    })?;
    let stem = Path::new(&result.name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("recording");
    let stem = stem
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(60)
        .collect::<String>();
    let name = format!(
        "{}-cleaned-{}.wav",
        if stem.is_empty() { "recording" } else { &stem },
        id()
    );
    let path = destination.join(&name);
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "Could not create export; existing files are unchanged")?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(
            "Export could not finish. Your cleaned recording remains saved in the app.".into(),
        );
    }
    Ok(Exported {
        name,
        message: "Saved to Downloads/EthiopicVoiceStudio. The original is unchanged.".into(),
    })
}
#[tauri::command]
pub async fn export_cleanup_audio(
    app: tauri::AppHandle,
    state: State<'_, CleanupState>,
    id: String,
) -> Result<Exported, String> {
    let destination = app
        .path()
        .download_dir()
        .map_err(|_| "Downloads folder unavailable")?
        .join("EthiopicVoiceStudio");
    export_to(&state.root, &destination, &id)
}
fn read_audio(path: &Path) -> Result<Vec<u8>, String> {
    let f = fs::File::open(path).map_err(|_| "Audio unavailable")?;
    let mut bytes = vec![];
    f.take(MAX_AUDIO as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Audio unavailable")?;
    validate_audio(&bytes)?;
    Ok(bytes)
}
fn update(tasks: &Mutex<HashMap<String, Task>>, id: &str, apply: impl FnOnce(&mut Job)) {
    if let Ok(mut tasks) = tasks.lock() {
        if let Some(t) = tasks.get_mut(id) {
            apply(&mut t.job);
        }
    }
}
fn worker_stage(job: &mut Job, stage: &str) {
    let progress = stage
        .strip_prefix("Cleaning audio · ")
        .and_then(|value| value.strip_suffix('%'))
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| *value <= 100);
    job.stage = match stage {
        "loading" | "Loading cleanup model" | "Loading Voice Cleanup" => "Loading Voice Cleanup",
        "Preparing recording" => "Preparing recording",
        "Checking cleaned recording" => "Checking cleaned recording",
        "Saving cleaned recording" => "Saving cleaned recording",
        "Cleaning your recording" => "Cleaning your recording",
        "processing" | "Cleaning audio" => "Cleaning audio",
        "saving" | "Saving cleaned audio" => "Saving cleaned audio",
        value if progress.is_some() => value,
        _ => "Processing audio locally",
    }
    .to_string();
    if let Some(progress) = progress {
        job.progress = Some(job.progress.unwrap_or(0).max(progress));
        job.stage = format!("Cleaning audio · {}%", job.progress.unwrap());
    }
}
fn supervise(
    root: &Path,
    audio: &Path,
    job_id: &str,
    cancel: &AtomicBool,
    tasks: &Mutex<HashMap<String, Task>>,
) -> Result<String, String> {
    let (config, python, site) = installation(root)?;
    let input_hash = format!("{:x}", Sha256::digest(read_audio(audio)?));
    update(tasks, job_id, |j| {
        j.stage = "Checking installed voice files".into()
    });
    if config.files.is_empty() {
        return Err("Voice-file checksums are missing. Repair voice tools.".into());
    }
    for entry in &config.files {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        let mut file = fs::File::open(inside(&resources(root), &entry.path)?)
            .map_err(|_| "Voice files need repair")?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err("cancelled".into());
            }
            let size = file
                .read(&mut buffer)
                .map_err(|_| "Voice files could not be checked")?;
            if size == 0 {
                break;
            }
            digest.update(&buffer[..size]);
        }
        if format!("{:x}", digest.finalize()) != entry.sha256 {
            return Err("An installed voice file is damaged. Repair voice tools and retry.".into());
        }
    }
    let work = root.join("cleanup-jobs").join(job_id);
    fs::create_dir_all(&work).map_err(|_| "Check available storage and retry")?;
    let _cleanup = JobFiles(work.clone());
    let worker = work.join("worker.py");
    fs::write(
        &worker,
        include_str!("../../../../workers/cleanup/worker.py"),
    )
    .map_err(|_| "Could not prepare voice tools")?;
    fs::write(
        work.join("mamba_reference.py"),
        include_str!("../../../../workers/cleanup/mamba_reference.py"),
    )
    .map_err(|_| "Could not prepare cleanup adapter")?;
    let output = work.join("cleaned.wav");
    let metadata_output = work.join("metadata.json");
    let mut child=Command::new(python).arg("-I").arg("-B").arg("-u").arg("-c").arg("import runpy,sys,os; sys.path.insert(0,sys.argv.pop(1)); worker=sys.argv.pop(1); sys.path.insert(0,os.path.dirname(worker)); runpy.run_path(worker,run_name='__main__')")
        .arg(site).arg(&worker).arg("--bundle").arg(resources(root).join("bundle")).arg("--audio").arg(audio).arg("--output").arg(&output).arg("--metadata").arg(&metadata_output)
        .env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1").env("OMP_NUM_THREADS","8").env("ETHIOPIC_MANAGED_WORKER","1")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_|"The private voice runtime could not start. Repair voice tools and retry.")?;
    let stdout = child.stdout.take().ok_or("Voice runtime unavailable")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut reader = reader;
        loop {
            let mut line = Vec::new();
            let Ok(n) = reader.by_ref().take(4097).read_until(b'\n', &mut line) else {
                break;
            };
            if n == 0 || line.len() > 4096 {
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
                    "Cleanup took too long. Try a shorter recording."
                }
                .to_string(),
            );
            break None;
        }
        while let Ok(line) = receiver.try_recv() {
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&line) {
                if event["protocol"] == 1 {
                    if let Some(stage) = event["stage"].as_str() {
                        update(tasks, job_id, |job| worker_stage(job, stage));
                    }
                    if event["error"] == "out_of_memory" {
                        failure=Some("Not enough free memory. Close other applications and try a shorter recording.".into());
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
                    "Cleanup stopped unexpectedly. Your recording is safe; please retry.".into(),
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
        return Err("The voice tools could not finish cleanup. Your recording is safe. Retry, or repair voice tools if this repeats.".into());
    }
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&{
        let f = fs::File::open(&metadata_output).map_err(|_| "Cleanup output unavailable")?;
        let mut b = vec![];
        f.take(65537)
            .read_to_end(&mut b)
            .map_err(|_| "Cleanup output unavailable")?;
        if b.len() > 65536 {
            return Err("Cleanup metadata too large".into());
        }
        b
    })
    .map_err(|_| "Invalid cleanup metadata")?;
    if metadata["model_version"].as_str() != Some(&config.model_version) {
        return Err("Cleanup model version mismatch".into());
    }
    let original = read_audio(audio)?;
    if format!("{:x}", Sha256::digest(&original)) != input_hash {
        return Err("The original recording changed during cleanup; result discarded".into());
    }
    let cleaned = read_audio(&output)?;
    let original_spec = validate_audio(&original)?;
    if metadata["sample_rate"].as_u64() != Some(original_spec.sample_rate as u64)
        || metadata["channels"].as_u64() != Some(original_spec.channels as u64)
        || metadata["frames"].as_u64() != Some(original_spec.frames as u64)
    {
        return Err("Cleanup metadata does not match the recording".into());
    }
    if validate_audio(&cleaned)? != original_spec {
        return Err("Cleanup changed the audio dimensions; result discarded".into());
    }
    let parent = root.join("cleanup-results");
    fs::create_dir_all(&parent).map_err(|_| "Result storage unavailable")?;
    let result_id = id();
    let staging = parent.join(format!(".pending-{result_id}"));
    fs::create_dir(&staging).map_err(|_| "Result storage unavailable")?;
    let guard = JobFiles(staging.clone());
    let name: String = serde_json::from_slice(
        &fs::read(audio.parent().unwrap().join("name.json"))
            .map_err(|_| "Recording metadata unavailable")?,
    )
    .map_err(|_| "Recording metadata unavailable")?;
    let result = CleanupResult {
        id: result_id.clone(),
        name,
        model_version: config.model_version,
        sample_rate: original_spec.sample_rate,
        channels: original_spec.channels,
        frames: original_spec.frames,
        original_file: "original.wav".into(),
        cleaned_file: "cleaned.wav".into(),
        original_sha256: format!("{:x}", Sha256::digest(&original)),
    };
    durable(&staging.join("original.wav"), &original)?;
    durable(&staging.join("cleaned.wav"), &cleaned)?;
    crate::recording_transfer::inherit(
        audio.parent().ok_or("Recording folder unavailable")?,
        &staging,
        &original,
    )?;
    durable(
        &staging.join("result.json"),
        &serde_json::to_vec(&result).unwrap(),
    )?;
    sync_dir(&staging)?;
    if cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    fs::rename(&staging, parent.join(&result_id)).map_err(|_| "Could not commit cleanup result")?;
    sync_dir(&parent)?;
    drop(guard);
    Ok(result_id)
}
#[tauri::command]
pub fn start_cleanup(
    state: State<'_, CleanupState>,
    editing: State<'_, crate::voice_editing::EditingState>,
    audio_id: String,
) -> Result<Job, String> {
    safe_id(&audio_id)?;
    installation(&state.root)?;
    let dir = state.root.join("cleanup-recordings").join(&audio_id);
    let name: String = serde_json::from_slice(
        &fs::read(dir.join("name.json")).map_err(|_| "Import a recording first")?,
    )
    .map_err(|_| "Recording metadata unavailable")?;
    if name.contains(['/', '\\']) {
        return Err("Invalid recording".into());
    }
    if !valid_name(&name) {
        return Err("Invalid recording".into());
    }
    let audio = dir.join("audio.wav");
    let spec = validate_audio(&fs::read(&audio).map_err(|_| "Recording unavailable")?)?;
    if spec.frames < spec.sample_rate / 10 {
        return Err("Choose a recording at least 0.1 seconds long.".into());
    }
    fs::create_dir_all(&state.root).map_err(|_| "Cleanup storage unavailable")?;
    let process_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.root.join("recognition.lock"))
        .map_err(|_| "Cleanup queue unavailable")?;
    process_lock
        .try_lock_exclusive()
        .map_err(|_| "Cleanup is already running in another studio window.")?;
    editing.release_idle()?;
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    if system.total_memory() >= 4 * 1024 * 1024 * 1024
        && crate::memory::available(&system) < 2 * 1024 * 1024 * 1024
        && crate::asr_runtime::release_idle()?
    {
        system.refresh_memory();
    }
    if system.total_memory() < 4 * 1024 * 1024 * 1024
        || crate::memory::available(&system) < 2 * 1024 * 1024 * 1024
    {
        return Err("This model needs at least 4 GB RAM and 2 GB currently free. Close other applications and retry.".into());
    }
    let mut tasks = state
        .tasks
        .lock()
        .map_err(|_| "Cleanup queue unavailable")?;
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Another recording is being cleaned. Wait or cancel it first.".into());
    }
    let job = Job {
        id: id(),
        audio_id,
        name,
        state: "running".into(),
        stage: "Preparing local voice tools".into(),
        progress: None,
        error: None,
        result_id: None,
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
                job.stage = "Cleaned audio saved".into();
                job.result_id = Some(id);
                job.progress = Some(100);
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
                    "Cleanup could not finish"
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
pub fn get_active_cleanup_job(state: State<'_, CleanupState>) -> Result<Option<Job>, String> {
    let tasks = state.tasks.lock().map_err(|_| "Progress unavailable")?;
    Ok(tasks
        .values()
        .find(|t| t.job.state == "running")
        .map(|t| t.job.clone()))
}
#[tauri::command]
pub fn get_cleanup_job(state: State<'_, CleanupState>, job_id: String) -> Result<Job, String> {
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
pub fn cancel_cleanup(state: State<'_, CleanupState>, job_id: String) -> Result<(), String> {
    let tasks = state.tasks.lock().map_err(|_| "Progress unavailable")?;
    let task = tasks.get(&job_id).ok_or("Job unavailable")?;
    task.cancel.store(true, Ordering::SeqCst);
    Ok(())
}
pub fn cancel_all(app: &tauri::AppHandle) {
    let state = app.state::<CleanupState>();
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
pub(crate) fn forget_history(state: &CleanupState, recordings: &[String], results: &[String]) {
    if let Ok(mut tasks) = state.tasks.lock() {
        tasks.retain(|_, task| {
            !recordings.contains(&task.job.audio_id)
                && !task
                    .job
                    .result_id
                    .as_ref()
                    .is_some_and(|id| results.contains(id))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_deletion_forgets_matching_completed_jobs_only() {
        let state = CleanupState::new(std::env::temp_dir().join(id()));
        for (key, audio, result) in [
            ("first", "input-one", "result-one"),
            ("second", "input-two", "result-two"),
            ("keep", "input-three", "result-three"),
        ] {
            state.tasks.lock().unwrap().insert(
                key.into(),
                Task {
                    job: Job {
                        id: key.into(),
                        audio_id: audio.into(),
                        name: "Saved recording.wav".into(),
                        state: "completed".into(),
                        stage: "Saved".into(),
                        progress: Some(100),
                        error: None,
                        result_id: Some(result.into()),
                    },
                    cancel: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        forget_history(&state, &["input-one".into()], &["result-two".into()]);
        let tasks = state.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks.contains_key("keep"));
    }
    #[test]
    fn job_reports_source_and_bounded_progress_without_exposing_local_paths() {
        let mut job = Job {
            id: "job-1".into(),
            audio_id: "source-1".into(),
            name: "Podcast.wav".into(),
            state: "running".into(),
            stage: "Preparing local voice tools".into(),
            progress: None,
            error: None,
            result_id: None,
        };
        worker_stage(&mut job, "loading");
        assert_eq!(job.stage, "Loading Voice Cleanup");
        assert_eq!(job.progress, None);
        worker_stage(&mut job, "Cleaning audio · 37%");
        assert_eq!(job.progress, Some(37));
        worker_stage(&mut job, "Cleaning audio · 20%");
        assert_eq!(job.stage, "Cleaning audio · 37%");
        for invalid in [
            "Cleaning audio · 101%",
            "Cleaning audio · NaN%",
            "Cleaning audio · -1%",
            "sensitive worker output",
        ] {
            worker_stage(&mut job, invalid);
            assert_eq!(job.stage, "Processing audio locally");
            assert_eq!(job.progress, Some(37));
        }
        worker_stage(&mut job, "Cleaning audio · 100%");
        worker_stage(&mut job, "Saving cleaned recording");
        assert_eq!(job.progress, Some(100));
        let value = serde_json::to_value(job).unwrap();
        assert_eq!(value["audioId"], "source-1");
        assert_eq!(value["name"], "Podcast.wav");
        assert_eq!(value["progress"], 100);
        assert!(value.get("path").is_none());
    }
    fn wav(channels: u16, rate: u32, frames: u32) -> Vec<u8> {
        let mut output = Cursor::new(vec![]);
        {
            let mut writer = hound::WavWriter::new(
                &mut output,
                hound::WavSpec {
                    channels,
                    sample_rate: rate,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..frames * channels as u32 {
                writer.write_sample(123i16).unwrap();
            }
            writer.finalize().unwrap();
        }
        output.into_inner()
    }
    #[test]
    fn only_supports_complete_pcm_at_supported_rates_and_channels() {
        assert_eq!(
            validate_audio(&wav(2, 48000, 1234)).unwrap(),
            AudioSpec {
                channels: 2,
                sample_rate: 48000,
                frames: 1234
            }
        );
        assert!(validate_audio(&wav(3, 16000, 100)).is_err());
        assert!(validate_audio(&wav(1, 96000, 100)).is_err());
        assert!(validate_audio(&wav(1, 16000, 0)).is_err());
        let mut truncated = wav(1, 16000, 100);
        truncated.truncate(truncated.len() - 20);
        assert!(validate_audio(&truncated).is_err());
    }
    #[test]
    fn rejects_paths_and_control_characters() {
        assert!(safe_id("../escaped").is_err());
        assert!(safe_id("recording-123").is_ok());
        for name in [
            "../audio.wav",
            "a\\b.wav",
            "bad\naudio.wav",
            "",
            "audio.mp3",
        ] {
            assert!(!valid_name(name));
        }
        assert!(valid_name("አማርኛ.wav"));
    }
    #[test]
    fn durable_write_does_not_overwrite_original() {
        let root = std::env::temp_dir().join(format!("cleanup-test-{}", id()));
        fs::create_dir(&root).unwrap();
        let path = root.join("original.wav");
        let original = wav(1, 16000, 100);
        durable(&path, &original).unwrap();
        assert!(durable(&path, b"replacement").is_err());
        assert_eq!(fs::read(path).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn resource_paths_cannot_escape_through_parent_or_symlink() {
        let root = std::env::temp_dir().join(format!("cleanup-test-{}", id()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("good"), b"ok").unwrap();
        assert!(inside(&root, "good").is_ok());
        assert!(inside(&root, "../escape").is_err());
        assert!(inside(&root, "/tmp/escape").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(std::env::temp_dir(), root.join("link")).unwrap();
            assert!(inside(&root, "link").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn result_metadata_cannot_redirect_audio_reads() {
        let root = std::env::temp_dir().join(format!("cleanup-test-{}", id()));
        let dir = root.join("cleanup-results/test-1");
        fs::create_dir_all(&dir).unwrap();
        let result = CleanupResult {
            id: "test-1".into(),
            name: "original.wav".into(),
            model_version: "v1".into(),
            sample_rate: 16000,
            channels: 1,
            frames: 100,
            original_file: "../../secret.wav".into(),
            cleaned_file: "cleaned.wav".into(),
            original_sha256: "abc".into(),
        };
        fs::write(
            dir.join("result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
        assert!(read_result(&root, "test-1").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    #[ignore = "requires locally installed validated cleanup resources"]
    fn cancels_running_worker_during_model_loading() {
        let root = PathBuf::from(std::env::var("ETHIOPIC_CLEANUP_TEST_ROOT").unwrap());
        let audio = PathBuf::from(std::env::var("ETHIOPIC_CLEANUP_TEST_WAV").unwrap());
        let job_id = id();
        let cancel = Arc::new(AtomicBool::new(false));
        let tasks = Arc::new(Mutex::new(HashMap::from([(
            job_id.clone(),
            Task {
                job: Job {
                    id: job_id.clone(),
                    audio_id: "fixture".into(),
                    name: "Fixture.wav".into(),
                    state: "running".into(),
                    stage: "queued".into(),
                    progress: None,
                    error: None,
                    result_id: None,
                },
                cancel: cancel.clone(),
            },
        )])));
        let observer = tasks.clone();
        let flag = cancel.clone();
        let watched_id = job_id.clone();
        let monitor = std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(90) {
                let loading =
                    observer.lock().unwrap()[&watched_id].job.stage == "Loading Voice Cleanup";
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
        assert!(!root.join("cleanup-jobs").join(job_id).exists());
        assert!(audio.exists());
    }

    #[test]
    #[ignore = "requires locally installed validated cleanup resources"]
    fn real_worker_preserves_original_and_audio_dimensions() {
        let root =
            PathBuf::from(std::env::var("ETHIOPIC_CLEANUP_TEST_ROOT").expect("resource root"));
        let source =
            PathBuf::from(std::env::var("ETHIOPIC_CLEANUP_TEST_WAV").expect("validation WAV"));
        let original = read_audio(&source).unwrap();
        let recording = root.join("cleanup-recordings").join(id());
        fs::create_dir_all(&recording).unwrap();
        durable(&recording.join("audio.wav"), &original).unwrap();
        durable(&recording.join("name.json"), br#""validation.wav""#).unwrap();
        let result_id = supervise(
            &root,
            &recording.join("audio.wav"),
            &id(),
            &AtomicBool::new(false),
            &Mutex::new(HashMap::new()),
        )
        .unwrap();
        let metadata = read_result(&root, &result_id).unwrap();
        let dir = root.join("cleanup-results").join(&result_id);
        assert_eq!(read_audio(&dir.join("original.wav")).unwrap(), original);
        assert_eq!(
            validate_audio(&read_audio(&dir.join("cleaned.wav")).unwrap()).unwrap(),
            validate_audio(&original).unwrap()
        );
        assert_eq!(
            metadata.original_sha256,
            format!("{:x}", Sha256::digest(&original))
        );
        assert_eq!(
            metadata.model_version,
            installation(&root).unwrap().0.model_version
        );
    }
}
