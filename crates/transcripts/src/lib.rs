//! Local transcript storage. Corrections preserve model timing; they never realign speech.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self(e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Atomically replace validated app-owned metadata while the caller holds its storage lock.
pub fn replace_metadata(from: &Path, to: &Path) -> std::io::Result<()> {
    replace(from, to)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoiseReduction {
    pub provider: String,
    pub revision: String,
    pub sha256: String,
    pub adapter_version: u32,
    pub lookahead_ms: u16,
    pub device: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognitionProcessing {
    pub noise_reduction: NoiseReduction,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTranscript {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing: Option<RecognitionProcessing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub text: String,
    pub words: Vec<ModelWord>,
    pub model_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub id: usize,
    pub word: String,
    pub corrected: String,
    pub start: f64,
    pub end: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub human_checked: bool,
    pub start_word: usize,
    pub end_word_exclusive: usize,
    pub audio_start: f64,
    pub audio_end: f64,
    pub original_text: String,
    pub previous_text: String,
    pub replacement: String,
    pub timestamp_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing: Option<RecognitionProcessing>,
    pub id: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub model_version: String,
    pub name: String,
    pub original_text: String,
    pub text: String,
    pub words: Vec<Word>,
    pub corrections: Vec<Correction>,
    pub reviewed: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub audio_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_recording_id: Option<String>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn fail(s: &str) -> Error {
    Error(s.into())
}
fn safe_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Err(fail("Invalid transcript ID"))
    } else {
        Ok(())
    }
}
fn validate_model(m: &ModelTranscript) -> Result<()> {
    if let Some(processing) = &m.processing {
        let n = &processing.noise_reduction;
        if n.provider != "inworld-voice-isolator-v1-realtime"
            || n.revision.len() != 40
            || !n.revision.bytes().all(|c| c.is_ascii_hexdigit())
            || n.sha256.len() != 64
            || !n.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            || n.adapter_version != 1
            || !matches!(n.lookahead_ms, 0 | 20 | 40)
            || !matches!(n.device.as_str(), "cpu" | "mps")
        {
            return Err(fail("Invalid recognition preprocessing metadata"));
        }
    }
    if m.model.as_ref().is_some_and(|name| {
        name.trim().is_empty() || name.len() > 200 || name.chars().any(char::is_control)
    }) {
        return Err(fail("Model identity is invalid"));
    }
    if m.model_version.trim().is_empty() {
        return Err(fail("Model version is required"));
    }
    let mut last = 0.0;
    for w in &m.words {
        if w.word.trim().is_empty()
            || !w.start.is_finite()
            || !w.end.is_finite()
            || w.start < 0.0
            || w.end < w.start
            || w.start < last
        {
            return Err(fail(
                "Words need finite, nonnegative chronological timestamps",
            ));
        }
        last = w.end;
    }
    Ok(())
}
#[derive(Clone)]
pub struct TranscriptStore {
    root: PathBuf,
}
impl TranscriptStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
    fn lock(&self) -> Result<File> {
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".lock"))?;
        f.lock_exclusive()?;
        Ok(f)
    }
    fn dir(&self, id: &str) -> Result<PathBuf> {
        safe_id(id)?;
        Ok(self.root.join(id))
    }
    fn read(&self, id: &str) -> Result<Transcript> {
        let dir = self.dir(id)?;
        if fs::symlink_metadata(&self.root)?.file_type().is_symlink()
            || fs::symlink_metadata(&dir)?.file_type().is_symlink()
            || dir.canonicalize()? != self.root.canonicalize()?.join(id)
        {
            return Err(fail("Refusing redirected transcript storage"));
        }
        let file = dir.join("transcript.json");
        let metadata = fs::symlink_metadata(&file)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > 16 * 1024 * 1024
        {
            return Err(fail("Invalid transcript metadata file"));
        }
        let t: Transcript = serde_json::from_slice(&fs::read(file)?)?;
        if t.id != id || t.audio_file != "audio.wav" {
            return Err(fail("Invalid transcript metadata"));
        }
        if let Some(recording_id) = &t.source_recording_id {
            safe_id(recording_id)?;
        }
        Ok(t)
    }
    /// Copies a WAV recording into managed storage; original recordings are never modified.
    pub fn create(
        &self,
        audio_path: impl AsRef<Path>,
        model: &ModelTranscript,
    ) -> Result<Transcript> {
        self.create_inner(audio_path, model, None, None, None)
    }
    /// Link a completed transcript to its existing managed draft recording.
    pub fn create_for_recording(
        &self,
        audio_path: impl AsRef<Path>,
        model: &ModelTranscript,
        recording_id: &str,
    ) -> Result<Transcript> {
        safe_id(recording_id)?;
        self.create_inner(audio_path, model, Some(recording_id), None, None)
    }
    /// Import independent audio plus its source context. A snapshot preserves checked
    /// corrections when the caller has verified that the copied audio is identical.
    pub fn create_transferred(
        &self,
        audio_path: impl AsRef<Path>,
        model: &ModelTranscript,
        recording_id: Option<&str>,
        context: &[u8],
        snapshot: Option<&Transcript>,
    ) -> Result<Transcript> {
        if context.len() > 8 * 1024 * 1024 {
            return Err(fail("Recording context is too large"));
        }
        if let Some(id) = recording_id {
            safe_id(id)?;
        }
        self.create_inner(audio_path, model, recording_id, Some(context), snapshot)
    }
    fn create_inner(
        &self,
        audio_path: impl AsRef<Path>,
        model: &ModelTranscript,
        recording_id: Option<&str>,
        context: Option<&[u8]>,
        snapshot: Option<&Transcript>,
    ) -> Result<Transcript> {
        validate_model(model)?;
        if let Some(snapshot) = snapshot {
            validate_model(&ModelTranscript {
                processing: snapshot.processing.clone(),
                model: snapshot.model.clone(),
                model_version: snapshot.model_version.clone(),
                text: snapshot.original_text.clone(),
                words: snapshot
                    .words
                    .iter()
                    .map(|w| ModelWord {
                        word: w.word.clone(),
                        start: w.start,
                        end: w.end,
                    })
                    .collect(),
            })?;
            if snapshot.text.len() > 200000
                || snapshot
                    .words
                    .iter()
                    .enumerate()
                    .any(|(i, w)| w.id != i || w.corrected.len() > 4096)
            {
                return Err(fail("Invalid transferred transcript"));
            }
        }
        let _lock = self.lock()?;
        let timestamp = now();
        let id = format!(
            "{}-{}-{}",
            timestamp,
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        );
        let pending = self.root.join(format!(".pending-{id}"));
        fs::create_dir(&pending)?;
        let result = (|| {
            fs::copy(audio_path.as_ref(), pending.join("audio.wav"))?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(pending.join("audio.wav"))?
                .sync_all()?;
            let mut t = Transcript {
                processing: model.processing.clone(),
                id: id.clone(),
                revision: 1,
                model: model.model.clone(),
                model_version: model.model_version.clone(),
                name: audio_path
                    .as_ref()
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "audio.wav".into()),
                original_text: model.text.clone(),
                text: model.text.clone(),
                words: model
                    .words
                    .iter()
                    .enumerate()
                    .map(|(id, w)| Word {
                        id,
                        word: w.word.clone(),
                        corrected: w.word.clone(),
                        start: w.start,
                        end: w.end,
                    })
                    .collect(),
                corrections: vec![],
                reviewed: false,
                created_at_ms: timestamp,
                updated_at_ms: timestamp,
                audio_file: "audio.wav".into(),
                source_recording_id: recording_id.map(str::to_owned),
            };
            if let Some(source) = snapshot {
                let new_id = t.id;
                let created = t.created_at_ms;
                let linked = t.source_recording_id;
                t = source.clone();
                t.id = new_id;
                t.revision = 1;
                t.created_at_ms = created;
                t.updated_at_ms = created;
                t.audio_file = "audio.wav".into();
                t.source_recording_id = linked;
            }
            if let Some(context) = context {
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(pending.join("source-media.json"))?;
                file.write_all(context)?;
                file.sync_all()?;
            }
            self.write(&pending, &t)?;
            fs::rename(&pending, self.dir(&id)?)?;
            sync_dir(&self.root)?;
            Ok(t)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(pending);
        }
        result
    }
    pub fn list(&self) -> Result<Vec<Transcript>> {
        let _lock = self.lock()?;
        let mut items = vec![];
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_dir() && !name.starts_with('.') {
                items.push(self.read(&name)?);
            }
        }
        items.sort_by_key(|t| std::cmp::Reverse(t.created_at_ms));
        Ok(items)
    }
    pub fn get(&self, id: &str) -> Result<Transcript> {
        let _lock = self.lock()?;
        self.read(id)
    }
    pub fn audio_path(&self, id: &str) -> Result<PathBuf> {
        let _lock = self.lock()?;
        self.read(id)?;
        Ok(self.dir(id)?.join("audio.wav"))
    }
    /// Permanently remove only this store's managed copy, including corrections.
    /// Revision checks prevent a stale window from deleting newly edited work.
    pub fn delete(&self, id: &str, revision: u64) -> Result<()> {
        if fs::symlink_metadata(&self.root)?.file_type().is_symlink() {
            return Err(fail("Refusing to delete redirected managed storage"));
        }
        let _lock = self.lock()?;
        let dir = self.dir(id)?;
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        reject_links(&dir)?;
        let metadata = dir.join("transcript.json");
        if metadata.exists() {
            let t = self.read(id)?;
            if t.revision != revision {
                return Err(fail("Revision conflict: reload transcript before deleting"));
            }
        } else if fs::read_dir(&dir)?.next().is_some() {
            return Err(fail(
                "Transcript metadata is unavailable; deletion was not completed",
            ));
        }
        // Keep metadata until other files are gone so interrupted deletion can retry.
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path != metadata {
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
            }
        }
        if metadata.exists() {
            fs::remove_file(metadata)?;
        }
        fs::remove_dir(dir)?;
        sync_dir(&self.root)?;
        Ok(())
    }
    fn write(&self, dir: &Path, t: &Transcript) -> Result<()> {
        let temp = dir.join("transcript.next");
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)?;
        f.write_all(&serde_json::to_vec_pretty(t)?)?;
        f.sync_all()?;
        drop(f);
        // POSIX rename replaces atomically. Windows needs ReplaceFileW for existing files.
        replace(&temp, &dir.join("transcript.json"))?;
        sync_dir(dir)?;
        Ok(())
    }
    fn update(
        &self,
        id: &str,
        revision: u64,
        edit: impl FnOnce(&mut Transcript) -> Result<()>,
    ) -> Result<Transcript> {
        let _lock = self.lock()?;
        let mut t = self.read(id)?;
        if t.revision != revision {
            return Err(fail("Revision conflict: reload transcript before saving"));
        }
        edit(&mut t)?;
        t.revision = t
            .revision
            .checked_add(1)
            .ok_or_else(|| fail("Revision exhausted"))?;
        t.updated_at_ms = now();
        self.write(&self.dir(id)?, &t)?;
        Ok(t)
    }
    pub fn set_reviewed(&self, id: &str, revision: u64, reviewed: bool) -> Result<Transcript> {
        self.update(id, revision, |t| {
            t.reviewed = reviewed;
            Ok(())
        })
    }
    /// Replace a span of original words, retaining its original audio boundaries.
    /// Replacement text is stored on the first word; remaining span words become empty.
    pub fn correct(
        &self,
        id: &str,
        revision: u64,
        start_word: usize,
        end_word_exclusive: usize,
        replacement: String,
    ) -> Result<Transcript> {
        self.update(id, revision, |t| {
            if start_word >= end_word_exclusive || end_word_exclusive > t.words.len() {
                return Err(fail("Invalid word span"));
            }
            // Reject partially overlapping prior spans: their replacement cannot be split without alignment.
            for c in &t.corrections {
                if start_word < c.end_word_exclusive
                    && end_word_exclusive > c.start_word
                    && !(start_word <= c.start_word && end_word_exclusive >= c.end_word_exclusive)
                {
                    return Err(fail("Select the complete previously corrected span"));
                }
            }
            let span = &t.words[start_word..end_word_exclusive];
            let correction = Correction {
                human_checked: true,
                start_word,
                end_word_exclusive,
                audio_start: span[0].start,
                audio_end: span[span.len() - 1].end,
                original_text: span
                    .iter()
                    .map(|w| w.word.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                previous_text: span
                    .iter()
                    .map(|w| w.corrected.as_str())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                replacement: replacement.clone(),
                timestamp_ms: now(),
            };
            for w in &mut t.words[start_word..end_word_exclusive] {
                w.corrected.clear();
            }
            t.words[start_word].corrected = replacement;
            t.corrections.push(correction);
            t.text = t
                .words
                .iter()
                .map(|w| w.corrected.as_str())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            t.reviewed = false;
            Ok(())
        })
    }
}
fn reject_links(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(fail("Refusing to delete redirected managed storage"));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            reject_links(&entry?.path())?;
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}
#[cfg(unix)]
fn sync_dir(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}
#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(old: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let old: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let new: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(old.as_ptr(), new.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        store: TranscriptStore,
        audio: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "transcripts-test-{}-{}-{}",
                now(),
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&root).unwrap();
            let audio = root.join("original.wav");
            fs::write(&audio, b"test audio").unwrap();
            let store = TranscriptStore::new(root.join("store")).unwrap();
            Self { root, store, audio }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn model() -> ModelTranscript {
        ModelTranscript {
            processing: None,
            model: None,
            text: "Hello, world!".into(),
            model_version: "ethiopic-asr-1".into(),
            words: vec![
                ModelWord {
                    word: "Hello".into(),
                    start: 0.0,
                    end: 0.5,
                },
                ModelWord {
                    word: "world".into(),
                    start: 0.7,
                    end: 1.2,
                },
            ],
        }
    }
    #[test]
    fn recognition_processing_survives_corrections_without_changing_original_audio() {
        let f = Fixture::new();
        let mut input = model();
        input.processing = Some(RecognitionProcessing {
            noise_reduction: NoiseReduction {
                provider: "inworld-voice-isolator-v1-realtime".into(),
                revision: "a".repeat(40),
                sha256: "b".repeat(64),
                adapter_version: 1,
                lookahead_ms: 20,
                device: "mps".into(),
            },
        });
        let original = fs::read(&f.audio).unwrap();
        let t = f.store.create(&f.audio, &input).unwrap();
        let corrected = f
            .store
            .correct(&t.id, t.revision, 0, 1, "Corrected".into())
            .unwrap();
        assert_eq!(
            corrected.processing.unwrap().noise_reduction.sha256,
            "b".repeat(64)
        );
        assert_eq!(
            f.store
                .get(&t.id)
                .unwrap()
                .processing
                .unwrap()
                .noise_reduction
                .lookahead_ms,
            20
        );
        assert_eq!(
            fs::read(f.store.audio_path(&t.id).unwrap()).unwrap(),
            original
        );
        let mut legacy = serde_json::to_value(&input).unwrap();
        legacy.as_object_mut().unwrap().remove("processing");
        assert!(serde_json::from_value::<ModelTranscript>(legacy)
            .unwrap()
            .processing
            .is_none());
    }
    #[test]
    fn model_family_is_retained_with_same_version_and_legacy_identity_is_not_guessed() {
        let f = Fixture::new();
        let mut large = model();
        large.model = Some("amharic-ctc".into());
        large.model_version = "v0.3".into();
        let mut small = large.clone();
        small.model = Some("amharic-ctc-300m".into());
        let a = f.store.create(&f.audio, &large).unwrap();
        let b = f.store.create(&f.audio, &small).unwrap();
        assert_eq!(a.model_version, b.model_version);
        assert_ne!(a.model, b.model);
        let corrected = f
            .store
            .correct(&b.id, b.revision, 0, 1, "Correction".into())
            .unwrap();
        assert_eq!(corrected.model, small.model);
        assert_eq!(f.store.get(&a.id).unwrap().model, large.model);
        let mut legacy = serde_json::to_value(&a).unwrap();
        legacy.as_object_mut().unwrap().remove("model");
        let legacy: Transcript = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.model, None);
        let mut invalid = large;
        invalid.model = Some("\n".into());
        assert!(f.store.create(&f.audio, &invalid).is_err());
    }
    #[test]
    fn source_recording_provenance_persists_and_legacy_metadata_remains_readable() {
        let f = Fixture::new();
        let linked = f
            .store
            .create_for_recording(&f.audio, &model(), "import-123")
            .unwrap();
        assert_eq!(
            f.store
                .get(&linked.id)
                .unwrap()
                .source_recording_id
                .as_deref(),
            Some("import-123")
        );
        let updated = f
            .store
            .correct(&linked.id, linked.revision, 0, 1, "corrected".into())
            .unwrap();
        assert_eq!(updated.source_recording_id.as_deref(), Some("import-123"));
        let legacy = f.store.create(&f.audio, &model()).unwrap();
        let json = serde_json::to_value(&legacy).unwrap();
        assert!(json.get("source_recording_id").is_none());
        assert!(f
            .store
            .get(&legacy.id)
            .unwrap()
            .source_recording_id
            .is_none());
        assert!(f
            .store
            .create_for_recording(&f.audio, &model(), "../external")
            .is_err());
    }
    #[test]
    fn deletion_erases_corrections_and_copy_but_preserves_original_and_other_history() {
        let f = Fixture::new();
        let first = f.store.create(&f.audio, &model()).unwrap();
        let other = f.store.create(&f.audio, &model()).unwrap();
        let corrected = f
            .store
            .correct(&first.id, first.revision, 0, 1, "private correction".into())
            .unwrap();
        assert!(f.store.delete(&first.id, first.revision).is_err());
        assert!(f.store.get(&first.id).is_ok());
        f.store.delete(&first.id, corrected.revision).unwrap();
        assert!(!f.store.dir(&first.id).unwrap().exists());
        assert_eq!(fs::read(&f.audio).unwrap(), b"test audio");
        assert_eq!(f.store.list().unwrap().len(), 1);
        assert_eq!(f.store.get(&other.id).unwrap().text, model().text);
        assert!(f
            .store
            .correct(&first.id, corrected.revision, 0, 1, "resurrect".into())
            .is_err());
        f.store.delete(&first.id, corrected.revision).unwrap();
        assert!(f.store.delete("../original.wav", 1).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn deletion_rejects_symlinked_transcript_or_nested_directory() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let t = f.store.create(&f.audio, &model()).unwrap();
        symlink(&f.root, f.store.dir(&t.id).unwrap().join("escape")).unwrap();
        assert!(f.store.delete(&t.id, t.revision).is_err());
        assert_eq!(fs::read(&f.audio).unwrap(), b"test audio");
        fs::remove_file(f.store.dir(&t.id).unwrap().join("escape")).unwrap();
        let real = f.store.dir(&t.id).unwrap();
        let moved = f.root.join("external-transcript");
        fs::rename(&real, &moved).unwrap();
        symlink(&moved, &real).unwrap();
        assert!(f.store.delete(&t.id, t.revision).is_err());
        assert!(moved.join("transcript.json").exists());
    }
    #[test]
    fn create_copies_audio_and_preserves_model_output() {
        let f = Fixture::new();
        let t = f.store.create(&f.audio, &model()).unwrap();
        fs::remove_file(&f.audio).unwrap();
        assert_eq!(
            fs::read(f.store.audio_path(&t.id).unwrap()).unwrap(),
            b"test audio"
        );
        let loaded = f.store.get(&t.id).unwrap();
        assert_eq!(loaded.original_text, "Hello, world!");
        assert_eq!(loaded.model_version, "ethiopic-asr-1");
        assert!(!loaded.reviewed);
        assert_eq!(f.store.list().unwrap().len(), 1);
    }
    #[test]
    fn multiword_correction_preserves_timing_history_and_invalidates_review() {
        let f = Fixture::new();
        let t = f.store.create(&f.audio, &model()).unwrap();
        let t = f.store.set_reviewed(&t.id, t.revision, true).unwrap();
        let t = f
            .store
            .correct(&t.id, t.revision, 0, 2, "A new phrase".into())
            .unwrap();
        assert_eq!(t.text, "A new phrase");
        assert_eq!(t.words[0].word, "Hello");
        assert_eq!(t.words[1].corrected, "");
        assert_eq!(t.words[1].start, 0.7);
        assert_eq!(t.corrections[0].audio_end, 1.2);
        assert_eq!(t.corrections[0].original_text, "Hello world");
        assert!(!t.reviewed);
        assert!(f
            .store
            .correct(&t.id, t.revision, 1, 2, "partial".into())
            .is_err());
    }
    #[test]
    fn revisions_serialize_concurrent_writers() {
        let f = Fixture::new();
        let t = f.store.create(&f.audio, &model()).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut jobs = vec![];
        for replacement in ["one", "two"] {
            let store = f.store.clone();
            let id = t.id.clone();
            let b = barrier.clone();
            jobs.push(std::thread::spawn(move || {
                b.wait();
                store.correct(&id, 1, 0, 1, replacement.into())
            }));
        }
        barrier.wait();
        let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(f.store.get(&t.id).unwrap().revision, 2);
    }
    #[test]
    fn validates_timestamps_and_ids() {
        let f = Fixture::new();
        for (start, end) in [
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
            (-1.0, 1.0),
            (2.0, 1.0),
        ] {
            let mut m = model();
            m.words[0].start = start;
            m.words[0].end = end;
            assert!(f.store.create(&f.audio, &m).is_err());
        }
        let mut m = model();
        m.words[1].start = 0.4;
        assert!(f.store.create(&f.audio, &m).is_err());
        assert!(f.store.get("../escape").is_err());
        assert!(f.store.audio_path("/tmp/other").is_err());
        assert!(f.store.list().unwrap().is_empty());
    }
    #[test]
    fn failed_import_does_not_publish_partial_project() {
        let f = Fixture::new();
        assert!(f
            .store
            .create(f.root.join("missing.wav"), &model())
            .is_err());
        assert!(f.store.list().unwrap().is_empty());
        assert!(!fs::read_dir(&f.store.root).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pending-")));
    }
    #[test]
    fn removal_and_restoration_keep_original_word_and_span() {
        let f = Fixture::new();
        let original = f.store.create(&f.audio, &model()).unwrap();
        let removed = f
            .store
            .correct(&original.id, original.revision, 0, 1, String::new())
            .unwrap();
        assert_eq!(removed.words[0].corrected, "");
        assert_eq!(removed.words[0].word, original.words[0].word);
        assert!(removed.corrections[0].human_checked);
        let restored = f
            .store
            .correct(
                &original.id,
                removed.revision,
                0,
                1,
                original.words[0].word.clone(),
            )
            .unwrap();
        assert_eq!(restored.words[0].start, original.words[0].start);
        assert_eq!(restored.words[0].end, original.words[0].end);
        assert_eq!(restored.corrections.len(), 2);
    }
    #[test]
    fn failed_save_preserves_last_committed_revision() {
        let f = Fixture::new();
        let t = f.store.create(&f.audio, &model()).unwrap();
        fs::create_dir(f.store.dir(&t.id).unwrap().join("transcript.next")).unwrap();
        assert!(f
            .store
            .correct(&t.id, t.revision, 0, 1, "changed".into())
            .is_err());
        let saved = f.store.get(&t.id).unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.text, "Hello, world!");
        assert!(saved.corrections.is_empty());
    }
}
