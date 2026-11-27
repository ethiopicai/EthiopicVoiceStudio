//! Each speech render is an immutable project, grouped by its first render's ID.
use serde_json::Value;
use std::{fs, path::Path};
use tauri::Manager;

pub(crate) fn versions(root: &Path, group: &str) -> Result<Vec<Value>, String> {
    let path = crate::asr::managed_deletion_dir(root, "voice-edit/projects", group)?;
    let first = crate::voice_history::json(&path.join("synthesis-context.json"))?;
    if first["groupId"].as_str().is_some_and(|id| id != group) {
        return Err("Open the parent speech card to manage its versions.".into());
    }
    let mut output = vec![];
    for entry in
        fs::read_dir(root.join("voice-edit/projects")).map_err(|_| "Speech versions unavailable")?
    {
        let entry = entry.map_err(|_| "Speech versions unavailable")?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if id.starts_with('.') || !entry.path().join("synthesis-context.json").exists() {
            continue;
        }
        let path = crate::asr::managed_deletion_dir(root, "voice-edit/projects", &id)?;
        let mut context = crate::voice_history::json(&path.join("synthesis-context.json"))?;
        if id != group && context["groupId"].as_str() != Some(group) {
            continue;
        }
        // Older takes did not always record their project ID or settings.
        if id == group && context.get("projectId").is_none() {
            context["projectId"] = Value::String(id.clone());
        }
        if context["projectId"].as_str() != Some(&id) {
            return Err("Speech version metadata needs repair.".into());
        }
        let project = crate::voice_history::json(&path.join("project.json"))?;
        context["created"] = project["created"].clone();
        output.push(context);
    }
    output.sort_by(|a, b| {
        a["created"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&b["created"].as_f64().unwrap_or_default())
            .then_with(|| a["projectId"].as_str().cmp(&b["projectId"].as_str()))
    });
    Ok(output)
}

/// Includes incomplete renders, so deleting a card removes every owned snapshot.
pub(crate) fn members(root: &Path, group: &str) -> Result<Vec<String>, String> {
    let path = crate::asr::managed_deletion_dir(root, "voice-edit/projects", group)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    if path.join("speech-group.json").exists()
        && crate::voice_history::json(&path.join("speech-group.json"))?["groupId"].as_str()
            != Some(group)
    {
        return Err("Delete the parent speech card instead.".into());
    }
    let mut ids = vec![];
    for entry in
        fs::read_dir(root.join("voice-edit/projects")).map_err(|_| "Speech history unavailable")?
    {
        let entry = entry.map_err(|_| "Speech history unavailable")?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if id == group || !entry.path().join("speech-group.json").exists() {
            continue;
        }
        let path = crate::asr::managed_deletion_dir(root, "voice-edit/projects", &id)?;
        if crate::voice_history::json(&path.join("speech-group.json"))?["groupId"].as_str()
            == Some(group)
        {
            ids.push(id)
        }
    }
    Ok(ids)
}

#[tauri::command]
pub async fn get_speech_versions(
    app: tauri::AppHandle,
    group_id: String,
) -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        versions(
            &app.path()
                .app_data_dir()
                .map_err(|_| "Speech storage unavailable")?,
            &group_id,
        )
    })
    .await
    .map_err(|_| "Speech history interrupted")?
}
