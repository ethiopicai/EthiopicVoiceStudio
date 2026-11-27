//! Explicit user actions on the latest native transcript. No user content is logged.
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
use transcripts::{Transcript, TranscriptStore};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const MAX_AUDIO: u64 = 100 * 1024 * 1024;
#[derive(Serialize)]
pub struct Exported {
    pub name: String,
    pub path: String,
    pub message: String,
}
fn safe_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Err("Invalid transcript ID".into())
    } else {
        Ok(())
    }
}
fn transcript_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    safe_id(id)?;
    let base = root
        .canonicalize()
        .map_err(|_| "Transcript storage unavailable")?;
    let path = root.join("transcripts").join(id);
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "Transcript no longer exists. Refresh history.")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || path.canonicalize().map_err(|_| "Transcript unavailable")?
            != base.join("transcripts").join(id)
    {
        return Err("Refusing redirected transcript storage".into());
    }
    Ok(path)
}
fn load(root: &Path, id: &str) -> Result<Transcript, String> {
    let dir = transcript_dir(root, id)?;
    let metadata_path = dir.join("transcript.json");
    let metadata = fs::symlink_metadata(&metadata_path).map_err(|_| "Transcript unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 8 * 1024 * 1024
    {
        return Err("Refusing redirected or invalid transcript metadata".into());
    }
    TranscriptStore::new(root.join("transcripts"))
        .and_then(|store| store.get(id))
        .map_err(|_| "Transcript could not be read. Refresh history and retry.".into())
}
fn wav_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    let transcript = load(root, id)?;
    if transcript.audio_file != "audio.wav" {
        return Err("Invalid transcript audio reference".into());
    }
    let dir = transcript_dir(root, id)?;
    let path = dir.join("audio.wav");
    let metadata = fs::symlink_metadata(&path).map_err(|_| "The saved WAV is unavailable")?;
    let expected = root
        .canonicalize()
        .map_err(|_| "Transcript storage unavailable")?
        .join("transcripts")
        .join(id)
        .join("audio.wav");
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_AUDIO
        || path
            .canonicalize()
            .map_err(|_| "The saved WAV is unavailable")?
            != expected
    {
        return Err("Refusing redirected or invalid WAV storage".into());
    }
    Ok(expected)
}
fn string_path(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or("This file path cannot be represented as text".into())
}
#[cfg(target_os = "macos")]
pub(crate) fn copy(value: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new("/usr/bin/pbcopy")
        .env("LANG", "en_US.UTF-8")
        .env("LC_CTYPE", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Clipboard unavailable. Please try again.")?;
    let written = child
        .stdin
        .take()
        .ok_or("Clipboard unavailable")
        .and_then(|mut input| {
            input
                .write_all(value.as_bytes())
                .map_err(|_| "Clipboard could not be updated")
        });
    if written.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Clipboard could not be updated".into());
    }
    if !child.wait().map_err(|_| "Clipboard unavailable")?.success() {
        return Err("Clipboard could not be updated".into());
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn copy(_value: &str) -> Result<(), String> {
    Err("Native clipboard actions are not available on this platform yet.".into())
}
#[tauri::command]
pub async fn copy_transcript_text(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Transcript storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || copy(&load(&root, &id)?.text))
        .await
        .map_err(|_| "Clipboard action failed")?
}
#[tauri::command]
pub async fn get_transcript_wav_path(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Transcript storage unavailable")?;
    string_path(&wav_path(&root, &id)?)
}
#[tauri::command]
pub async fn copy_transcript_wav_path(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Transcript storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || copy(&string_path(&wav_path(&root, &id)?)?))
        .await
        .map_err(|_| "Clipboard action failed")?
}
struct Cue {
    start: u64,
    end: u64,
    text: String,
}
fn milliseconds(seconds: f64) -> Result<u64, String> {
    if !seconds.is_finite() || !(0.0..=3_155_760_000.0).contains(&seconds) {
        return Err("Transcript contains invalid timing values".into());
    }
    Ok((seconds * 1000.0).round() as u64)
}
fn caption_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn validate_timings(transcript: &Transcript) -> Result<(), String> {
    let mut previous = 0.0;
    for (index, word) in transcript.words.iter().enumerate() {
        if word.id != index
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < previous
            || word.end < word.start
        {
            return Err("Transcript word timings are invalid".into());
        }
        milliseconds(word.start)?;
        milliseconds(word.end)?;
        previous = word.end;
    }
    for correction in &transcript.corrections {
        if correction.start_word >= correction.end_word_exclusive
            || correction.end_word_exclusive > transcript.words.len()
            || !correction.audio_start.is_finite()
            || !correction.audio_end.is_finite()
            || correction.audio_start < 0.0
            || correction.audio_end < correction.audio_start
        {
            return Err("Transcript correction timings are invalid".into());
        }
    }
    Ok(())
}
fn cues(transcript: &Transcript) -> Result<Vec<Cue>, String> {
    validate_timings(transcript)?;
    let mut cues: Vec<Cue> = vec![];
    for (index, word) in transcript.words.iter().enumerate() {
        let text = caption_text(&word.corrected);
        if text.is_empty() {
            continue;
        }
        let (start, end) = if let Some(correction) = transcript
            .corrections
            .iter()
            .rev()
            .find(|c| c.start_word <= index && index < c.end_word_exclusive)
        {
            let span = &transcript.words[correction.start_word..correction.end_word_exclusive];
            if correction.start_word != index
                || correction.replacement != word.corrected
                || span.iter().skip(1).any(|word| !word.corrected.is_empty())
                || (correction.audio_start - span[0].start).abs() > 1e-6
                || (correction.audio_end - span[span.len() - 1].end).abs() > 1e-6
            {
                return Err(
                    "Transcript correction span needs review before subtitle export".into(),
                );
            }
            (correction.audio_start, correction.audio_end)
        } else {
            (word.start, word.end)
        };
        let start = milliseconds(start)?;
        let end = milliseconds(end)?;
        if end <= start || cues.last().is_some_and(|cue| start < cue.end) {
            return Err("Visible subtitle words need positive, chronological millisecond spans. Review word timings before exporting.".into());
        }
        cues.push(Cue { start, end, text });
    }
    Ok(cues)
}
fn stamp(milliseconds: u64, separator: char) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds / 60_000 % 60;
    let seconds = milliseconds / 1000 % 60;
    let fraction = milliseconds % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{fraction:03}")
}
fn serialize(transcript: &Transcript, format: &str, wav: &Path) -> Result<Vec<u8>, String> {
    if !matches!(format, "vtt" | "srt" | "json") {
        return Err("Choose VTT, SRT, or JSON export".into());
    }
    if format == "json" {
        validate_timings(transcript)?;
        return serde_json::to_vec_pretty(&serde_json::json!({"schema_version":1,"timing_unit":"seconds","timing_reference":"original managed WAV; corrections retain original audio spans","wav_reference":{"path":string_path(wav)?,"transcript_audio_file":transcript.audio_file},"transcript":transcript})).map_err(|_|"Transcript JSON could not be exported".into());
    }
    let cues = cues(transcript)?;
    let mut output = if format == "vtt" {
        "WEBVTT\n\n".to_owned()
    } else {
        String::new()
    };
    for (index, cue) in cues.iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            stamp(cue.start, if format == "vtt" { '.' } else { ',' }),
            stamp(cue.end, if format == "vtt" { '.' } else { ',' }),
            cue.text
        ));
    }
    Ok(output.into_bytes())
}
fn write_new(destination: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    fs::create_dir_all(destination)
        .map_err(|_| "Downloads could not be opened. The transcript remains saved in the app.")?;
    let path = destination.join(name);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|_| "Could not create export. Existing files are unchanged.")?;
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err("Export did not finish. The transcript remains saved in the app.".into());
    }
    Ok(path)
}
fn export_to(root: &Path, destination: &Path, id: &str, format: &str) -> Result<Exported, String> {
    if !matches!(format, "vtt" | "srt" | "json") {
        return Err("Choose VTT, SRT, or JSON export".into());
    }
    let transcript = load(root, id)?;
    let wav = wav_path(root, id)?;
    let bytes = serialize(&transcript, format, &wav)?;
    let stem = Path::new(&transcript.name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("transcript");
    let mut safe = String::new();
    for c in stem
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
    {
        if safe.len() + c.len_utf8() > 120 {
            break;
        }
        safe.push(c);
    }
    if safe.is_empty() {
        safe = "transcript".into();
    }
    let nonce = format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let name = format!("{safe}-transcript-{nonce}.{format}");
    let path = write_new(destination, &name, &bytes)?;
    Ok(Exported {
        name,
        path: string_path(&path)?,
        message:
            "Saved to Downloads/EthiopicVoiceStudio. Your recording and transcript are unchanged."
                .into(),
    })
}
#[tauri::command]
pub async fn export_transcript(
    app: tauri::AppHandle,
    id: String,
    format: String,
) -> Result<Exported, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Transcript storage unavailable")?;
    let destination = app
        .path()
        .download_dir()
        .map_err(|_| "Downloads folder unavailable")?
        .join("EthiopicVoiceStudio");
    tauri::async_runtime::spawn_blocking(move || export_to(&root, &destination, &id, &format))
        .await
        .map_err(|_| "Export could not finish")?
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        original: PathBuf,
        store: TranscriptStore,
        transcript: Transcript,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "transcript-actions-{}-{}-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            let original = root.join("Amharic.wav");
            fs::write(&original, b"original audio").unwrap();
            let store = TranscriptStore::new(root.join("app/transcripts")).unwrap();
            let model = transcripts::ModelTranscript {
                processing: None,
                model: None,
                text: "ሰላም ዓለም again".into(),
                model_version: "amharic-v1".into(),
                words: vec![
                    transcripts::ModelWord {
                        word: "ሰላም".into(),
                        start: 0.0,
                        end: 0.5,
                    },
                    transcripts::ModelWord {
                        word: "ዓለም".into(),
                        start: 0.7,
                        end: 1.2,
                    },
                    transcripts::ModelWord {
                        word: "again".into(),
                        start: 1.5,
                        end: 2.0,
                    },
                ],
            };
            let transcript = store.create(&original, &model).unwrap();
            Self {
                root,
                original,
                store,
                transcript,
            }
        }
        fn app(&self) -> PathBuf {
            self.root.join("app")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn corrected_span_exports_original_full_interval_and_deleted_words_are_omitted() {
        let f = Fixture::new();
        let corrected = f
            .store
            .correct(&f.transcript.id, 1, 0, 2, "እንዴት ነህ".into())
            .unwrap();
        let corrected = f
            .store
            .correct(&corrected.id, corrected.revision, 2, 3, String::new())
            .unwrap();
        let vtt = String::from_utf8(serialize(&corrected, "vtt", Path::new("audio.wav")).unwrap())
            .unwrap();
        assert_eq!(
            vtt,
            "WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.200\nእንዴት ነህ\n\n"
        );
        let srt = String::from_utf8(serialize(&corrected, "srt", Path::new("audio.wav")).unwrap())
            .unwrap();
        assert_eq!(srt, "1\n00:00:00,000 --> 00:00:01,200\nእንዴት ነህ\n\n");
        let json: serde_json::Value =
            serde_json::from_slice(&serialize(&corrected, "json", Path::new("audio.wav")).unwrap())
                .unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["timing_unit"], "seconds");
        assert_eq!(
            json["transcript"],
            serde_json::to_value(&corrected).unwrap()
        );
        assert_eq!(
            json["transcript"]["corrections"].as_array().unwrap().len(),
            2
        );
    }
    #[test]
    fn captions_escape_markup_and_cue_injection_but_json_retains_exact_text() {
        let f = Fixture::new();
        let text = "ሰላም & <b>\n\n99\n00:00:01.000 --> 00:00:03.000\r\nboom\0";
        let corrected = f
            .store
            .correct(&f.transcript.id, 1, 0, 1, text.into())
            .unwrap();
        let vtt = String::from_utf8(serialize(&corrected, "vtt", Path::new("audio.wav")).unwrap())
            .unwrap();
        assert!(vtt.contains("ሰላም &amp; &lt;b&gt; 99 00:00:01.000 --&gt; 00:00:03.000 boom"));
        assert_eq!(vtt.matches(" --> ").count(), 3);
        assert!(!vtt.contains('\0'));
        assert!(!vtt.contains("<b>"));
        let json: serde_json::Value =
            serde_json::from_slice(&serialize(&corrected, "json", Path::new("audio.wav")).unwrap())
                .unwrap();
        assert_eq!(json["transcript"]["words"][0]["corrected"], text);
    }
    #[test]
    fn subtitle_rounding_rejects_nonpositive_spans_and_json_preserves_zero_duration() {
        let f = Fixture::new();
        let mut t = f.transcript.clone();
        t.words[0].end = 0.0001;
        assert!(serialize(&t, "vtt", Path::new("audio.wav")).is_err());
        t.words[0].end = 0.0;
        assert!(serialize(&t, "srt", Path::new("audio.wav")).is_err());
        assert!(serialize(&t, "json", Path::new("audio.wav")).is_ok());
        t.words[0].start = f64::NAN;
        assert!(serialize(&t, "json", Path::new("audio.wav")).is_err());
        assert_eq!(stamp(milliseconds(3599.9996).unwrap(), '.'), "01:00:00.000");
        assert_eq!(stamp(3661002, ','), "01:01:01,002");
        let mut empty = f.transcript.clone();
        for word in &mut empty.words {
            word.corrected.clear();
        }
        assert_eq!(
            serialize(&empty, "vtt", Path::new("audio.wav")).unwrap(),
            b"WEBVTT\n\n"
        );
        assert!(serialize(&empty, "srt", Path::new("audio.wav"))
            .unwrap()
            .is_empty());
    }
    #[test]
    fn exports_latest_revision_to_unique_files_without_modifying_original_or_overwriting() {
        let f = Fixture::new();
        f.store
            .correct(&f.transcript.id, 1, 0, 1, "updated latest".into())
            .unwrap();
        let destination = f.root.join("exports");
        let a = export_to(&f.app(), &destination, &f.transcript.id, "vtt").unwrap();
        let b = export_to(&f.app(), &destination, &f.transcript.id, "vtt").unwrap();
        assert_ne!(a.path, b.path);
        assert!(fs::read_to_string(&a.path)
            .unwrap()
            .contains("updated latest"));
        assert_eq!(fs::read(&f.original).unwrap(), b"original audio");
        let existing = destination.join("existing.vtt");
        fs::write(&existing, b"keep existing").unwrap();
        assert!(write_new(&destination, "existing.vtt", b"overwrite").is_err());
        assert_eq!(fs::read(existing).unwrap(), b"keep existing");
        assert!(export_to(&f.app(), &destination, &f.transcript.id, "../escape").is_err());
        assert!(wav_path(&f.app(), "../external").is_err());
        assert_eq!(
            wav_path(&f.app(), &f.transcript.id).unwrap(),
            f.app()
                .canonicalize()
                .unwrap()
                .join("transcripts")
                .join(&f.transcript.id)
                .join("audio.wav")
        );
    }
    #[cfg(unix)]
    #[test]
    fn wav_paths_reject_external_symlinks_and_missing_audio() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let audio = f.store.audio_path(&f.transcript.id).unwrap();
        fs::remove_file(&audio).unwrap();
        symlink(&f.original, &audio).unwrap();
        assert!(wav_path(&f.app(), &f.transcript.id).is_err());
        assert!(export_to(&f.app(), &f.root.join("exports"), &f.transcript.id, "json").is_err());
        assert_eq!(fs::read(&f.original).unwrap(), b"original audio");
        fs::remove_file(&audio).unwrap();
        assert!(wav_path(&f.app(), &f.transcript.id).is_err());
        let metadata = f
            .app()
            .join("transcripts")
            .join(&f.transcript.id)
            .join("transcript.json");
        let external = f.root.join("external.json");
        fs::copy(&metadata, &external).unwrap();
        fs::remove_file(&metadata).unwrap();
        symlink(&external, &metadata).unwrap();
        assert!(load(&f.app(), &f.transcript.id).is_err());
    }
}
