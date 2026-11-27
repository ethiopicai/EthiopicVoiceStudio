//! Validated model selection inside a managed recognition bundle.
use resource_packs::studio::Manifest;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::Path};

const REPAIR: &str = "Recognition resources do not match this release. Repair voice tools.";

pub(crate) fn provider_error(code: &str) -> Option<&'static str> {
    match code {
        "gpu_unavailable" => Some("The voice engine could not use your NVIDIA GPU. Check its driver, then retry setup."),
        "gpu_precision_unsupported" => Some("These voice tools are incompatible with your GPU. Check setup for compatible voice tools."),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Provider {
    pub variant: String,
    pub backend: String,
    pub folder: String,
    pub device: String,
    pub dtype: String,
    pub model_version: String,
    pub model: String,
    pub threads: u32,
    #[serde(default)]
    pub context_sec: Option<f64>,
    #[serde(default)]
    pub update_sec: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bundle {
    schema_version: u32,
    pub transcription: Provider,
    pub dictation: Provider,
}

pub(crate) fn read(bundle: &Path) -> Result<Option<Bundle>, String> {
    let path = bundle.join("recognition.json");
    match fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(m) if m.is_file() && m.len() <= 64 * 1024 => {}
        _ => return Err(REPAIR.into()),
    }
    let value: Bundle =
        serde_json::from_slice(&fs::read(path).map_err(|_| REPAIR)?).map_err(|_| REPAIR)?;
    if value.schema_version != 1 {
        return Err(REPAIR.into());
    }
    Ok(Some(value))
}

impl Bundle {
    /// Check model/config metadata before activating a staged resource install.
    /// Large model bytes are verified by extraction or the inference integrity cache.
    pub(crate) fn validate_files(
        &self,
        bundle_dir: &Path,
        manifest: &Manifest,
        files: &HashMap<&str, &str>,
    ) -> Result<(), String> {
        let base = bundle_dir.canonicalize().map_err(|_| REPAIR)?;
        let file = |relative: &str| -> Result<std::path::PathBuf, String> {
            if !resource_packs::safe_path(relative) {
                return Err(REPAIR.into());
            }
            let path = bundle_dir.join(relative);
            let metadata = fs::symlink_metadata(&path).map_err(|_| REPAIR)?;
            let actual = path.canonicalize().map_err(|_| REPAIR)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || !actual.starts_with(&base)
            {
                return Err(REPAIR.into());
            }
            Ok(actual)
        };
        let config_path = file("recognition.json")?;
        let config_hash = files.get("bundle/recognition.json").ok_or(REPAIR)?;
        let current = read(bundle_dir)?.ok_or(REPAIR)?;
        if &current != self
            || format!(
                "{:x}",
                Sha256::digest(fs::read(config_path).map_err(|_| REPAIR)?)
            ) != *config_hash
        {
            return Err(REPAIR.into());
        }
        for provider in [&self.transcription, &self.dictation] {
            let relative = format!("{}/variant.json", provider.folder);
            let declared = files
                .get(format!("bundle/{relative}").as_str())
                .ok_or(REPAIR)?;
            let path = file(&relative)?;
            if fs::metadata(&path).map_err(|_| REPAIR)?.len() > 64 * 1024 {
                return Err(REPAIR.into());
            }
            let bytes = fs::read(path).map_err(|_| REPAIR)?;
            if format!("{:x}", Sha256::digest(&bytes)) != *declared {
                return Err(REPAIR.into());
            }
            let metadata: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| REPAIR)?;
            let expected = serde_json::to_value(manifest.find(&provider.variant).ok_or(REPAIR)?)
                .map_err(|_| REPAIR)?;
            for field in [
                "id",
                "folder",
                "model",
                "model_version",
                "runtime",
                "precision",
                "files",
            ] {
                if metadata[field] != expected[field] {
                    return Err(REPAIR.into());
                }
            }
        }
        for (path, sha) in self.required_files(manifest)? {
            if files.get(path.as_str()).copied() != Some(sha.as_str()) {
                return Err(REPAIR.into());
            }
            file(path.strip_prefix("bundle/").ok_or(REPAIR)?)?;
        }
        Ok(())
    }

    /// Return every required model file with its authoritative release checksum.
    pub(crate) fn required_files(
        &self,
        manifest: &Manifest,
    ) -> Result<HashMap<String, String>, String> {
        let mut files = HashMap::new();
        for provider in [&self.transcription, &self.dictation] {
            let variant = manifest.find(&provider.variant).ok_or(REPAIR)?;
            if variant.component != "asr"
                || provider.folder != variant.folder
                || provider.model_version.as_str()
                    != variant.model_version.as_deref().ok_or(REPAIR)?
                || provider.model.as_str() != variant.model.as_deref().ok_or(REPAIR)?
                || !(1..=64).contains(&provider.threads)
                || provider
                    .context_sec
                    .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 10.0)
                || provider
                    .update_sec
                    .is_some_and(|v| !v.is_finite() || !(0.1..=5.0).contains(&v))
                || !match provider.backend.as_str() {
                    "onnx" => {
                        variant.runtime == "onnxruntime-cpu"
                            && provider.device == "cpu"
                            && matches!(provider.dtype.as_str(), "float32" | "int8")
                    }
                    "torch" => {
                        variant.runtime.starts_with("torch-cuda")
                            && provider.device == "cuda"
                            && matches!(provider.dtype.as_str(), "float16" | "float32")
                    }
                    _ => false,
                }
            {
                return Err(REPAIR.into());
            }
            for file in &variant.files {
                files.insert(
                    format!("bundle/{}/{}", variant.folder, file.path),
                    file.sha256.clone(),
                );
            }
        }
        Ok(files)
    }
}

pub(crate) fn stage_workers(directory: &Path) -> Result<(), String> {
    for (name, source) in [
        (
            "provider.py",
            include_str!("../../../../workers/asr/provider.py"),
        ),
        (
            "streaming.py",
            include_str!("../../../../workers/asr/streaming.py"),
        ),
        (
            "setup_check.py",
            include_str!("../../../../workers/asr/setup_check.py"),
        ),
    ] {
        fs::write(directory.join(name), source)
            .map_err(|_| "Could not prepare recognition tools")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bundle() -> Bundle {
        let p = |id: &str, model: &str| json!({"variant":id,"backend":"onnx","folder":id,"device":"cpu","dtype":"float32","model_version":"v1.0","model":model,"threads":4});
        serde_json::from_value(json!({"schema_version":1,
            "transcription":p("asr-wav2vec_ctc-1b-amh-int8_onnx","amharic-ctc"),
            "dictation":p("asr-wav2vec_ctc-300m-amh-int8_onnx","amharic-ctc-300m")}))
        .unwrap()
    }
    #[test]
    fn batch_and_live_require_both_pinned_model_sets() {
        let manifest =
            Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        let files = bundle().required_files(&manifest).unwrap();
        assert!(files.contains_key("bundle/asr-wav2vec_ctc-1b-amh-int8_onnx/asr.int8.onnx"));
        assert!(files.contains_key("bundle/asr-wav2vec_ctc-300m-amh-int8_onnx/asr.int8.onnx"));
        assert_eq!(files.len(), 10);
    }
    #[test]
    fn rejects_redirected_paths_wrong_identity_and_incompatible_backends() {
        let manifest =
            Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        for invalid in [
            "folder", "version", "model", "backend", "threads", "context",
        ] {
            let mut config = bundle();
            match invalid {
                "folder" => config.dictation.folder = "../elsewhere".into(),
                "version" => config.dictation.model_version = "v0".into(),
                "model" => config.dictation.model = "amharic-ctc".into(),
                "backend" => config.dictation.backend = "torch".into(),
                "threads" => config.dictation.threads = 0,
                _ => config.dictation.context_sec = Some(f64::NAN),
            }
            assert!(config.required_files(&manifest).is_err(), "{invalid}");
        }
    }
}
