//! Shared resident ASR process, kept loaded by default. One serialized job, private pipes, no listening ports.
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;

static POOL: Mutex<Option<Worker>> = Mutex::new(None);
static PREFERENCES: Mutex<()> = Mutex::new(());
const ERROR: &str = "The local voice engine stopped. Your recording is safe; please retry.";

pub fn enabled(root: &Path) -> bool {
    fs::read(root.join("keep-asr-ready.json"))
        .ok()
        .and_then(|data| serde_json::from_slice::<bool>(&data).ok())
        .unwrap_or(true)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    keep_ready: bool,
    state: &'static str,
}
fn status(root: &Path) -> RuntimeStatus {
    let state = match POOL.try_lock() {
        Ok(mut pool) => {
            if pool
                .as_mut()
                .is_some_and(|w| w.child.try_wait().ok().flatten().is_some())
            {
                *pool = None;
            }
            if pool.is_some() {
                "waiting"
            } else {
                "unloaded"
            }
        }
        Err(_) => "running",
    };
    RuntimeStatus {
        keep_ready: enabled(root),
        state,
    }
}
#[tauri::command]
pub fn get_asr_runtime_status(app: tauri::AppHandle) -> Result<RuntimeStatus, String> {
    Ok(status(&app.path().app_data_dir().map_err(|_| ERROR)?))
}
#[tauri::command]
pub fn set_asr_keep_ready(
    app: tauri::AppHandle,
    keep_ready: bool,
) -> Result<RuntimeStatus, String> {
    let root = app.path().app_data_dir().map_err(|_| ERROR)?;
    let _prefs = PREFERENCES.lock().map_err(|_| ERROR)?;
    fs::create_dir_all(&root).map_err(|_| "Could not save voice preferences")?;
    save_preference(&root, keep_ready)?;
    if !keep_ready {
        if let Ok(mut pool) = POOL.try_lock() {
            *pool = None;
        }
    }
    Ok(status(&root))
}
/// Release only an idle model. A recording owns the pool lock for its entire job.
/// This leaves retention enabled so the next job can load once and stay ready again.
pub(crate) fn release_idle() -> Result<bool, String> {
    let mut pool = POOL
        .try_lock()
        .map_err(|_| "Finish or stop transcription or dictation before unloading its model.")?;
    Ok(pool.take().is_some())
}
fn unload(root: &Path) -> Result<RuntimeStatus, String> {
    release_idle()?;
    Ok(status(root))
}
#[tauri::command]
pub async fn unload_asr_model(app: tauri::AppHandle) -> Result<RuntimeStatus, String> {
    let root = app.path().app_data_dir().map_err(|_| ERROR)?;
    tauri::async_runtime::spawn_blocking(move || unload(&root))
        .await
        .map_err(|_| "Unloading voice tools was interrupted. Please retry.")?
}
fn save_preference(root: &Path, keep_ready: bool) -> Result<(), String> {
    let pending = root.join(format!("keep-asr-ready-{}.pending", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&pending)?;
        file.write_all(if keep_ready { b"true" } else { b"false" })?;
        file.sync_all()?;
        fs::rename(&pending, root.join("keep-asr-ready.json"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result.map_err(|_| "Could not save voice preferences".into())
}
pub fn shutdown() {
    if let Ok(mut pool) = POOL.try_lock() {
        *pool = None;
    }
}

/// Setup calls this from its background thread after taking the recognition lock.
/// Unlike best-effort app exit, replacement must release an idle model before its self-test.
pub fn release_for_setup() -> Result<(), String> {
    let mut pool = POOL
        .lock()
        .map_err(|_| "Could not release voice tools. Close the studio and try setup again.")?;
    *pool = None;
    Ok(())
}

pub struct Worker {
    child: Child,
    input: ChildStdin,
    events: mpsc::Receiver<Result<Value, ()>>,
    reader: Option<std::thread::JoinHandle<()>>,
    identity: Vec<u8>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Unbounded channel prevents the private reader from blocking during shutdown.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
impl Worker {
    pub fn send(&mut self, value: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.input, value).map_err(|_| ERROR)?;
        self.input
            .write_all(b"\n")
            .and_then(|_| self.input.flush())
            .map_err(|_| ERROR.into())
    }
    fn start(
        root: &Path,
        python: &Path,
        site: &Path,
        bundle: &Path,
        identity: Vec<u8>,
    ) -> Result<Self, String> {
        let scripts = root.join("asr-runtime");
        fs::create_dir_all(&scripts).map_err(|_| ERROR)?;
        crate::noise_reduction::stage_adapter(&scripts)?;
        crate::recognition_bundle::stage_workers(&scripts)?;
        for (name, source) in [
            (
                "resident_worker.py",
                include_str!("../../../../workers/asr/resident_worker.py"),
            ),
            (
                "worker.py",
                include_str!("../../../../workers/asr/worker.py"),
            ),
            (
                "dictation_worker.py",
                include_str!("../../../../workers/asr/dictation_worker.py"),
            ),
        ] {
            fs::write(scripts.join(name), source).map_err(|_| ERROR)?;
        }
        let mut child = crate::background_process::command(python).args(["-I","-B","-u","-X","utf8","-c","import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')"]).arg(site).arg(&scripts).arg(scripts.join("resident_worker.py")).arg("--bundle").arg(bundle)
            .env("ORT_DISABLE_TELEMETRY", "1").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1").env("OMP_NUM_THREADS","8")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| ERROR)?;
        let input = child.stdin.take().ok_or(ERROR)?;
        let output = child.stdout.take().ok_or(ERROR)?;
        let (send, events) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                match output.by_ref().take(524289).read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(n) if n <= 524288 => {
                        let parsed = serde_json::from_slice::<Value>(&line).map_err(|_| ());
                        let invalid = parsed.is_err();
                        if send.send(parsed).is_err() || invalid {
                            break;
                        }
                    }
                    _ => {
                        let _ = send.send(Err(()));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            input,
            events,
            reader: Some(reader),
            identity,
        })
    }
}
/// Keep process only after a complete idle handshake. All failures/cancellation discard it.
pub fn execute(
    root: &Path,
    runtime: (&Path, &Path, &Path),
    cancel: &AtomicBool,
    request: Value,
    mut tick: impl FnMut(&mut Worker) -> Result<(), String>,
    mut event: impl FnMut(Value) -> Result<(), String>,
) -> Result<(), String> {
    let (python, site, bundle) = runtime;
    let mut pool = POOL
        .try_lock()
        .map_err(|_| "Voice tools are busy. Please wait for the current recording.")?;
    let mut identity = fs::read(root.join("asr/active/installation.json")).map_err(|_| ERROR)?;
    if let Ok(addon) = fs::read(root.join("noise-reduction/active/installation.json")) {
        identity.extend_from_slice(&addon);
    }
    for path in [python, site, bundle] {
        identity.extend_from_slice(path.as_os_str().to_string_lossy().as_bytes());
    }
    if pool.as_ref().is_some_and(|w| w.identity != identity) {
        *pool = None;
    }
    if pool
        .as_mut()
        .is_some_and(|w| !matches!(w.child.try_wait(), Ok(None)))
    {
        *pool = None;
    }
    let mut worker = match pool.take() {
        Some(w) => w,
        None => Worker::start(root, python, site, bundle, identity)?,
    };
    worker.send(&request)?;
    let started = Instant::now();
    let mut finished = false;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        if started.elapsed() > Duration::from_secs(1800) {
            return Err("Recognition took too long. Your recording is safe; please retry.".into());
        }
        tick(&mut worker)?;
        match worker.events.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(value)) => {
                if value["protocol"] != 1 {
                    return Err(ERROR.into());
                }
                if let Some(code) = value["error"].as_str() {
                    if let Some(message) = crate::recognition_bundle::provider_error(code) {
                        return Err(message.into());
                    }
                    return Err(if code == "out_of_memory" { "Not enough free memory. Turn off Keep voice tools ready, close other applications and retry." } else { ERROR }.into());
                }
                if value["event"] == "idle" {
                    if !finished {
                        return Err(ERROR.into());
                    }
                    let _prefs = PREFERENCES.lock().map_err(|_| ERROR)?;
                    if enabled(root) {
                        *pool = Some(worker);
                    }
                    return Ok(());
                }
                if value["event"] == "finished" {
                    finished = true;
                }
                event(value)?;
            }
            Ok(Err(())) | Err(mpsc::RecvTimeoutError::Disconnected) => return Err(ERROR.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if worker.child.try_wait().map_err(|_| ERROR)?.is_some() {
                    return Err(ERROR.into());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn setup_waits_for_status_reader_and_releases_the_idle_process() {
        let mut child = crate::background_process::command("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        *POOL.lock().unwrap() = Some(Worker {
            child,
            input,
            events: mpsc::channel().1,
            reader: None,
            identity: vec![],
        });
        let (held, ready) = mpsc::channel();
        let (release, waiting) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let _guard = POOL.lock().unwrap();
            held.send(()).unwrap();
            waiting.recv().unwrap();
        });
        ready.recv().unwrap();
        assert!(release_idle().unwrap_err().contains("Finish or stop"));
        let (done, completed) = mpsc::channel();
        let setup = std::thread::spawn(move || {
            done.send(release_for_setup()).unwrap();
        });
        // A brief status read must delay setup rather than silently skip process shutdown.
        assert!(completed.recv_timeout(Duration::from_millis(30)).is_err());
        release.send(()).unwrap();
        reader.join().unwrap();
        completed
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        setup.join().unwrap();
        assert!(POOL.lock().unwrap().is_none());
    }
    #[test]
    fn preference_keeps_models_ready_by_default_and_preserves_explicit_opt_out() {
        let root = std::env::temp_dir().join(format!("asr-preference-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert!(enabled(&root));
        for (value, expected) in [
            ("true", true),
            ("false", false),
            ("{}", true),
            ("broken", true),
        ] {
            fs::write(root.join("keep-asr-ready.json"), value).unwrap();
            assert_eq!(enabled(&root), expected);
        }
        save_preference(&root, true).unwrap();
        assert!(enabled(&root));
        save_preference(&root, false).unwrap();
        assert!(!enabled(&root));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires installed ASR runtime; exercises real resident model with synthetic silence"]
    fn real_model_reuses_process_across_batch_and_live_sessions() {
        let installed = std::path::PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_ROOT").unwrap());
        let root = std::env::temp_dir().join(format!("resident-real-{}", std::process::id()));
        fs::create_dir_all(root.join("asr/active")).unwrap();
        fs::copy(
            installed.join("asr/active/installation.json"),
            root.join("asr/active/installation.json"),
        )
        .unwrap();
        assert!(enabled(&root));
        let cancel = AtomicBool::new(false);
        let (version, python, site, bundle) =
            crate::asr::verified_dictation_runtime(&installed, &cancel).unwrap();
        let selection = crate::recognition_bundle::read(&bundle).unwrap();
        let audio = root.join("silence.wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&32036u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&32000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&32000u32.to_le_bytes());
        bytes.resize(32044, 0);
        fs::write(&audio, bytes).unwrap();
        let mut pid = None;
        let mut loads = 0;
        for mode in ["transcribe", "transcribe", "dictation", "dictation"] {
            let output = root.join("result.json");
            let mut sent = false;
            execute(
                &root,
                (&python, &site, &bundle),
                &cancel,
                if mode == "transcribe" {
                    serde_json::json!({"type":mode,"audio":audio,"output":output})
                } else {
                    serde_json::json!({"type":mode,"output":output})
                },
                |worker| {
                    if mode == "dictation" && !sent {
                        worker.send(
                            &serde_json::json!({"type":"audio","samples":vec![0.0f32;16000]}),
                        )?;
                        worker.send(&serde_json::json!({"type":"finish"}))?;
                        sent = true;
                    }
                    Ok(())
                },
                |event| {
                    if event["stage"] == "Loading Amharic voice tools" {
                        loads += 1;
                    }
                    Ok(())
                },
            )
            .unwrap();
            let result: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
            if let Some(selection) = &selection {
                let provider = if mode == "dictation" {
                    &selection.dictation
                } else {
                    &selection.transcription
                };
                assert_eq!(result["model_version"], provider.model_version);
                assert_eq!(result["model"], provider.model);
            } else {
                assert_eq!(result["model_version"], version);
            }
            assert_eq!(status(&root).state, "waiting");
            let current = POOL.lock().unwrap().as_ref().unwrap().child.id();
            assert_eq!(*pid.get_or_insert(current), current);
        }
        let switches_model = selection.as_ref().is_some_and(|b| {
            let key = |p: &crate::recognition_bundle::Provider| {
                (
                    p.variant.clone(),
                    p.backend.clone(),
                    p.device.clone(),
                    p.dtype.clone(),
                    p.threads,
                )
            };
            key(&b.transcription) != key(&b.dictation)
        });
        assert_eq!(
            loads,
            if switches_model { 2 } else { 1 },
            "Repeated jobs retain a model; changing the selected model replaces it once"
        );
        let before_unload = pid.unwrap();
        let released = unload(&root).unwrap();
        assert!(released.keep_ready);
        assert_eq!(released.state, "unloaded");
        execute(&root, (&python, &site, &bundle), &cancel,
            serde_json::json!({"type":"transcribe","audio":audio,"output":root.join("reload.json")}),
            |_| Ok(()), |_| Ok(())).unwrap();
        assert_eq!(status(&root).state, "waiting");
        assert_ne!(
            POOL.lock().unwrap().as_ref().unwrap().child.id(),
            before_unload
        );
        cancel.store(true, Ordering::SeqCst);
        let result = execute(
            &root,
            (&python, &site, &bundle),
            &cancel,
            serde_json::json!({"type":"transcribe","audio":audio,"output":root.join("cancelled.json")}),
            |_| Ok(()),
            |_| Ok(()),
        );
        assert_eq!(result.unwrap_err(), "cancelled");
        assert_eq!(status(&root).state, "unloaded");
        cancel.store(false, Ordering::SeqCst);
        let mut changed = false;
        execute(&root, (&python, &site, &bundle), &cancel,
            serde_json::json!({"type":"transcribe","audio":audio,"output":root.join("release.json")}),
            |_| {
                if !changed { fs::write(root.join("keep-asr-ready.json"),b"false").unwrap(); changed=true; }
                Ok(())
            }, |_| Ok(())).unwrap();
        assert_eq!(
            status(&root).state,
            "unloaded",
            "turning off during a job releases the worker after completion"
        );
        shutdown();
        fs::remove_dir_all(root).unwrap();
    }
}
