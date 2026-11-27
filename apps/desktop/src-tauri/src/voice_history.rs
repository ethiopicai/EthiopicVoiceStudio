//! Deletion is restricted to selected app-owned voice assets, under the shared voice lock.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    path::Path,
};
use tauri::Manager;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    CleanupRecording,
    CleanupResult,
    VoiceProject,
}
impl Kind {
    fn category(self) -> &'static str {
        match self {
            Self::CleanupRecording => "cleanup-recordings",
            Self::CleanupResult => "cleanup-results",
            Self::VoiceProject => "voice-edit/projects",
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub kind: Kind,
    pub id: String,
    pub revision: Option<u64>,
}
#[derive(Debug, Serialize)]
pub struct Key {
    pub kind: Kind,
    pub id: String,
}
#[derive(Debug, Serialize)]
pub struct Failure {
    pub kind: Kind,
    pub id: String,
    pub message: String,
}
#[derive(Default, Debug, Serialize)]
pub struct Deleted {
    pub deleted: Vec<Key>,
    pub failed: Vec<Failure>,
}

pub(crate) fn json(path: &Path) -> Result<Value, String> {
    if fs::symlink_metadata(path)
        .map_err(|_| "Saved metadata is unavailable")?
        .file_type()
        .is_symlink()
    {
        return Err("Redirected history metadata".into());
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|_| "Saved metadata is unavailable")?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Saved metadata is unavailable")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Saved metadata is too large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Saved metadata could not be checked".into())
}
fn core_metadata(path: &Path) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    let mut output = vec![];
    for name in [
        "project.json",
        "synthesis-context.json",
        "name.json",
        "result.json",
        "card-title.json",
        "speech-group.json",
    ] {
        let file = match fs::File::open(path.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("Saved metadata could not be checked".into()),
        };
        let mut bytes = vec![];
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Saved metadata could not be checked")?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Saved metadata is too large".into());
        }
        output.push((name, bytes));
    }
    Ok(output)
}
fn restore_metadata(root: &Path, item: &Item, backup: &[(&str, Vec<u8>)]) {
    let Ok(path) = crate::asr::managed_deletion_dir(root, item.kind.category(), &item.id) else {
        return;
    };
    if !path.is_dir() {
        return;
    }
    for (name, bytes) in backup {
        if let Ok(mut file) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join(name))
        {
            let _ = file.write_all(bytes);
        }
    }
}
fn delete_with(
    root: &Path,
    items: &[Item],
    mut remove: impl FnMut(&Path, &str, &str) -> Result<(), String>,
) -> Result<Deleted, String> {
    if items.len() > 5000 {
        return Err("Select no more than 5,000 recordings at a time.".into());
    }
    let mut seen = HashSet::new();
    for item in items {
        if !seen.insert((item.kind, item.id.as_str())) {
            return Err("Refresh history and select each recording once.".into());
        }
        let path = crate::asr::managed_deletion_dir(root, item.kind.category(), &item.id)?;
        if item.kind == Kind::VoiceProject {
            let revision = item
                .revision
                .ok_or("Refresh editing history before deleting this project.")?;
            if path.exists() {
                let project = json(&path.join("project.json"))?;
                crate::speech_versions::members(root, &item.id)?;
                if project["id"].as_str() != Some(&item.id)
                    || project["revision"].as_u64() != Some(revision)
                {
                    return Err(
                        "A selected project changed. Refresh history before deleting.".into(),
                    );
                }
            }
        }
    }
    // The caller holds the recognition lock. Remove abandoned worker output as
    // well as saved cleanup assets; no live cleanup can be using these folders.
    if items
        .iter()
        .any(|item| matches!(item.kind, Kind::CleanupRecording | Kind::CleanupResult))
    {
        crate::asr::managed_deletion_dir(root, "cleanup-jobs", "history-preflight")?;
        let jobs = root.join("cleanup-jobs");
        let mut stale = vec![];
        if jobs.exists() {
            for entry in fs::read_dir(jobs).map_err(|_| "Temporary cleanup files unavailable")? {
                let entry = entry.map_err(|_| "Temporary cleanup files unavailable")?;
                let id = entry.file_name().to_string_lossy().into_owned();
                crate::asr::managed_deletion_dir(root, "cleanup-jobs", &id)?;
                if entry
                    .file_type()
                    .map_err(|_| "Temporary cleanup files unavailable")?
                    .is_dir()
                {
                    stale.push(id);
                }
            }
        }
        for id in stale {
            crate::asr::remove_managed_dir(root, "cleanup-jobs", &id)?;
        }
    }
    let mut result = Deleted::default();
    for item in items {
        // Retain only listing/revision metadata if an OS error interrupts file
        // removal (for example a locked WAV on Windows), so retry stays possible.
        let path = root.join(item.kind.category()).join(&item.id);
        let backup = core_metadata(&path);
        let removal = match &backup {
            Ok(_) => (|| {
                if item.kind == Kind::VoiceProject {
                    for id in crate::speech_versions::members(root, &item.id)? {
                        let child = Item {
                            kind: Kind::VoiceProject,
                            id,
                            revision: None,
                        };
                        let path = root.join(item.kind.category()).join(&child.id);
                        let backup = core_metadata(&path)?;
                        if let Err(error) = remove(root, item.kind.category(), &child.id) {
                            restore_metadata(root, &child, &backup);
                            return Err(error);
                        }
                    }
                }
                remove(root, item.kind.category(), &item.id)
            })(),
            Err(error) => Err(error.clone()),
        };
        match removal {
            Ok(()) => result.deleted.push(Key {
                kind: item.kind,
                id: item.id.clone(),
            }),
            Err(_) => {
                if let Ok(backup) = backup {
                    restore_metadata(root, item, &backup);
                }
                result.failed.push(Failure{kind:item.kind,id:item.id.clone(),message:"Some app-owned files could not be removed. Close playback and retry. Original files and exports are unchanged.".into()})
            }
        }
    }
    Ok(result)
}
#[tauri::command]
pub async fn delete_voice_history(
    app: tauri::AppHandle,
    items: Vec<Item>,
) -> Result<Deleted, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|_| "Voice history is unavailable")?;
        app.state::<crate::voice_editing::EditingState>()
            .change_history(|| {
                let result = delete_with(&root, &items, crate::asr::remove_managed_dir)?;
                let recordings = result
                    .deleted
                    .iter()
                    .filter(|item| item.kind == Kind::CleanupRecording)
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>();
                let results = result
                    .deleted
                    .iter()
                    .filter(|item| item.kind == Kind::CleanupResult)
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>();
                crate::cleanup::forget_history(
                    &app.state::<crate::cleanup::CleanupState>(),
                    &recordings,
                    &results,
                );
                Ok(result)
            })
    })
    .await
    .map_err(|_| "Voice history deletion was interrupted. Refresh and retry.")?
}
#[derive(Serialize)]
pub struct Generation {
    id: String,
    name: String,
    text: String,
    revision: u64,
    created: f64,
    settings: Option<Value>,
    #[serde(rename = "versionCount")]
    version_count: usize,
    #[serde(rename = "currentVersion")]
    current_version: String,
}
fn generations(root: &Path) -> Result<Vec<Generation>, String> {
    let parent = root.join("voice-edit/projects");
    if !parent.exists() {
        return Ok(vec![]);
    }
    // Read each version once. History refresh must not reread every WAV or scan
    // all of the other cards again for each group.
    let mut groups: BTreeMap<String, Vec<(String, Value, Value)>> = BTreeMap::new();
    for entry in fs::read_dir(parent).map_err(|_| "Generated speech history unavailable")? {
        let entry = entry.map_err(|_| "Generated speech history unavailable")?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.starts_with('.') || !entry.path().join("synthesis-context.json").exists() {
            continue;
        }
        let path = crate::asr::managed_deletion_dir(root, "voice-edit/projects", &id)?;
        let project = json(&path.join("project.json"))?;
        let synthesis = json(&path.join("synthesis-context.json"))?;
        let group = synthesis["groupId"].as_str().unwrap_or(&id).to_owned();
        crate::asr::managed_deletion_dir(root, "voice-edit/projects", &group)?;
        if project["id"].as_str() != Some(&id)
            || synthesis["projectId"]
                .as_str()
                .is_some_and(|value| value != id)
        {
            return Err("Speech history needs repair".into());
        }
        groups
            .entry(group)
            .or_default()
            .push((id, project, synthesis));
    }
    let mut output = vec![];
    for (id, mut versions) in groups {
        let project = versions
            .iter()
            .find(|v| v.0 == id)
            .ok_or("Speech card metadata unavailable")?
            .1
            .clone();
        versions.sort_by(|a, b| {
            a.1["created"]
                .as_f64()
                .unwrap_or_default()
                .total_cmp(&b.1["created"].as_f64().unwrap_or_default())
                .then_with(|| a.0.cmp(&b.0))
        });
        let (latest, _, synthesis) = versions.last().ok_or("Speech version unavailable")?;
        output.push(Generation {
            id,
            name: project["name"]
                .as_str()
                .unwrap_or("Generated speech.wav")
                .into(),
            text: synthesis["text"].as_str().unwrap_or_default().into(),
            revision: project["revision"]
                .as_u64()
                .ok_or("Invalid saved revision")?,
            created: project["created"].as_f64().unwrap_or_default(),
            settings: synthesis.get("settings").filter(|v| !v.is_null()).cloned(),
            version_count: versions.len(),
            current_version: latest.clone(),
        });
    }
    output.sort_by(|a, b| b.created.total_cmp(&a.created));
    Ok(output)
}
#[tauri::command]
pub async fn list_voice_generations(app: tauri::AppHandle) -> Result<Vec<Generation>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        generations(
            &app.path()
                .app_data_dir()
                .map_err(|_| "Voice history unavailable")?,
        )
    })
    .await
    .map_err(|_| "Generated speech history unavailable")?
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Files(std::path::PathBuf);
    impl Files {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "voice-history-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn item(&self, kind: Kind, id: &str) -> Item {
            let dir = self.0.join(kind.category()).join(id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("audio.wav"), b"owned audio").unwrap();
            fs::write(dir.join("source-media.json"), b"owned metadata").unwrap();
            if kind == Kind::VoiceProject {
                fs::write(
                    dir.join("project.json"),
                    format!(
                        "{{\"id\":\"{id}\",\"name\":\"Voice.wav\",\"revision\":0,\"created\":1}}"
                    ),
                )
                .unwrap();
            }
            Item {
                kind,
                id: id.into(),
                revision: (kind == Kind::VoiceProject).then_some(0),
            }
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn deletion_removes_only_selected_owned_audio_metadata_and_edit_history() {
        let f = Files::new();
        let items = [
            f.item(Kind::CleanupRecording, "input"),
            f.item(Kind::CleanupResult, "cleaned"),
            f.item(Kind::VoiceProject, "edited"),
        ];
        fs::create_dir_all(f.0.join("cleanup-jobs/old-job")).unwrap();
        fs::write(
            f.0.join("cleanup-jobs/old-job/cleaned.wav"),
            b"temporary copy",
        )
        .unwrap();
        let kept = f.item(Kind::VoiceProject, "independent-copy");
        fs::write(f.0.join("original.wav"), b"external original").unwrap();
        fs::write(f.0.join("export.wav"), b"user export").unwrap();
        let result = delete_with(&f.0, &items, crate::asr::remove_managed_dir).unwrap();
        assert_eq!(result.deleted.len(), 3);
        assert!(result.failed.is_empty());
        assert!(!f.0.join("cleanup-jobs/old-job").exists());
        for item in items {
            assert!(!f.0.join(item.kind.category()).join(item.id).exists())
        }
        assert!(f.0.join(kept.kind.category()).join(kept.id).exists());
        assert_eq!(
            fs::read(f.0.join("original.wav")).unwrap(),
            b"external original"
        );
        assert_eq!(fs::read(f.0.join("export.wav")).unwrap(), b"user export");
    }
    #[test]
    fn stale_revision_or_invalid_selection_prevents_all_deletions() {
        let f = Files::new();
        let first = f.item(Kind::CleanupResult, "first");
        let mut stale = f.item(Kind::VoiceProject, "changed");
        stale.revision = Some(1);
        assert!(delete_with(
            &f.0,
            &[first.clone(), stale],
            crate::asr::remove_managed_dir
        )
        .is_err());
        assert!(f.0.join("cleanup-results/first").exists());
        assert!(delete_with(
            &f.0,
            &[first.clone(), first.clone()],
            crate::asr::remove_managed_dir
        )
        .is_err());
        assert!(delete_with(
            &f.0,
            &[Item {
                kind: Kind::CleanupResult,
                id: "../original".into(),
                revision: None
            }],
            crate::asr::remove_managed_dir
        )
        .is_err());
    }
    #[test]
    fn partial_failure_is_returned_for_retry_and_missing_items_are_idempotent() {
        let f = Files::new();
        let first = f.item(Kind::CleanupRecording, "first");
        let second = f.item(Kind::CleanupResult, "second");
        let result = delete_with(
            &f.0,
            &[first.clone(), second.clone()],
            |root, category, id| {
                if id == "second" {
                    Err("locked".into())
                } else {
                    crate::asr::remove_managed_dir(root, category, id)
                }
            },
        )
        .unwrap();
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.failed[0].id, "second");
        let retry = delete_with(&f.0, &[first, second], crate::asr::remove_managed_dir).unwrap();
        assert_eq!(retry.deleted.len(), 2);
    }
    #[test]
    fn interrupted_file_removal_restores_revision_metadata_for_retry() {
        let f = Files::new();
        let item = f.item(Kind::VoiceProject, "retry-project");
        let path = f.0.join("voice-edit/projects/retry-project");
        fs::write(
            path.join("synthesis-context.json"),
            b"{\"text\":\"saved take\"}",
        )
        .unwrap();
        let result = delete_with(&f.0, std::slice::from_ref(&item), |_, _, _| {
            fs::remove_file(path.join("project.json")).unwrap();
            fs::remove_file(path.join("synthesis-context.json")).unwrap();
            Err("locked audio".into())
        })
        .unwrap();
        assert_eq!(result.failed.len(), 1);
        assert_eq!(generations(&f.0).unwrap().len(), 1);
        assert_eq!(
            delete_with(&f.0, &[item], crate::asr::remove_managed_dir)
                .unwrap()
                .deleted
                .len(),
            1
        );
    }
    #[test]
    fn generated_history_is_reopenable_and_deletion_includes_its_reference_and_output() {
        let f = Files::new();
        let item = f.item(Kind::VoiceProject, "generated");
        f.item(Kind::VoiceProject, "ordinary-edit");
        let dir = f.0.join("voice-edit/projects/generated");
        fs::write(dir.join("synthesis.wav"), b"generated speech").unwrap();
        fs::write(
            dir.join("synthesis-context.json"),
            b"{\"text\":\"test script\"}",
        )
        .unwrap();
        let history = generations(&f.0).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "test script");
        assert!(history[0].settings.is_none());
        let settings =
            serde_json::json!({"schemaVersion":1,"temperature":0.4,"sentencePause":0.5,"seed":42});
        fs::write(
            dir.join("synthesis-context.json"),
            serde_json::to_vec(&serde_json::json!({"text":"test script","settings":settings}))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(generations(&f.0).unwrap()[0].settings, Some(settings));
        delete_with(&f.0, &[item], crate::asr::remove_managed_dir).unwrap();
        assert!(generations(&f.0).unwrap().is_empty());
        assert!(!dir.exists());
    }
    #[test]
    fn speech_versions_list_as_one_card_and_group_deletion_can_retry_a_locked_version() {
        let f = Files::new();
        let item = f.item(Kind::VoiceProject, "speech");
        f.item(Kind::VoiceProject, "speech-version");
        f.item(Kind::VoiceProject, "unrelated");
        for (id, created, text) in [
            ("speech", 1, "original script"),
            ("speech-version", 2, "changed script"),
        ] {
            let dir = f.0.join("voice-edit/projects").join(id);
            let mut project = json(&dir.join("project.json")).unwrap();
            project["created"] = created.into();
            fs::write(
                dir.join("project.json"),
                serde_json::to_vec(&project).unwrap(),
            )
            .unwrap();
            fs::write(dir.join("speech-group.json"), br#"{"groupId":"speech"}"#).unwrap();
            fs::write(
                dir.join("synthesis-context.json"),
                serde_json::to_vec(
                    &serde_json::json!({"groupId":"speech","projectId":id,"text":text}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        let history = generations(&f.0).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "speech");
        assert_eq!(history[0].version_count, 2);
        assert_eq!(history[0].text, "changed script");
        assert_eq!(history[0].current_version, "speech-version");
        let child = Item {
            kind: Kind::VoiceProject,
            id: "speech-version".into(),
            revision: item.revision,
        };
        assert!(delete_with(&f.0, &[child], crate::asr::remove_managed_dir).is_err());
        let result = delete_with(&f.0, std::slice::from_ref(&item), |root, category, id| {
            if id == "speech-version" {
                return Err("locked".into());
            }
            crate::asr::remove_managed_dir(root, category, id)
        })
        .unwrap();
        assert_eq!(result.failed.len(), 1);
        assert_eq!(generations(&f.0).unwrap()[0].version_count, 2);
        let result = delete_with(&f.0, &[item], crate::asr::remove_managed_dir).unwrap();
        assert_eq!(result.deleted.len(), 1);
        assert!(!f.0.join("voice-edit/projects/speech").exists());
        assert!(!f.0.join("voice-edit/projects/speech-version").exists());
        assert!(f.0.join("voice-edit/projects/unrelated").exists());
    }
    #[cfg(unix)]
    #[test]
    fn redirected_storage_or_child_symlink_is_rejected_before_any_mutation() {
        use std::os::unix::fs::symlink;
        let f = Files::new();
        let first = f.item(Kind::CleanupResult, "first");
        let second = f.item(Kind::VoiceProject, "second");
        fs::write(f.0.join("outside.wav"), b"original").unwrap();
        symlink(
            f.0.join("outside.wav"),
            f.0.join("voice-edit/projects/second/link.wav"),
        )
        .unwrap();
        assert!(delete_with(&f.0, &[first, second], crate::asr::remove_managed_dir).is_err());
        assert!(f.0.join("cleanup-results/first").exists());
        assert_eq!(fs::read(f.0.join("outside.wav")).unwrap(), b"original");
    }
}
