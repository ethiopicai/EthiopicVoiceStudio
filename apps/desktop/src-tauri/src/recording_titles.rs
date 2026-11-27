//! Display-only titles. Audio filenames and model/project schemas stay unchanged.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::Manager;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const FILE: &str = "card-title.json";
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Transcript,
    Recording,
    CleanupRecording,
    CleanupResult,
    VoiceProject,
}
impl Kind {
    fn location(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Transcript => ("transcript", "transcripts", "transcript.json"),
            Self::Recording => ("recording", "recordings", "name.json"),
            Self::CleanupRecording => ("cleanup-recording", "cleanup-recordings", "name.json"),
            Self::CleanupResult => ("cleanup-result", "cleanup-results", "result.json"),
            Self::VoiceProject => ("voice-project", "voice-edit/projects", "project.json"),
        }
    }
    fn key(self, id: &str) -> String {
        format!("{}:{id}", self.location().0)
    }
}
fn validate(title: &str) -> Result<String, String> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 || title.chars().any(char::is_control) {
        return Err("Use a title from 1 to 120 characters on one line.".into());
    }
    Ok(trimmed.into())
}
fn directory(root: &Path, kind: Kind, id: &str) -> Result<PathBuf, String> {
    if root.is_symlink() {
        return Err("Redirected recording storage".into());
    }
    let (_, category, marker) = kind.location();
    let path = crate::asr::managed_deletion_dir(root, category, id)?;
    if !path.join(marker).is_file() {
        return Err("This recording is no longer available. Refresh history.".into());
    }
    Ok(path)
}
fn read(path: &Path) -> Result<Option<String>, String> {
    let file = path.join(FILE);
    if file.is_symlink() {
        return Err("Redirected recording title".into());
    }
    let stream = match fs::File::open(&file) {
        Ok(stream) => stream,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Recording title is unavailable".into()),
    };
    let mut bytes = vec![];
    stream
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "Recording title is unavailable")?;
    if bytes.len() > 4096 {
        return Err("Recording title is invalid".into());
    }
    let title: String = serde_json::from_slice(&bytes).map_err(|_| "Recording title is invalid")?;
    validate(&title).map(Some)
}
fn save(
    root: &Path,
    kind: Kind,
    id: &str,
    title: &str,
    expected: Option<&str>,
) -> Result<String, String> {
    let title = validate(title)?;
    let path = directory(root, kind, id)?;
    if read(&path)?.as_deref() != expected {
        return Err("This title changed elsewhere. Review it and save again.".into());
    }
    let pending = path.join(format!(
        ".title-{}-{}.pending",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "Could not save this title. Check available storage.")?;
        file.write_all(&serde_json::to_vec(&title).map_err(|_| "Invalid title")?)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Could not save this title")?;
        drop(file);
        transcripts::replace_metadata(&pending, &path.join(FILE))
            .map_err(|_| "Could not save this title")?;
        #[cfg(unix)]
        fs::File::open(&path)
            .and_then(|f| f.sync_all())
            .map_err(|_| "Could not finish saving this title")?;
        Ok(title)
    })();
    if result.is_err() {
        let _ = fs::remove_file(pending);
    }
    result
}
fn list(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut titles = BTreeMap::new();
    if !root.exists() {
        return Ok(titles);
    }
    for kind in [
        Kind::Transcript,
        Kind::Recording,
        Kind::CleanupRecording,
        Kind::CleanupResult,
        Kind::VoiceProject,
    ] {
        let base = root.join(kind.location().1);
        if !base.exists() {
            continue;
        }
        if base.is_symlink() || root.is_symlink() {
            return Err("Redirected recording storage".into());
        }
        for entry in fs::read_dir(&base)
            .map_err(|_| "Recording titles unavailable")?
            .take(20000)
        {
            let entry = entry.map_err(|_| "Recording titles unavailable")?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !entry.path().join(FILE).exists() {
                continue;
            }
            // Invalid/staging/missing/corrupt titles never prevent opening recordings.
            if let Ok(path) = directory(root, kind, &id) {
                if let Ok(Some(title)) = read(&path) {
                    titles.insert(kind.key(&id), title);
                }
            }
        }
    }
    Ok(titles)
}
#[tauri::command]
pub async fn list_recording_titles(
    app: tauri::AppHandle,
) -> Result<BTreeMap<String, String>, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Recording storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || list(&root))
        .await
        .map_err(|_| "Recording titles unavailable")?
}
#[tauri::command]
pub async fn rename_recording(
    app: tauri::AppHandle,
    kind: Kind,
    id: String,
    title: String,
    expected_title: Option<String>,
) -> Result<String, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Recording storage unavailable")?;
    tauri::async_runtime::spawn_blocking(move || {
        if root.is_symlink() {
            return Err("Redirected recording storage".into());
        }
        let lock_path = root.join("recognition.lock");
        if lock_path.is_symlink() {
            return Err("Redirected recording storage".into());
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "Recording storage unavailable")?;
        lock.try_lock_exclusive().map_err(|_| {
            "Finish or cancel the current voice task before renaming this recording."
        })?;
        save(&root, kind, &id, &title, expected_title.as_deref())
    })
    .await
    .map_err(|_| "Renaming was interrupted. Retry saving the title.")?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn titles_survive_reload_without_touching_audio_or_metadata_and_are_deleted_with_the_asset() {
        let root = std::env::temp_dir().join(format!(
            "ethiopic-titles-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        for kind in [
            Kind::Transcript,
            Kind::Recording,
            Kind::CleanupRecording,
            Kind::CleanupResult,
            Kind::VoiceProject,
        ] {
            let (_, category, marker) = kind.location();
            let path = root.join(category).join("first");
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join(marker), b"original metadata").unwrap();
            fs::write(path.join("original.wav"), b"unchanged PCM").unwrap();
            assert_eq!(
                save(&root, kind, "first", "  የፖድካስት መግቢያ  ", None).unwrap(),
                "የፖድካስት መግቢያ"
            );
            assert_eq!(
                list(&root).unwrap().get(&kind.key("first")).unwrap(),
                "የፖድካስት መግቢያ"
            );
            assert!(save(&root, kind, "first", "Stale edit", None).is_err());
            save(&root, kind, "first", "Episode 2", Some("የፖድካስት መግቢያ")).unwrap();
            assert_eq!(fs::read(path.join(marker)).unwrap(), b"original metadata");
            assert_eq!(
                fs::read(path.join("original.wav")).unwrap(),
                b"unchanged PCM"
            );
            crate::asr::remove_managed_dir(&root, category, "first").unwrap();
            assert!(!list(&root).unwrap().contains_key(&kind.key("first")));
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn validates_titles_and_refuses_missing_or_redirected_assets() {
        for title in [" ", "\nName", "a\0b", &"ሀ".repeat(121)] {
            assert!(validate(title).is_err());
        }
        assert!(validate(&"ሀ".repeat(120)).is_ok());
        assert!(validate("My / podcast <intro>").is_ok());
        let root = std::env::temp_dir().join(format!(
            "ethiopic-titles-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        assert!(save(&root, Kind::Transcript, "../outside", "Name", None).is_err());
        assert!(save(&root, Kind::Recording, "missing", "Name", None).is_err());
        #[cfg(unix)]
        {
            let path = root.join("recordings").join("linked");
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("name.json"), b"name").unwrap();
            fs::write(root.join("original"), b"external").unwrap();
            std::os::unix::fs::symlink(root.join("original"), path.join(FILE)).unwrap();
            assert!(save(&root, Kind::Recording, "linked", "Name", None).is_err());
            assert_eq!(fs::read(root.join("original")).unwrap(), b"external");
        }
        fs::remove_dir_all(root).unwrap();
    }
}
