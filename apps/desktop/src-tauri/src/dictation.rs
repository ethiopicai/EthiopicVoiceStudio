//! One supervised, local streaming ASR session. Audio is retained independently of inference.
use fs2::FileExt;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};
use transcripts::{ModelTranscript, ModelWord, TranscriptStore};
const RATE: u32 = 16000;
#[cfg(test)]
const MAX_CHUNK: usize = 32000;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn id() -> String {
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub state: String,
    pub stage: String,
    pub final_words: Vec<ModelWord>,
    pub partial_words: Vec<ModelWord>,
    pub error: Option<String>,
    pub transcript_id: Option<String>,
    pub recording_id: Option<String>,
    pub model_version: Option<String>,
    pub decode_seconds: Option<f64>,
    pub input_sample_rate: u32,
}
struct Input {
    writer: Option<hound::WavWriter<std::io::BufWriter<fs::File>>>,
    sender: SyncSender<Vec<f32>>,
    samples: u64,
    accepting: bool,
}
struct Session {
    noise: Option<serde_json::Value>,
    view: Mutex<SessionView>,
    input: Mutex<Input>,
    cancel: AtomicBool,
    finish: AtomicBool,
    overflow: AtomicBool,
}
pub struct DictationState {
    root: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,
}
impl DictationState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
fn active(state: &str) -> bool {
    matches!(state, "loading" | "ready" | "recording" | "finishing")
}
fn snapshot(session: &Session) -> Result<SessionView, String> {
    session
        .view
        .lock()
        .map(|v| v.clone())
        .map_err(|_| "Dictation session unavailable".into())
}
fn session(state: &DictationState, id: &str) -> Result<Arc<Session>, String> {
    state
        .sessions
        .lock()
        .map_err(|_| "Dictation session unavailable")?
        .get(id)
        .cloned()
        .ok_or("Dictation session unavailable".into())
}
pub fn shortcut_snapshot(state: &DictationState, id: &str) -> Result<SessionView, String> {
    snapshot(session(state, id)?.as_ref())
}
fn recording_name(id: &str) -> String {
    format!("Dictation-{id}.wav")
}
fn process_lock(root: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(root).map_err(|_| "Check free storage")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("recognition.lock"))
        .map_err(|_| "Voice tools unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "Another voice task is running. Finish or cancel it before dictation.")?;
    Ok(lock)
}
#[cfg(test)]
fn new_session(root: &Path) -> Result<(Arc<Session>, mpsc::Receiver<Vec<f32>>), String> {
    new_session_with_noise(root, None)
}
fn new_session_with_noise(
    root: &Path,
    noise: Option<serde_json::Value>,
) -> Result<(Arc<Session>, mpsc::Receiver<Vec<f32>>), String> {
    let rate = if noise.is_some() { 48000 } else { RATE };
    let id = id();
    let dir = root.join("recordings").join(&id);
    fs::create_dir_all(&dir).map_err(|_| "Check free storage")?;
    let writer = hound::WavWriter::create(
        dir.join(recording_name(&id)),
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|_| "Could not save microphone audio. Check storage.")?;
    fs::write(
        dir.join("name.json"),
        serde_json::to_vec(&recording_name(&id)).map_err(|_| "Recording metadata unavailable")?,
    )
    .map_err(|_| "Recording metadata unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(24);
    Ok((
        Arc::new(Session {
            noise,
            view: Mutex::new(SessionView {
                id,
                state: "loading".into(),
                stage: "Preparing local dictation".into(),
                final_words: vec![],
                partial_words: vec![],
                error: None,
                transcript_id: None,
                recording_id: None,
                model_version: None,
                decode_seconds: None,
                input_sample_rate: rate,
            }),
            input: Mutex::new(Input {
                writer: Some(writer),
                sender,
                samples: 0,
                accepting: true,
            }),
            cancel: AtomicBool::new(false),
            finish: AtomicBool::new(false),
            overflow: AtomicBool::new(false),
        }),
        receiver,
    ))
}
#[tauri::command]
pub async fn start_dictation(
    state: State<'_, DictationState>,
    editing: State<'_, crate::voice_editing::EditingState>,
) -> Result<SessionView, String> {
    begin_session(&state, &editing)
}
fn begin_session(
    state: &DictationState,
    editing: &crate::voice_editing::EditingState,
) -> Result<SessionView, String> {
    let mut sessions = state
        .sessions
        .lock()
        .map_err(|_| "Dictation session unavailable")?;
    if sessions
        .values()
        .any(|s| snapshot(s).is_ok_and(|v| active(&v.state)))
    {
        return Err("Dictation is already running".into());
    }
    sessions.clear();
    let lock = process_lock(&state.root)?;
    editing.release_idle()?;
    let noise = crate::noise_reduction::configuration(&state.root, &AtomicBool::new(false), true)?;
    let (session, receiver) = new_session_with_noise(&state.root, noise)?;
    let initial = snapshot(&session)?;
    sessions.insert(initial.id.clone(), session.clone());
    let root = state.root.clone();
    std::thread::spawn(move || {
        let _lock = lock;
        run_session(&root, &session, receiver);
    });
    Ok(initial)
}
fn feed(session: &Session, samples: Vec<f32>) -> Result<(), String> {
    let rate = snapshot(session)?.input_sample_rate;
    if samples.is_empty()
        || samples.len() > rate as usize * 2
        || samples.iter().any(|v| !v.is_finite() || v.abs() > 1.0)
    {
        return Err("Microphone audio must be finite mono samples at the session's sample rate between -1 and 1.".into());
    }
    if !snapshot(session).is_ok_and(|v| matches!(v.state.as_str(), "ready" | "recording")) {
        return Err("Wait for dictation to be ready before recording.".into());
    }
    let mut input = session
        .input
        .lock()
        .map_err(|_| "Microphone recording unavailable")?;
    if !input.accepting
        || session.cancel.load(Ordering::SeqCst)
        || session.finish.load(Ordering::SeqCst)
    {
        return Err("Dictation is stopping".into());
    }
    if input.samples + samples.len() as u64 > rate as u64 * 1800 {
        session.overflow.store(true, Ordering::SeqCst);
        session.cancel.store(true, Ordering::SeqCst);
        input.accepting = false;
        return Err(
            "Dictation reached its 30-minute recording limit. Captured audio is being saved."
                .into(),
        );
    }
    let write_result = (|| {
        let writer = input.writer.as_mut().ok_or(())?;
        for value in &samples {
            writer
                .write_sample(((*value * 32768.0).round().clamp(-32768.0, 32767.0)) as i16)
                .map_err(|_| ())?;
        }
        writer.flush().map_err(|_| ())
    })();
    if write_result.is_err() {
        session.overflow.store(true, Ordering::SeqCst);
        session.cancel.store(true, Ordering::SeqCst);
        input.accepting = false;
        return Err("Audio could not be saved. Dictation stopped; check free storage.".into());
    }
    input.samples += samples.len() as u64;
    match input.sender.try_send(samples) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
            session.overflow.store(true, Ordering::SeqCst);
            session.cancel.store(true, Ordering::SeqCst);
            input.accepting = false;
            return Err(
                "Dictation could not keep up. Captured audio is saved for transcription.".into(),
            );
        }
    }
    if let Ok(mut view) = session.view.lock() {
        if view.state == "ready" {
            view.state = "recording".into();
            view.stage = "Listening locally".into();
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn feed_dictation(
    state: State<'_, DictationState>,
    session_id: String,
    samples: Vec<f32>,
) -> Result<(), String> {
    feed(session(&state, &session_id)?.as_ref(), samples)
}
#[tauri::command]
pub async fn stop_dictation(
    state: State<'_, DictationState>,
    session_id: String,
) -> Result<(), String> {
    let session = session(&state, &session_id)?;
    let mut input = session.input.lock().map_err(|_| "Dictation unavailable")?;
    input.accepting = false;
    session.finish.store(true, Ordering::SeqCst);
    if let Ok(mut view) = session.view.lock() {
        if active(&view.state) {
            view.state = "finishing".into();
            view.stage = "Finishing your transcript".into();
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn cancel_dictation(
    state: State<'_, DictationState>,
    session_id: String,
) -> Result<(), String> {
    let session = session(&state, &session_id)?;
    session.cancel.store(true, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
pub async fn get_dictation_session(
    state: State<'_, DictationState>,
    session_id: String,
) -> Result<SessionView, String> {
    snapshot(session(&state, &session_id)?.as_ref())
}
#[tauri::command]
pub async fn get_active_dictation_session(
    state: State<'_, DictationState>,
) -> Result<Option<SessionView>, String> {
    let sessions = state.sessions.lock().map_err(|_| "Dictation unavailable")?;
    Ok(sessions
        .values()
        .filter_map(|s| snapshot(s).ok())
        .find(|v| active(&v.state)))
}
fn words(value: &serde_json::Value) -> Result<Vec<ModelWord>, String> {
    let words: Vec<ModelWord> = serde_json::from_value(value.clone())
        .map_err(|_| "Dictation returned invalid word timings")?;
    if words.len() > 100000 {
        return Err("Dictation result too large".into());
    }
    let mut last = 0.0;
    for w in &words {
        if w.word.trim().is_empty()
            || w.word.len() > 4096
            || !w.start.is_finite()
            || !w.end.is_finite()
            || w.start < last
            || w.end < w.start
        {
            return Err("Dictation returned invalid word timings".into());
        }
        last = w.end;
    }
    Ok(words)
}
fn apply_event(session: &Session, event: serde_json::Value, version: &str) -> Result<bool, String> {
    if event["protocol"] != 1 {
        return Err("Dictation protocol is incompatible. Repair voice tools.".into());
    }
    if let Some(error) = event["error"].as_str() {
        if let Some(message) = crate::recognition_bundle::provider_error(error) {
            return Err(message.into());
        }
        return Err(match error {"out_of_memory"=>"Dictation ran out of memory. Close other applications and retry.","backlog_exceeded"=>"Dictation could not keep up on this computer. Your audio is saved for transcription.",_=>"The local dictation engine stopped. Your audio is saved for retry."}.into());
    }
    let mut view = session.view.lock().map_err(|_| "Dictation unavailable")?;
    match event["event"].as_str() {
        Some("ready") => {
            if event["model_version"].as_str() != Some(version) {
                return Err("Dictation model version mismatch".into());
            }
            if view.state == "loading" {
                view.state = "ready".into();
                view.stage = "Ready for microphone".into();
            }
            view.model_version = Some(version.into());
            view.decode_seconds = event["decode_seconds"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0);
        }
        Some("transcript") => {
            let finals = words(&event["final"])?;
            let partial = words(&event["partial"])?;
            let last = view.final_words.last().map_or(0.0, |w| w.end);
            if finals.first().is_some_and(|w| w.start < last)
                || view.final_words.len() + finals.len() > 100000
            {
                return Err("Dictation changed an already finalized word".into());
            }
            let boundary = finals.last().map_or(last, |w| w.end);
            if partial.first().is_some_and(|w| w.start < boundary) {
                return Err("Dictation returned overlapping word timings".into());
            }
            view.final_words.extend(finals);
            view.partial_words = partial;
        }
        Some("finished") => return Ok(true),
        _ => {
            if event["stage"].is_string() && view.state == "loading" {
                view.stage = "Loading Amharic dictation tools".into();
            }
        }
    }
    Ok(false)
}
struct WorkFiles(PathBuf);
impl Drop for WorkFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn supervise(
    root: &Path,
    session: &Arc<Session>,
    receiver: mpsc::Receiver<Vec<f32>>,
) -> Result<ModelTranscript, String> {
    let (version, python, site, bundle) =
        crate::asr::verified_dictation_runtime(root, &session.cancel)?;
    if session.cancel.load(Ordering::SeqCst) {
        return Err("cancelled".into());
    }
    let id = snapshot(session)?.id;
    let work = root.join("dictation-jobs").join(&id);
    fs::create_dir_all(&work).map_err(|_| "Check free storage")?;
    let _files = WorkFiles(work.clone());
    let script = work.join("worker.py");
    fs::write(
        &script,
        include_str!("../../../../workers/asr/dictation_worker.py"),
    )
    .map_err(|_| "Could not prepare dictation tools")?;
    crate::recognition_bundle::stage_workers(&work)?;
    let output = work.join("result.json");
    crate::noise_reduction::stage_adapter(&work)?;
    if crate::asr_runtime::enabled(root) {
        let mut sent_finish = false;
        crate::asr_runtime::execute(
            root,
            (&python, &site, &bundle),
            &session.cancel,
            serde_json::json!({"type":"dictation","output":output,"sample_rate":snapshot(session)?.input_sample_rate,"noise_reduction":session.noise}),
            |worker| {
                if sent_finish {
                    return Ok(());
                }
                // Feed bounded microphone messages off the audio thread. The Python
                // reader coalesces all waiting samples before inference.
                loop {
                    match receiver.try_recv() {
                        Ok(samples) => {
                            worker.send(&serde_json::json!({"type":"audio","samples":samples}))?
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return Err("Microphone transport stopped".into())
                        }
                    }
                }
                if session.finish.load(Ordering::SeqCst) {
                    worker.send(&serde_json::json!({"type":"finish"}))?;
                    sent_finish = true;
                }
                Ok(())
            },
            |event| apply_event(session, event, &version).map(|_| ()),
        )?;
    } else {
        let mut command = crate::background_process::command(python);
        command.args(["-I","-B","-u","-X","utf8","-c","import runpy,sys,pathlib;sys.path.insert(0,sys.argv.pop(1));path=sys.argv.pop(1);sys.path.insert(0,str(pathlib.Path(path).parent));runpy.run_path(path,run_name='__main__')"]).arg(site).arg(script).arg("--bundle").arg(bundle).arg("--output").arg(&output).arg("--sample-rate").arg(snapshot(session)?.input_sample_rate.to_string()).env("ORT_DISABLE_TELEMETRY", "1").env("HF_HUB_OFFLINE","1").env("TRANSFORMERS_OFFLINE","1").env("HF_HUB_DISABLE_TELEMETRY","1").env("ETHIOPIC_MANAGED_WORKER","1");
        if let Some(config) = &session.noise {
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
            .map_err(|_| "The private dictation runtime could not start. Repair voice tools.")?;
        let mut stdin = child.stdin.take().ok_or("Dictation runtime unavailable")?;
        let stdout = child.stdout.take().ok_or("Dictation runtime unavailable")?;
        let writer_session = session.clone();
        let writer_done = Arc::new(AtomicBool::new(false));
        let writer_closed = writer_done.clone();
        let writer = std::thread::spawn(move || loop {
            if writer_session.cancel.load(Ordering::SeqCst) || writer_closed.load(Ordering::SeqCst)
            {
                return;
            }
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(samples) => {
                    if serde_json::to_writer(
                        &mut stdin,
                        &serde_json::json!({"type":"audio","samples":samples}),
                    )
                    .is_err()
                        || stdin.write_all(b"\n").and_then(|_| stdin.flush()).is_err()
                    {
                        writer_session.overflow.store(true, Ordering::SeqCst);
                        writer_session.cancel.store(true, Ordering::SeqCst);
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if writer_session.finish.load(Ordering::SeqCst) {
                        let _ = stdin.write_all(b"{\"type\":\"finish\"}\n");
                        let _ = stdin.flush();
                        while !writer_closed.load(Ordering::SeqCst)
                            && !writer_session.cancel.load(Ordering::SeqCst)
                        {
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        });
        let (send, events) = mpsc::sync_channel(8);
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = vec![];
                let read = reader.by_ref().take(524289).read_until(b'\n', &mut line);
                match read {
                    Ok(0) => break,
                    Ok(n) if n <= 524288 => {
                        if send.send(Ok(line)).is_err() {
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
        let started = Instant::now();
        let mut finished = false;
        let result: Result<(), String> = loop {
            if session.cancel.load(Ordering::SeqCst) {
                break Err("cancelled".into());
            }
            if started.elapsed() > Duration::from_secs(1800) {
                break Err(
                    "Dictation reached its time limit. Your audio is saved for transcription."
                        .into(),
                );
            }
            while let Ok(event) = events.try_recv() {
                match event {
                    Ok(line) => {
                        let value = serde_json::from_slice(&line)
                            .map_err(|_| "Dictation returned an invalid response".to_string());
                        match value.and_then(|v| apply_event(session, v, &version)) {
                            Ok(done) => finished |= done,
                            Err(error) => {
                                session.cancel.store(true, Ordering::SeqCst);
                                if let Ok(mut v) = session.view.lock() {
                                    v.error = Some(error);
                                }
                                break;
                            }
                        }
                    }
                    Err(()) => {
                        session.cancel.store(true, Ordering::SeqCst);
                        if let Ok(mut v) = session.view.lock() {
                            v.error = Some("Dictation response exceeded its safe limit".into());
                        }
                        break;
                    }
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    // The stdout reader may still be delivering the final flushed event
                    // after the OS reports process exit. Drain it before judging success.
                    while let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
                        match event {
                            Ok(line) => match serde_json::from_slice(&line)
                                .map_err(|_| "Dictation returned an invalid response".to_string())
                                .and_then(|value| apply_event(session, value, &version))
                            {
                                Ok(done) => finished |= done,
                                Err(error) => {
                                    if let Ok(mut view) = session.view.lock() {
                                        view.error = Some(error);
                                    }
                                    finished = false;
                                    break;
                                }
                            },
                            Err(()) => {
                                finished = false;
                                break;
                            }
                        }
                    }
                    if !status.success() || !finished {
                        break Err(
                        "The dictation engine stopped unexpectedly. Your audio is saved for retry."
                            .into(),
                    );
                    }
                    break Ok(());
                }
                Err(_) => break Err("The dictation runtime stopped unexpectedly".into()),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if result.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        session.finish.store(true, Ordering::SeqCst);
        writer_done.store(true, Ordering::SeqCst);
        drop(events);
        let _ = writer.join();
        let _ = reader.join();
        result?;
    }
    let file = fs::File::open(output).map_err(|_| "Dictation result unavailable")?;
    let mut bytes = vec![];
    file.take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Dictation result unavailable")?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Dictation result too large".into());
    }
    let model: ModelTranscript =
        serde_json::from_slice(&bytes).map_err(|_| "Dictation result invalid")?;
    crate::noise_reduction::verify_result(&model, session.noise.as_ref())?;
    crate::asr::validate_model_identity(root, true, &model)?;
    if model.model_version != version {
        return Err("Dictation model version mismatch".into());
    }
    let captured = session
        .input
        .lock()
        .map_err(|_| "Dictation recording unavailable")?
        .samples as f64
        / snapshot(session)?.input_sample_rate as f64;
    let checked = words(&serde_json::to_value(&model.words).map_err(|_| "Invalid transcript")?)?;
    if checked.last().is_some_and(|w| w.end > captured + 0.04) {
        return Err("Dictation words extend beyond the recording".into());
    }
    let prior = snapshot(session)?.final_words;
    if prior.len() > model.words.len()
        || prior.iter().zip(&model.words).any(|(a, b)| {
            a.word != b.word || (a.start - b.start).abs() > 1e-6 || (a.end - b.end).abs() > 1e-6
        })
    {
        return Err("Dictation changed finalized words".into());
    }
    Ok(model)
}
fn finalize_capture(root: &Path, session: &Session) -> Result<Option<PathBuf>, String> {
    let id = snapshot(session)?.id;
    let mut input = session
        .input
        .lock()
        .map_err(|_| "Dictation recording unavailable")?;
    input.accepting = false;
    if let Some(writer) = input.writer.take() {
        writer
            .finalize()
            .map_err(|_| "Could not finish saving microphone audio. Check free storage.")?;
    }
    let dir = root.join("recordings").join(&id);
    if input.samples == 0 {
        let _ = fs::remove_dir_all(dir);
        return Ok(None);
    }
    let name = recording_name(&id);
    let temp = dir.join("name.pending");
    let mut f = fs::File::create(&temp).map_err(|_| "Could not save recording metadata")?;
    f.write_all(&serde_json::to_vec(&name).map_err(|_| "Could not save recording metadata")?)
        .and_then(|_| f.sync_all())
        .map_err(|_| "Could not save recording metadata")?;
    fs::rename(temp, dir.join("name.json")).map_err(|_| "Could not save recording metadata")?;
    if let Ok(mut v) = session.view.lock() {
        v.recording_id = Some(id);
    }
    Ok(Some(dir.join(name)))
}
fn run_session(root: &Path, session: &Arc<Session>, receiver: mpsc::Receiver<Vec<f32>>) {
    let model = supervise(root, session, receiver).and_then(|model| {
        if session.cancel.load(Ordering::SeqCst) {
            Err("cancelled".into())
        } else {
            Ok(model)
        }
    });
    let captured = finalize_capture(root, session);
    let result = match (model, captured) {
        (Ok(model), Ok(Some(audio))) => TranscriptStore::new(root.join("transcripts"))
            .and_then(|store| {
                let recording_id = audio
                    .parent()
                    .and_then(|dir| dir.file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                store.create_for_recording(&audio, &model, recording_id)
            })
            .map(|t| Some((t.id, model)))
            .map_err(|_| {
                "Your audio is saved, but the transcript could not be stored. Retry transcription."
                    .to_string()
            }),
        (Ok(_), Ok(None)) => Err("No microphone audio was recorded".into()),
        (Err(error), Ok(_)) => Err(error),
        (_, Err(error)) => Err(error),
    };
    if let Ok(mut v) = session.view.lock() {
        match result {
            Ok(Some((id, model))) => {
                v.state = "completed".into();
                v.stage = "Dictation saved".into();
                v.transcript_id = Some(id);
                v.final_words = model.words;
                v.partial_words.clear();
            }
            Ok(None) => {}
            Err(error) => {
                let failed = session.overflow.load(Ordering::SeqCst)
                    || v.error.is_some()
                    || error != "cancelled";
                v.state = if failed { "failed" } else { "cancelled" }.into();
                v.stage = if failed {
                    "Dictation stopped; captured audio is saved"
                } else {
                    "Dictation cancelled; captured audio is saved"
                }
                .into();
                if v.error.is_none() && failed {
                    v.error = Some(if session.overflow.load(Ordering::SeqCst) {
                        "Dictation stopped because audio could not be queued or saved. Captured audio remains available for transcription.".into()
                    } else {
                        error
                    });
                }
                v.partial_words.clear();
            }
        }
    }
}
pub fn forget_history(app: &tauri::AppHandle, transcript: Option<&str>, recordings: &[String]) {
    let state = app.state::<DictationState>();
    let mut sessions = state
        .sessions
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    sessions.retain(|_, session| {
        let view = session
            .view
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        transcript.is_none_or(|id| view.transcript_id.as_deref() != Some(id))
            && !view
                .recording_id
                .as_ref()
                .is_some_and(|id| recordings.contains(id))
    });
}
pub fn cancel_all(app: &tauri::AppHandle) {
    let state = app.state::<DictationState>();
    if let Ok(sessions) = state.sessions.lock() {
        for session in sessions.values() {
            session.cancel.store(true, Ordering::SeqCst);
        }
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let done = state
            .sessions
            .lock()
            .map(|sessions| {
                sessions
                    .values()
                    .all(|s| snapshot(s).is_ok_and(|v| !active(&v.state)))
            })
            .unwrap_or(true);
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, Arc<Session>, mpsc::Receiver<Vec<f32>>) {
        let root = std::env::temp_dir().join(format!("dictation-test-{}", id()));
        let (s, r) = new_session(&root).unwrap();
        (root, s, r)
    }
    fn ready(session: &Session) {
        apply_event(session,serde_json::json!({"protocol":1,"event":"ready","model_version":"fixture-v1","decode_seconds":1.5}),"fixture-v1").unwrap();
    }
    #[test]
    fn final_words_append_partial_words_replace_and_version_mismatch_is_rejected() {
        let (root, s, _) = fixture();
        ready(&s);
        apply_event(&s,serde_json::json!({"protocol":1,"event":"transcript","final":[],"partial":[{"word":"draft","start":0.0,"end":0.4}]}),"fixture-v1").unwrap();
        apply_event(&s,serde_json::json!({"protocol":1,"event":"transcript","final":[{"word":"checked","start":0.0,"end":0.5}],"partial":[{"word":"next","start":0.6,"end":1.0}]}),"fixture-v1").unwrap();
        apply_event(
            &s,
            serde_json::json!({"protocol":1,"event":"transcript","final":[],"partial":[]}),
            "fixture-v1",
        )
        .unwrap();
        let view = snapshot(&s).unwrap();
        assert_eq!(view.final_words[0].word, "checked");
        assert!(view.partial_words.is_empty());
        assert!(apply_event(&s,serde_json::json!({"protocol":1,"event":"transcript","final":[{"word":"rewrite","start":0.0,"end":0.4}],"partial":[]}),"fixture-v1").is_err());
        assert!(apply_event(
            &s,
            serde_json::json!({"protocol":1,"event":"ready","model_version":"wrong"}),
            "fixture-v1"
        )
        .is_err());
        finalize_capture(&root, &s).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recording_survives_full_queue_and_invalid_audio_is_rejected() {
        let (root, s, _receiver) = fixture();
        assert!(feed(&s, vec![0.1; 1600]).is_err());
        ready(&s);
        for invalid in [vec![f32::NAN], vec![1.01], vec![], vec![0.0; MAX_CHUNK + 1]] {
            assert!(feed(&s, invalid).is_err());
        }
        for _ in 0..24 {
            feed(&s, vec![0.25; 1600]).unwrap();
        }
        assert!(feed(&s, vec![0.5; 1600]).is_err());
        assert!(s.cancel.load(Ordering::SeqCst));
        let audio = finalize_capture(&root, &s).unwrap().unwrap();
        let reader = hound::WavReader::open(&audio).unwrap();
        assert_eq!(reader.duration(), 25 * 1600);
        assert_eq!(reader.spec().sample_rate, RATE);
        assert!(audio.parent().unwrap().join("name.json").exists());
        let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(samples[0], 8192);
        assert_eq!(*samples.last().unwrap(), 16384);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn noise_capture_preserves_pcm_at_negotiated_rate_independent_of_processing() {
        let root = std::env::temp_dir().join(format!("dictation-noise-{}", id()));
        let (s, _receiver) = new_session_with_noise(&root, Some(serde_json::json!({}))).unwrap();
        s.view.lock().unwrap().state = "ready".into();
        assert_eq!(snapshot(&s).unwrap().input_sample_rate, 48000);
        feed(&s, vec![0.25; 48000]).unwrap();
        assert!(feed(&s, vec![0.; 96001]).is_err());
        let audio = finalize_capture(&root, &s).unwrap().unwrap();
        let reader = hound::WavReader::open(audio).unwrap();
        assert_eq!(reader.spec().sample_rate, 48000);
        assert_eq!(reader.duration(), 48000);
        assert!(reader.into_samples::<i16>().all(|v| v.unwrap() == 8192));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn voice_task_lock_excludes_other_sessions() {
        let root = std::env::temp_dir().join(format!("dictation-lock-{}", id()));
        let first = process_lock(&root).unwrap();
        assert!(process_lock(&root).is_err());
        drop(first);
        assert!(process_lock(&root).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires local streaming ASR resource installation and a 16 kHz mono PCM16 fixture"]
    fn real_worker_streams_then_persists_transcript_and_recording() {
        let root = PathBuf::from(
            std::env::var("ETHIOPIC_DICTATION_TEST_ROOT").expect("dictation test root"),
        );
        let audio = PathBuf::from(
            std::env::var("ETHIOPIC_DICTATION_TEST_WAV").expect("dictation test audio"),
        );
        let original = fs::read(&audio).unwrap();
        let reader = hound::WavReader::new(std::io::Cursor::new(&original)).unwrap();
        let source_rate = reader.spec().sample_rate;
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().bits_per_sample, 16);
        let samples: Vec<f32> = reader
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect();
        // Exercise the same admission/queue path as the packaged command.
        let state = DictationState::new(root.clone());
        let editing = crate::voice_editing::EditingState::new(root.clone());
        let initial = begin_session(&state, &editing).unwrap();
        assert_eq!(initial.input_sample_rate, source_rate);
        let s = session(&state, &initial.id).unwrap();
        let started = Instant::now();
        while snapshot(&s).unwrap().state == "loading"
            && started.elapsed() < Duration::from_secs(90)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            snapshot(&s).unwrap().state,
            "ready",
            "{:?}",
            snapshot(&s).unwrap().error
        );
        for chunk in samples.chunks(source_rate as usize / 10) {
            feed(&s, chunk.to_vec()).unwrap_or_else(|error| {
                let view = snapshot(&s).unwrap();
                panic!(
                    "Audio feed failed: {error}; session state: {}; worker error: {:?}",
                    view.state, view.error
                );
            });
            // Replay at microphone rate; a long fixture sent ten times faster
            // measures queue overflow rather than streaming transcription.
            std::thread::sleep(Duration::from_secs_f64(
                chunk.len() as f64 / source_rate as f64,
            ));
        }
        s.input.lock().unwrap().accepting = false;
        s.finish.store(true, Ordering::SeqCst);
        let finishing = Instant::now();
        while active(&snapshot(&s).unwrap().state) && finishing.elapsed() < Duration::from_secs(90)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let view = snapshot(&s).unwrap();
        assert_eq!(view.state, "completed", "{:?}", view.error);
        assert!(view.partial_words.is_empty());
        assert!(!view.final_words.is_empty());
        let store = TranscriptStore::new(root.join("transcripts")).unwrap();
        let t = store.get(view.transcript_id.as_ref().unwrap()).unwrap();
        assert_eq!(Some(t.model_version), view.model_version);
        assert_eq!(t.words.len(), view.final_words.len());
        let saved = hound::WavReader::open(store.audio_path(&t.id).unwrap()).unwrap();
        assert_eq!(saved.spec().sample_rate, source_rate);
        assert_eq!(t.processing.is_some(), s.noise.is_some());
        assert_eq!(saved.duration() as usize, samples.len());
        assert_eq!(fs::read(&audio).unwrap(), original);
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    #[test]
    #[ignore = "requires installed streaming ASR resources"]
    fn cancellation_during_real_model_loading_releases_worker_and_session_files() {
        let root = PathBuf::from(
            std::env::var("ETHIOPIC_DICTATION_TEST_ROOT").expect("dictation test root"),
        );
        let (s, receiver) = new_session(&root).unwrap();
        let worker = s.clone();
        let worker_root = root.clone();
        let handle = std::thread::spawn(move || run_session(&worker_root, &worker, receiver));
        let started = Instant::now();
        while snapshot(&s).unwrap().stage != "Loading Amharic dictation tools"
            && started.elapsed() < Duration::from_secs(90)
        {
            if !active(&snapshot(&s).unwrap().state) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            snapshot(&s).unwrap().stage,
            "Loading Amharic dictation tools"
        );
        s.cancel.store(true, Ordering::SeqCst);
        handle.join().unwrap();
        let view = snapshot(&s).unwrap();
        assert_eq!(view.state, "cancelled");
        assert!(view.transcript_id.is_none());
        assert!(!root.join("dictation-jobs").join(&view.id).exists());
        assert!(!root.join("recordings").join(view.id).exists());
    }
}
