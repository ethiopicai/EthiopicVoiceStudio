//! Read-only download quotes. A setup command must match the quote the user approved.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub purpose: String,
    pub bytes: u64,
    pub url: String,
    pub sha256: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub available: bool,
    pub reason: String,
    pub items: Vec<Item>,
    pub download_bytes: u64,
}
impl Plan {
    pub fn new(
        kind: &str,
        title: &str,
        available: bool,
        reason: String,
        items: Vec<Item>,
    ) -> Result<Self, String> {
        // Shared dependencies count once; conflicting catalog entries fail closed.
        let mut unique = BTreeMap::new();
        for item in items {
            if item.bytes == 0
                || item.sha256.len() != 64
                || !item.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !item.url.starts_with("https://")
            {
                return Err("Download details are incomplete. Check for an app update.".into());
            }
            if let Some(prior) = unique.insert(item.url.clone(), item.clone()) {
                if prior.bytes != item.bytes || prior.sha256 != item.sha256 {
                    return Err("Conflicting download details. Check for an app update.".into());
                }
            }
        }
        let items: Vec<Item> = unique.into_values().collect();
        if available && items.is_empty() && kind != "voice-tools" {
            return Err("Download details are incomplete. Check for an app update.".into());
        }
        let download_bytes = items
            .iter()
            .try_fold(0u64, |total, item| total.checked_add(item.bytes))
            .ok_or("Download size is too large")?;
        let data = serde_json::to_vec(&(kind, title, available, &reason, &items))
            .map_err(|_| "Download details unavailable")?;
        Ok(Self {
            id: format!("{:x}", Sha256::digest(data)),
            kind: kind.into(),
            title: title.into(),
            available,
            reason,
            items,
            download_bytes,
        })
    }
    pub fn approve(&self, id: &str) -> Result<(), String> {
        if !self.available {
            return Err(self.reason.clone());
        }
        if self.id != id {
            return Err(
                "The download details changed. Review the new files and size before downloading."
                    .into(),
            );
        }
        Ok(())
    }
}
pub(crate) fn model_items(
    m: &resource_packs::studio::Manifest,
    ids: &[String],
) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for id in ids {
        let variant = m.find(id).ok_or("A selected model is missing")?;
        for file in &variant.files {
            if resource_packs::naming::bundled_file(m, &variant.id, &file.path).is_some() {
                continue;
            }
            items.push(Item {
                name: format!("{}/{}", variant.folder, file.path),
                purpose: match variant.component.as_str() {
                    "asr" => "Amharic speech recognition model",
                    "tts" => "Speech generation model",
                    "tts-codec" => "Audio encoding and decoding tools",
                    "corrector" => "Transcript correction model",
                    "edit" => "Voice Editing support files",
                    _ => "Voice model and supporting files",
                }
                .into(),
                bytes: file.bytes,
                url: m.file_url(id, &file.path)?,
                sha256: file.sha256.clone(),
            });
        }
    }
    Ok(items)
}
pub(crate) fn quote(app: &tauri::AppHandle, kind: &str) -> Result<Plan, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Studio storage is unavailable")?;
    match kind {
        "voice-tools" => crate::voice_setup::download_quote(app),
        "recognition" => crate::resource_setup::download_quote(&root),
        "speech" | "editing" => crate::voice_editing::download_quote(
            &app.state::<crate::voice_editing::EditingState>(),
            kind,
        ),
        "noise" => crate::noise_reduction::download_quote(&root),
        _ => Err("Unknown voice tool download".into()),
    }
}
#[tauri::command]
pub async fn get_asset_download_plan(app: tauri::AppHandle, kind: String) -> Result<Plan, String> {
    tauri::async_runtime::spawn_blocking(move || quote(&app, &kind))
        .await
        .map_err(|_| "Download details could not be checked")?
}
pub(crate) fn require(app: &tauri::AppHandle, kind: &str, id: &str) -> Result<(), String> {
    quote(app, kind)?.approve(id)
}
fn needs_introduction(root: &Path) -> bool {
    !root.join("setup-introduction.json").is_file()
        && ![
            "asr/active",
            "tts-runtime/active",
            "studio-runtime/active",
            "voice-edit/models",
        ]
        .iter()
        .any(|path| root.join(path).is_dir())
}
#[tauri::command]
pub fn get_setup_introduction(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(needs_introduction(
        &app.path()
            .app_data_dir()
            .map_err(|_| "Studio storage unavailable")?,
    ))
}
#[tauri::command]
pub fn acknowledge_setup_introduction(app: tauri::AppHandle) -> Result<(), String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Studio storage unavailable")?;
    fs::create_dir_all(&root).map_err(|_| "Could not save setup choice")?;
    fs::write(root.join("setup-introduction.json"), b"{\"seen\":true}")
        .map_err(|_| "Could not save setup choice".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn item(bytes: u64) -> Item {
        Item {
            name: "model.bin".into(),
            purpose: "Voice model".into(),
            bytes,
            url: "https://huggingface.co/ethiopic/models/resolve/pin/model.bin".into(),
            sha256: "a".repeat(64),
        }
    }
    #[test]
    fn size_is_exact_shared_files_count_once_and_approval_is_bound_to_assets() {
        let plan = Plan::new(
            "speech",
            "Speech",
            true,
            "".into(),
            vec![item(200), item(200)],
        )
        .unwrap();
        assert_eq!(plan.download_bytes, 200);
        assert_eq!(plan.items.len(), 1);
        plan.approve(&plan.id).unwrap();
        let changed = Plan::new("speech", "Speech", true, "".into(), vec![item(201)]).unwrap();
        assert!(changed.approve(&plan.id).unwrap_err().contains("changed"));
        assert!(
            Plan::new("editing", "Editing", true, "".into(), vec![item(200)])
                .unwrap()
                .approve(&plan.id)
                .is_err()
        );
    }
    #[test]
    fn unavailable_and_incomplete_downloads_cannot_be_approved() {
        let unavailable = Plan::new(
            "speech",
            "Speech",
            false,
            "Runtime not released".into(),
            vec![],
        )
        .unwrap();
        assert!(unavailable.approve(&unavailable.id).is_err());
        assert!(Plan::new("speech", "Speech", true, "".into(), vec![item(0)]).is_err());
        assert!(Plan::new("speech", "Speech", true, "".into(), vec![item(1), item(2)]).is_err());
        assert!(Plan::new("speech", "Speech", true, "".into(), vec![]).is_err());
    }
    #[test]
    fn first_run_intro_is_separate_from_permission_and_existing_resources_are_kept() {
        let root = std::env::temp_dir().join(format!(
            "studio-setup-intro-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(needs_introduction(&root));
        fs::create_dir_all(root.join("asr/active")).unwrap();
        assert!(!needs_introduction(&root));
        fs::remove_dir_all(root.join("asr")).unwrap();
        fs::write(root.join("setup-introduction.json"), b"{\"seen\":true}").unwrap();
        assert!(!needs_introduction(&root));
        let plan = Plan::new("speech", "Speech", true, "".into(), vec![item(100)]).unwrap();
        assert!(plan.approve("").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
