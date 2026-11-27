//! Read-only published model inventory. Loading this never downloads or executes a provider.
use resource_packs::studio::{Manifest, DEFAULT_MANIFEST_URL, DEFAULT_REPOSITORY};
use serde::Serialize;
use tauri::State;

const BUNDLED_MANIFEST: &str = include_str!("../../resources/studio-manifest.json");
const BUNDLED_RELEASE_CATALOG: &str = include_str!("../../resources/release-catalog.json");

#[derive(Clone, Serialize)]
pub struct ModelSource {
    manifest_url: &'static str,
    manifest: Manifest,
}

pub struct Registry {
    source: Result<ModelSource, String>,
}

fn parse_source(json: &str) -> Result<ModelSource, String> {
    let manifest = Manifest::parse(json)?;
    if manifest.hf_repo.as_deref() != Some(DEFAULT_REPOSITORY) || manifest.hf_revision.is_none() {
        return Err("The Studio model inventory must pin the approved repository.".into());
    }
    Ok(ModelSource {
        manifest_url: DEFAULT_MANIFEST_URL,
        manifest,
    })
}

impl Registry {
    pub fn new() -> Self {
        // Invalid model metadata must not prevent access to existing recordings or transcripts.
        Self {
            source: parse_source(BUNDLED_MANIFEST),
        }
    }
}

#[tauri::command]
pub fn get_studio_model_source(state: State<'_, Registry>) -> Result<ModelSource, String> {
    state.source.clone()
}

/// Complete embedded release inventory. This does not contact an update server,
/// accept unsigned metadata, or change installed providers.
#[tauri::command]
pub fn get_studio_release_catalog() -> Result<serde_json::Value, String> {
    let value: serde_json::Value = serde_json::from_str(BUNDLED_RELEASE_CATALOG)
        .map_err(|_| "The bundled release catalog could not be read.")?;
    if value["schema_version"] != 1 || value["product"] != "ethiopic-voice-studio" {
        return Err("This release catalog needs a Studio update.".into());
    }
    parse_source(&value["catalogs"]["models"].to_string())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn master_catalog_bundles_inventory_without_enabling_remote_updates() {
        let catalog = get_studio_release_catalog().unwrap();
        assert_eq!(catalog["bundled_app_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(catalog["inventory"].as_array().unwrap().len(), 15);
        assert!(catalog["artifacts"].as_object().unwrap().len() >= 96);
        let speech_platforms: std::collections::HashSet<_> = catalog["catalogs"]["speech_runtimes"]
            ["releases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["os"].as_str().unwrap())
            .collect();
        assert_eq!(speech_platforms, ["macos", "windows"].into_iter().collect());
        let windows_profiles: std::collections::HashSet<_> = catalog["catalogs"]
            ["recognition_packages"]["releases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["os"] == "windows")
            .map(|r| r["profile"].as_str().unwrap())
            .collect();
        assert_eq!(
            windows_profiles,
            [
                "cpu-low",
                "cpu-mid",
                "cpu-high",
                "gpu-low",
                "gpu-pascal",
                "gpu-modern"
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(
            catalog["catalogs"]["models"],
            serde_json::from_str::<serde_json::Value>(BUNDLED_MANIFEST).unwrap()
        );
        assert_eq!(catalog["release_status"], "draft");
        assert_eq!(catalog["update_policy"]["automatic_checks_enabled"], false);
        assert_eq!(
            catalog["update_policy"]["automatic_downloads_enabled"],
            false
        );
    }

    #[test]
    fn released_inventory_resolves_every_declared_file_to_its_pinned_upload() {
        let source = Registry::new().source.expect("Valid bundled release");
        assert_eq!(source.manifest.variants.len(), 12);
        assert!(source.manifest.find("tts-flow_lm-90m-amh-fp32").is_some());
        assert_eq!(
            source.manifest_url,
            format!("https://huggingface.co/{DEFAULT_REPOSITORY}/resolve/main/manifest.json")
        );
        let revision = source.manifest.hf_revision.as_deref().unwrap();
        let mut count = 0;
        for variant in &source.manifest.variants {
            for file in &variant.files {
                if let Some(bytes) =
                    resource_packs::naming::bundled_file(&source.manifest, &variant.id, &file.path)
                {
                    assert_eq!(bytes.len() as u64, file.bytes);
                    count += 1;
                    continue;
                }
                let url = source.manifest.file_url(&variant.id, &file.path).unwrap();
                let expected = resource_packs::naming::legacy_file_url(
                    &source.manifest,
                    &variant.id,
                    &file.path,
                )
                .unwrap_or_else(|| {
                    format!(
                        "https://huggingface.co/{DEFAULT_REPOSITORY}/resolve/{revision}/{}/{}",
                        variant.folder, file.path
                    )
                });
                assert_eq!(url, expected);
                count += 1;
            }
        }
        assert_eq!(count, 74);
    }

    #[test]
    fn inventory_rejects_missing_hosting_or_a_different_repository() {
        let mut manifest: serde_json::Value = serde_json::from_str(BUNDLED_MANIFEST).unwrap();
        manifest["hf_repo"] = serde_json::json!("ethiopic/unapproved-models");
        assert!(parse_source(&manifest.to_string()).is_err());
        manifest["hf_repo"] = serde_json::Value::Null;
        manifest["hf_revision"] = serde_json::Value::Null;
        assert!(parse_source(&manifest.to_string()).is_err());
    }
}
