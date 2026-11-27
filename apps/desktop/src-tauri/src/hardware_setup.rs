//! Read-only setup inspection and authoritative pre-download hardware/storage validation.
use resource_packs::{
    routing::{self, RoutingPlan},
    studio::{Hardware, Manifest},
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::Manager;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feature {
    id: &'static str,
    title: &'static str,
    model: Option<String>,
    downloads: Vec<String>,
    model_bytes: u64,
    ready: bool,
    can_install: bool,
    message: String,
    active_version: Option<String>,
    requires_consent: bool,
    required_storage_bytes: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupPlan {
    supported: bool,
    reason: String,
    hardware: Hardware,
    os_version: Option<String>,
    routing: RoutingPlan,
    free_storage_bytes: u64,
    features: Vec<Feature>,
}
pub(crate) fn platform_check(h: &Hardware, version: Option<&str>) -> Result<(), String> {
    let numbers: Vec<u32> = version
        .unwrap_or("")
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect();
    let major = numbers
        .first()
        .copied()
        .ok_or("Could not verify the operating system version. Check hardware again.")?;
    match (h.os.as_str(), h.arch.as_str()) {
        ("macos", "aarch64" | "arm64") if major >= 14 => {}
        ("windows", "x86_64")
            if major >= 11
                || (major == 10 && numbers.last().is_some_and(|build| *build >= 19041)) => {}
        ("windows", _) => return Err("Voice setup needs 64-bit Windows 10 or later.".into()),
        ("macos", _) => {
            return Err("Voice setup needs an Apple Silicon Mac with macOS 14 or later.".into())
        }
        _ => {
            return Err(
                "Voice setup supports Apple Silicon Macs and 64-bit Windows computers.".into(),
            )
        }
    }
    if h.total_ram_bytes == 0 {
        return Err("Could not read this computer's memory. Check hardware again.".into());
    }
    Ok(())
}
fn free_storage(root: &Path) -> Result<u64, String> {
    let directory = root
        .ancestors()
        .find(|p| p.is_dir())
        .ok_or("Storage location is unavailable")?;
    fs2::available_space(directory)
        .map_err(|_| "Could not check available storage. Try again.".into())
}
fn model_file(root: &Path, revision: &str, folder: &str, path: &str) -> PathBuf {
    root.join("voice-edit/models")
        .join(revision)
        .join(folder)
        .join(path)
}
/// Cheap size estimates only. Installers perform SHA-256 verification before reuse/use.
pub(crate) fn missing_model_bytes(root: &Path, m: &Manifest, ids: &[String]) -> u64 {
    let revision = m.hf_revision.as_deref().unwrap_or("");
    ids.iter()
        .filter_map(|id| m.find(id))
        .flat_map(|v| v.files.iter().map(move |f| (v, f)))
        .filter(|(v, f)| {
            let path = model_file(root, revision, &v.folder, &f.path);
            !std::fs::symlink_metadata(path).is_ok_and(|meta| {
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() == f.bytes
            })
        })
        .map(|(_, f)| f.bytes)
        .sum()
}
pub(crate) fn storage_required(missing: u64) -> u64 {
    if missing == 0 {
        0
    } else {
        missing.saturating_mul(2).saturating_add(256 * 1024 * 1024)
    }
}
pub(crate) fn validate_download(
    root: &Path,
    m: &Manifest,
    h: &Hardware,
    ids: &[String],
    runtime_bytes: u64,
) -> Result<(), String> {
    platform_check(h, sysinfo::System::os_version().as_deref())?;
    if ids.is_empty() {
        return Err("No compatible model was selected for this computer.".into());
    }
    let missing = missing_model_bytes(root, m, ids);
    let required = storage_required(missing).saturating_add(runtime_bytes);
    if free_storage(root)? < required {
        return Err(format!("Free at least {:.1} GB of storage before downloading these tools. Saved recordings are unchanged.",required as f64/1e9));
    }
    Ok(())
}
fn inspect_setup(app: tauri::AppHandle) -> Result<SetupPlan, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "Studio storage is unavailable")?;
    let h = crate::hardware::inspect();
    let version = sysinfo::System::os_version();
    let compatible = platform_check(&h, version.as_deref());
    let supported = compatible.is_ok();
    let m = crate::voice_editing::setup_manifest(&root)?;
    let mut routing = routing::route(&m, &h, false, false)?;
    if crate::voice_editing::small_tts_selected(&m, &h)
        && routing.speech.variant.as_deref() != Some("tts-flow_lm-90m-amh-fp32")
    {
        let mut cpu = h.clone();
        cpu.nvidia.clear();
        routing.speech =
            m.select_for_setup(&cpu, resource_packs::studio::Task::Tts { opt_in: false })?;
        let mut seen = std::collections::HashSet::new();
        routing.downloads = routing
            .transcription
            .downloads
            .iter()
            .chain(&routing.dictation.downloads)
            .chain(&routing.speech.downloads)
            .filter(|id| seen.insert((*id).clone()))
            .cloned()
            .collect();
        routing.download_bytes = routing
            .downloads
            .iter()
            .filter_map(|id| m.find(id))
            .map(|v| v.total_bytes)
            .sum();
    }
    let free = free_storage(&root)?;
    let asr = serde_json::to_value(crate::asr::get_asr_status(
        app.state::<crate::asr::AsrState>(),
    ))
    .map_err(|_| "Recognition status unavailable")?;
    let cleanup = serde_json::to_value(crate::cleanup::get_cleanup_status(
        app.state::<crate::cleanup::CleanupState>(),
    ))
    .map_err(|_| "Cleanup status unavailable")?;
    let generation = serde_json::to_value(crate::voice_editing::generation_status_for_hardware(
        &app.state::<crate::voice_editing::EditingState>(),
        &m,
        &h,
    )?)
    .map_err(|_| "Speech status unavailable")?;
    let recognition_available = crate::resource_setup::has_hardware_release(&m, &h);
    let mut features = Vec::new();
    for (id, title, selection) in [
        ("transcription", "Transcription", &routing.transcription),
        ("dictation", "Dictation", &routing.dictation),
        ("speech", "Text to Speech", &routing.speech),
    ] {
        let ready = if id == "speech" {
            generation["ready"] == true
        } else {
            asr["state"] == "ready"
        };
        let available = if id == "speech" {
            generation["canPrepare"] == true
        } else {
            recognition_available
        };
        let runtime_space = if id == "speech" {
            generation["runtimeDownloadBytes"]
                .as_u64()
                .unwrap_or(0)
                .saturating_add(generation["runtimeUnpackedBytes"].as_u64().unwrap_or(0))
        } else {
            0
        };
        let mut required = storage_required(missing_model_bytes(&root, &m, &selection.downloads))
            .saturating_add(runtime_space);
        if id != "speech" {
            if let Some(bytes) = crate::resource_setup::required_storage_for_hardware(&root, &m, &h)
            {
                required = bytes;
            }
        }
        let can_install = supported && selection.variant.is_some() && available && free >= required;
        let message = if ready {
            "Installed tools are ready. Existing resources are kept.".into()
        } else if !supported {
            compatible.as_ref().err().cloned().unwrap_or_default()
        } else if selection.variant.is_none() {
            selection.reason.clone()
        } else if free < required {
            format!(
                "Free {:.1} GB of storage to set up this tool.",
                required as f64 / 1e9
            )
        } else if !available {
            "A model is selected. Its compatible runtime download is not released for this preview yet.".into()
        } else {
            "Ready to download the tools selected for this computer.".into()
        };
        let active_version = if id == "speech" {
            generation["modelVersion"].as_str()
        } else {
            asr["modelVersion"].as_str()
        }
        .map(str::to_owned);
        features.push(Feature {
            id,
            title,
            model: selection.variant.clone(),
            downloads: selection.downloads.clone(),
            model_bytes: selection.download_bytes,
            ready,
            can_install,
            message,
            active_version,
            requires_consent: id == "speech" && generation["consent"] != true,
            required_storage_bytes: required,
        });
    }
    features.push(Feature {
        id: "editing",
        title: "Voice Editing",
        model: routing.editing.tts.clone(),
        downloads: routing.editing.downloads.clone(),
        model_bytes: routing.editing.download_bytes,
        ready: false,
        can_install: false,
        message: if routing.editing.available {
            "Optional. Open Voice Editing to prepare its tools when needed.".into()
        } else {
            routing.editing.reason.clone()
        },
        active_version: None,
        requires_consent: false,
        required_storage_bytes: 0,
    });
    features.push(Feature {
        id: "cleanup",
        title: "Voice Cleanup",
        model: None,
        downloads: vec![],
        model_bytes: 0,
        ready: cleanup["state"] == "ready",
        can_install: false,
        message: if cleanup["state"] == "ready" {
            "Installed cleanup tools are ready.".into()
        } else {
            "Cleanup is installed separately when a compatible runtime is available.".into()
        },
        active_version: cleanup["modelVersion"].as_str().map(str::to_owned),
        requires_consent: false,
        required_storage_bytes: 0,
    });
    Ok(SetupPlan {
        supported,
        reason: compatible.err().unwrap_or_else(|| {
            "Tools are matched to this computer. Downloads are checked again before they start."
                .into()
        }),
        hardware: h,
        os_version: version,
        routing,
        free_storage_bytes: free,
        features,
    })
}
#[tauri::command]
pub async fn get_setup_plan(app: tauri::AppHandle) -> Result<SetupPlan, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_setup(app))
        .await
        .map_err(|_| "Hardware check was interrupted")?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hardware() -> Hardware {
        Hardware {
            os: "macos".into(),
            arch: "aarch64".into(),
            total_ram_bytes: 4 * 1024 * 1024 * 1024,
            free_ram_bytes: 128 * 1024 * 1024,
            physical_cores: Some(4),
            avx2: None,
            nvidia: vec![],
        }
    }
    #[test]
    fn release_platform_validation_preserves_four_gb_support_but_rejects_old_or_unqualified_platforms(
    ) {
        let mut h = hardware();
        assert!(platform_check(&h, Some("14.0")).is_ok());
        assert!(platform_check(&h, Some("13.9")).is_err());
        assert!(platform_check(&h, None).is_err());
        h.os = "windows".into();
        assert!(platform_check(&h, Some("11")).is_err());
        h.arch = "x86_64".into();
        assert!(platform_check(&h, Some("11 (26200)")).is_ok());
        assert!(platform_check(&h, Some("10.0.19045")).is_ok());
        assert!(platform_check(&h, Some("10 (19045)")).is_ok());
        assert!(platform_check(&h, Some("10.0.18363")).is_err());
        assert!(platform_check(&h, Some("6.3")).is_err());
        assert!(platform_check(&h, None).is_err());
        h.os = "linux".into();
        assert!(platform_check(&h, Some("6.1")).is_err());
    }
    #[test]
    fn staging_budget_is_bounded_and_existing_resources_do_not_need_a_new_full_size_reserve() {
        assert_eq!(storage_required(0), 0);
        assert_eq!(storage_required(100), 256 * 1024 * 1024 + 200);
        assert_eq!(storage_required(u64::MAX), u64::MAX);
    }
    #[test]
    fn cached_size_is_an_estimate_and_does_not_trust_symlinked_files() {
        let m = Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        let root =
            std::env::temp_dir().join(format!("ethiopic-route-cache-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let id = vec!["tts-flow_lm-90m-amh-fp32".into()];
        let total = m.find("tts-flow_lm-90m-amh-fp32").unwrap().total_bytes;
        assert_eq!(missing_model_bytes(&root, &m, &id), total);
        let v = m.find("tts-flow_lm-90m-amh-fp32").unwrap();
        let f = v
            .files
            .iter()
            .find(|f| f.path == "voices/default.wav")
            .unwrap();
        let path = model_file(&root, m.hf_revision.as_ref().unwrap(), &v.folder, &f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(f.bytes).unwrap();
        assert_eq!(missing_model_bytes(&root, &m, &id), total - f.bytes);
        #[cfg(unix)]
        {
            let external = root.join("external.wav");
            std::fs::rename(&path, &external).unwrap();
            std::os::unix::fs::symlink(&external, &path).unwrap();
            assert_eq!(missing_model_bytes(&root, &m, &id), total);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
