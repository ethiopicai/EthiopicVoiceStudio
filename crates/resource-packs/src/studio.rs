//! Studio variant manifest and hardware selection. Selection alone never downloads or runs code.
use crate::{hash_valid, safe_path, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MIB: u64 = 1024 * 1024;
pub const DEFAULT_REPOSITORY: &str = "ethiopic/ethiopic-studio-models";
pub const DEFAULT_MANIFEST_URL: &str =
    "https://huggingface.co/ethiopic/ethiopic-studio-models/resolve/main/manifest.json";
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub generated: String,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    pub bundles: HashMap<String, Bundle>,
    pub variants: Vec<Variant>,
    pub hf_repo: Option<String>,
    pub hf_revision: Option<String>,
    #[serde(flatten)]
    pub informational: serde_json::Map<String, serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bundle {
    pub asr: String,
    pub asr_live: String,
    pub corrector: String,
    pub tts: Option<String>,
    pub tts_optin: Option<String>,
    pub tts_num_step: Option<u32>,
    pub edit: Option<String>,
    pub edit_optin: Option<String>,
    #[serde(flatten)]
    pub informational: serde_json::Map<String, serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Variant {
    pub id: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub release: String,
    #[serde(default)]
    pub previous_name: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub component: String,
    pub runtime: String,
    pub precision: String,
    pub model: Option<String>,
    pub model_version: Option<String>,
    pub folder: String,
    pub total_bytes: u64,
    pub files: Vec<ModelFile>,
    pub requires: Vec<String>,
    #[serde(default)]
    pub requires_one_of: Vec<String>,
    pub ram_mb_est: Option<u64>,
    pub vram_mb_est: Option<u64>,
    pub license: String,
    #[serde(flatten)]
    pub informational: serde_json::Map<String, serde_json::Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModelFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    GpuModern,
    GpuLow,
    GpuPascal,
    AppleArm,
    CpuLow,
    CpuMid,
    CpuHigh,
}
impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Self::GpuModern => "gpu-modern",
            Self::GpuLow => "gpu-low",
            Self::GpuPascal => "gpu-pascal",
            Self::AppleArm => "apple-arm",
            Self::CpuLow => "cpu-low",
            Self::CpuMid => "cpu-mid",
            Self::CpuHigh => "cpu-high",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Nvidia {
    pub index: u32,
    pub compute_major: u32,
    pub compute_minor: u32,
    pub total_vram_bytes: u64,
    pub free_vram_bytes: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub os: String,
    pub arch: String,
    pub total_ram_bytes: u64,
    pub free_ram_bytes: u64,
    pub physical_cores: Option<u32>,
    pub avx2: Option<bool>,
    pub nvidia: Vec<Nvidia>,
}
fn gpu_tier(gpu: &Nvidia) -> Option<Tier> {
    let modern = (gpu.compute_major, gpu.compute_minor) >= (7, 5);
    if modern && gpu.total_vram_bytes >= 6 * 1024 * MIB {
        Some(Tier::GpuModern)
    } else if modern && gpu.total_vram_bytes >= 4 * 1024 * MIB {
        Some(Tier::GpuLow)
    } else if gpu.compute_major == 6 && gpu.total_vram_bytes >= 6 * 1024 * MIB {
        Some(Tier::GpuPascal)
    } else {
        None
    }
}
pub fn tier(hardware: &Hardware) -> Tier {
    for preferred in [Tier::GpuModern, Tier::GpuLow, Tier::GpuPascal] {
        if hardware
            .nvidia
            .iter()
            .any(|gpu| gpu_tier(gpu) == Some(preferred))
        {
            return preferred;
        }
    }
    if matches!(hardware.os.as_str(), "macos" | "windows")
        && matches!(hardware.arch.as_str(), "aarch64" | "arm64")
    {
        return Tier::AppleArm;
    }
    let x86 = matches!(hardware.arch.as_str(), "x86_64" | "x86" | "i686");
    if hardware.total_ram_bytes < 8 * 1024 * MIB
        || hardware.physical_cores.unwrap_or(0) < 4
        || (x86 && hardware.avx2 != Some(true))
    {
        Tier::CpuLow
    } else if hardware.physical_cores.unwrap_or(0) >= 6
        && hardware.total_ram_bytes >= 16 * 1024 * MIB
    {
        Tier::CpuHigh
    } else {
        Tier::CpuMid
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Task {
    Transcription,
    Dictation,
    Corrector,
    Tts { opt_in: bool },
}
#[derive(Clone, Copy, Debug, Default)]
pub enum Preference {
    #[default]
    Automatic,
    Faster,
    MoreAccurate,
}
#[derive(Debug, Serialize)]
pub struct Selection {
    pub tier: Tier,
    pub variant: Option<String>,
    /// All required variants, in dependency-before-consumer order, with no duplicates.
    pub downloads: Vec<String>,
    pub download_bytes: u64,
    pub reason: String,
    pub model: Option<String>,
    pub model_version: Option<String>,
    pub tts_num_step: Option<u32>,
    pub codec_fp32: bool,
    pub pascal_fp32: bool,
    pub licenses: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EditingSelection {
    pub tier: Tier,
    pub available: bool,
    pub opt_in_required: bool,
    pub slow: bool,
    pub reason: String,
    pub tts: Option<String>,
    pub device: Option<String>,
    pub steps: u32,
    pub downloads: Vec<String>,
    pub download_bytes: u64,
    pub licenses: Vec<String>,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > 16 * 1024 * 1024 {
            return Err("Studio manifest is too large".into());
        }
        let value: Self = serde_json::from_str(text).map_err(|_| "Studio manifest is invalid")?;
        if !matches!(value.manifest_version, 1 | 2)
            || value.variants.is_empty()
            || value.variants.len() > 256
            || value.bundles.len() > 32
            || value.generated.is_empty()
        {
            return Err("Unsupported or incomplete Studio manifest".into());
        }
        let mut ids = HashSet::new();
        for variant in &value.variants {
            if !valid_id(&variant.id)
                || variant.folder != variant.id
                || !ids.insert(&variant.id)
                || variant.files.is_empty()
                || variant.files.len() > 50_000
                || variant.license.trim().is_empty()
                || variant.ram_mb_est.is_some_and(|n| n > 1024 * 1024)
                || variant.vram_mb_est.is_some_and(|n| n > 1024 * 1024)
            {
                return Err("Invalid or duplicate Studio variant".into());
            }
            if value.manifest_version == 2
                && (!valid_id(&variant.family)
                    || variant.release.trim().is_empty()
                    || variant.release.len() > 200
                    || (variant.component != "tts-codec"
                        && variant.model_version.as_deref().is_none_or(str::is_empty)))
            {
                return Err(
                    "Version-2 variants must identify their family, release and version".into(),
                );
            }
            if variant.component == "asr"
                && (variant.model.as_ref().is_none_or(|s| s.trim().is_empty())
                    || variant
                        .model_version
                        .as_ref()
                        .is_none_or(|s| s.trim().is_empty()))
            {
                return Err("ASR variants must identify both model and version".into());
            }
            let mut paths = HashSet::new();
            let mut total = 0_u64;
            for file in &variant.files {
                if !safe_path(&file.path)
                    || !paths.insert(&file.path)
                    || !hash_valid(&file.sha256)
                    || file.bytes > 8 * 1024 * 1024 * 1024
                    || file.path.split('/').any(|p| {
                        p == "__pycache__" || p.starts_with("_work_") || p.starts_with("_validate_")
                    })
                    || file.path.ends_with(".pyc")
                    || file.path == "build_manifest.py"
                {
                    return Err("Unsafe or duplicate Studio file".into());
                }
                total = total
                    .checked_add(file.bytes)
                    .ok_or("Studio size overflow")?;
            }
            if total != variant.total_bytes || total > 16 * 1024 * 1024 * 1024 {
                return Err("Studio variant size does not match its files".into());
            }
        }
        if value.aliases.len() > 256 {
            return Err("Too many Studio aliases".into());
        }
        for (old, current) in &value.aliases {
            if !valid_id(old) || ids.contains(old) || !ids.contains(current) || old == current {
                return Err("Invalid or ambiguous Studio alias".into());
            }
        }
        for variant in &value.variants {
            let mut aliases = HashSet::new();
            for old in &variant.aliases {
                if !aliases.insert(old) || value.aliases.get(old) != Some(&variant.id) {
                    return Err("Variant aliases disagree with the Studio alias table".into());
                }
            }
        }
        for variant in &value.variants {
            value.dependencies(&variant.id)?;
            let mut alternatives = HashSet::new();
            for alternative in &variant.requires_one_of {
                if !alternatives.insert(alternative)
                    || alternative == &variant.id
                    || value.find(alternative).is_none()
                {
                    return Err("Invalid Studio alternative dependency".into());
                }
            }
        }
        for bundle in value.bundles.values() {
            if bundle
                .informational
                .values()
                .filter_map(serde_json::Value::as_str)
                .any(|id| value.find(id).is_none())
            {
                return Err("Studio tier references an unavailable variant".into());
            }
            for (id, component) in [
                (&bundle.asr, "asr"),
                (&bundle.asr_live, "asr"),
                (&bundle.corrector, "corrector"),
            ] {
                if value.find(id).is_none_or(|v| v.component != component) {
                    return Err("Studio tier references an unavailable component".into());
                }
            }
            if bundle
                .tts_num_step
                .is_some_and(|n| !(16..=128).contains(&n))
            {
                return Err("Unsupported TTS quality setting".into());
            }
            for id in [&bundle.tts, &bundle.tts_optin].into_iter().flatten() {
                if value.find(id).is_some_and(|v| v.component != "tts") {
                    return Err("Invalid TTS variant in tier".into());
                }
            }
            for id in [&bundle.edit, &bundle.edit_optin].into_iter().flatten() {
                if value.find(id).is_none_or(|v| v.component != "edit") {
                    return Err("Invalid editing variant in tier".into());
                }
            }
        }
        match (&value.hf_repo, &value.hf_revision) {
            (None, None) => {}
            (Some(repo), Some(revision))
                if repo.strip_prefix("ethiopic/").is_some_and(valid_id)
                    && revision.len() == 40
                    && revision.bytes().all(|b| b.is_ascii_hexdigit()) => {}
            _ => return Err("Studio hosting must pin a public Ethiopic HF commit".into()),
        }
        Ok(value)
    }
    pub fn find(&self, id: &str) -> Option<&Variant> {
        let id = self.aliases.get(id).map(String::as_str).unwrap_or(id);
        self.variants.iter().find(|v| v.id == id)
    }
    /// Keep old settings usable for at least this Studio release. Always persist the returned ID.
    pub fn resolve(&self, id: &str) -> Option<&str> {
        self.find(id).map(|variant| variant.id.as_str())
    }
    /// Editing estimates are aggregate process footprints, including TTS, codec and aligner.
    /// Do not add those dependencies a second time when checking the editor's budget.
    pub fn select_editing(&self, hardware: &Hardware, opt_in: bool) -> Result<EditingSelection> {
        let selected_tier = tier(hardware);
        let mut result = EditingSelection {
            tier: selected_tier, available: false, opt_in_required: false,
            slow: selected_tier == Tier::CpuHigh, reason: "Voice Editing is unavailable on this hardware. It needs a supported NVIDIA GPU or a high-memory CPU computer.".into(),
            tts: None, device: None, steps: 16, downloads: vec![], download_bytes: 0, licenses: vec![],
        };
        let Some(bundle) = self.bundles.get(selected_tier.name()) else {
            return Ok(result);
        };
        if selected_tier == Tier::AppleArm {
            result.reason="Voice Editing is not available on this ARM platform yet. Transcription and Dictation remain available.".into();
        } else if matches!(selected_tier, Tier::CpuLow | Tier::CpuMid) {
            result.reason="Voice Editing needs a supported NVIDIA GPU, or a CPU with at least 16 GB RAM, six physical cores and AVX2 on x86.".into();
        }
        let editor_id = if let Some(id) = &bundle.edit {
            id
        } else if let Some(id) = &bundle.edit_optin {
            result.opt_in_required = !opt_in;
            if !opt_in {
                result.reason = "Voice Editing on this computer is slow. Enable slow editing to continue (about 15–20 seconds per change; hardware-dependent).".into();
                return Ok(result);
            }
            id
        } else {
            return Ok(result);
        };
        let editor = self.find(editor_id).ok_or("Editing variant is missing")?;
        let tts = if bundle.edit.is_some() {
            bundle.tts.as_ref()
        } else {
            bundle.tts_optin.as_ref()
        }
        .ok_or("Editing needs the tier's OmniVoice variant")?;
        if !editor.requires_one_of.contains(tts) || !tts.starts_with("tts-codec_llm-0.6b-amh-") {
            return Err("The editing tier does not select a supported OmniVoice dependency".into());
        }
        let mut ids = vec![];
        for variant in self
            .dependencies(tts)?
            .into_iter()
            .chain(self.dependencies(editor_id)?)
        {
            if !ids.contains(&variant.id) {
                ids.push(variant.id.clone());
            }
        }
        let gpu = matches!(
            selected_tier,
            Tier::GpuModern | Tier::GpuLow | Tier::GpuPascal
        );
        let ram = editor
            .ram_mb_est
            .ok_or("Editing memory estimate is missing")?
            * MIB;
        let vram = if gpu {
            editor
                .vram_mb_est
                .unwrap_or(0)
                .max(if selected_tier == Tier::GpuPascal {
                    4300
                } else {
                    3300
                })
                * MIB
        } else {
            0
        };
        let selected_gpu = hardware
            .nvidia
            .iter()
            .filter(|g| gpu_tier(g) == Some(selected_tier))
            .max_by_key(|g| g.free_vram_bytes.min(g.total_vram_bytes));
        let gpu_free = selected_gpu
            .map(|g| g.free_vram_bytes.min(g.total_vram_bytes))
            .unwrap_or(0);
        if ram > hardware.free_ram_bytes.min(hardware.total_ram_bytes) / 10 * 7
            || vram > gpu_free / 10 * 7
        {
            result.reason = "Not enough memory is available for Voice Editing. Close other applications or release loaded voice tools and retry.".into();
            return Ok(result);
        }
        result.available = true;
        result.device = if gpu {
            selected_gpu.map(|g| format!("cuda:{}", g.index))
        } else {
            Some("cpu".into())
        };
        result.tts = Some(tts.clone());
        result.steps = bundle.tts_num_step.unwrap_or(16).max(16);
        result.download_bytes = ids
            .iter()
            .filter_map(|id| self.find(id))
            .map(|v| v.total_bytes)
            .sum();
        result.licenses = ids
            .iter()
            .filter_map(|id| self.find(id))
            .map(|v| v.license.clone())
            .collect();
        result.downloads = ids;
        result.reason = if result.slow {
            "Slow editing enabled · approximately 15–20 seconds per change; hardware-dependent."
        } else {
            "Voice Editing runs locally on this computer."
        }
        .into();
        Ok(result)
    }
    /// Choose durable downloads from hardware capacity; transient memory pressure is a warning.
    /// CPU synthesis uses FP32 Pocket TTS, independently of editing eligibility.
    pub fn select_for_setup(&self, hardware: &Hardware, task: Task) -> Result<Selection> {
        let mut capacity = hardware.clone();
        capacity.free_ram_bytes = capacity.total_ram_bytes;
        for gpu in &mut capacity.nvidia {
            gpu.free_vram_bytes = gpu.total_vram_bytes;
        }
        if matches!(task, Task::Tts { .. }) {
            let bundle = self.bundles.get(tier(&capacity).name());
            if bundle
                .and_then(|b| b.tts.as_deref())
                .is_some_and(|id| id.starts_with("tts-codec_llm-0.6b-amh-"))
            {
                // Current OmniVoice synthesis shares the editor/aligner runtime: use its aggregate footprint.
                let editing = self.select_editing(&capacity, false)?;
                if editing.available {
                    let mut selected = self.select(&capacity, task, Preference::Automatic)?;
                    selected.variant = editing.tts.clone();
                    selected.downloads = editing.downloads;
                    selected.download_bytes = editing.download_bytes;
                    selected.model = selected
                        .variant
                        .as_deref()
                        .and_then(|id| self.find(id))
                        .and_then(|v| v.model.clone());
                    selected.model_version = selected
                        .variant
                        .as_deref()
                        .and_then(|id| self.find(id))
                        .and_then(|v| v.model_version.clone());
                    selected.licenses = editing.licenses;
                    selected.tts_num_step = Some(editing.steps);
                    selected.codec_fp32 = true;
                    selected.reason =
                        "Selected the compatible GPU voice engine and its required resources."
                            .into();
                    return Ok(selected);
                }
            } else {
                let selected = self.select(&capacity, task, Preference::Automatic)?;
                if selected.variant.is_some() {
                    return Ok(selected);
                }
            }
            capacity.nvidia.clear();
            let mut fallback = self.select(
                &capacity,
                Task::Tts { opt_in: false },
                Preference::Automatic,
            )?;
            fallback.tier = tier(hardware);
            if fallback.variant.is_some() {
                fallback.reason =
                    "Using the CPU voice engine because the GPU engine does not fit this computer."
                        .into();
            }
            return Ok(fallback);
        }
        self.select(&capacity, task, Preference::Automatic)
    }
    pub fn dependencies(&self, id: &str) -> Result<Vec<&Variant>> {
        fn visit<'a>(
            m: &'a Manifest,
            id: &str,
            seen: &mut HashSet<String>,
            stack: &mut HashSet<String>,
            out: &mut Vec<&'a Variant>,
        ) -> Result<()> {
            let id = m.resolve(id).ok_or("Studio dependency is missing")?;
            if seen.contains(id) {
                return Ok(());
            }
            if !stack.insert(id.to_string()) {
                return Err("Studio dependency cycle".into());
            }
            let variant = m.find(id).ok_or("Studio dependency is missing")?;
            for dep in &variant.requires {
                visit(m, dep, seen, stack, out)?;
            }
            stack.remove(id);
            seen.insert(id.to_string());
            out.push(variant);
            Ok(())
        }
        let mut result = Vec::new();
        visit(
            self,
            id,
            &mut HashSet::new(),
            &mut HashSet::new(),
            &mut result,
        )?;
        Ok(result)
    }
    pub fn file_url(&self, id: &str, path: &str) -> Result<String> {
        let variant = self.find(id).ok_or("Unknown Studio variant")?;
        if !variant.files.iter().any(|f| f.path == path) {
            return Err("File is not declared by the Studio manifest".into());
        }
        if crate::naming::bundled_file(self, &variant.id, path).is_some() {
            return Err(
                "This model support file is included in the Studio app; no download URL is needed."
                    .into(),
            );
        }
        let repo = self
            .hf_repo
            .as_ref()
            .ok_or("Studio variants are not published yet")?;
        let revision = self
            .hf_revision
            .as_ref()
            .ok_or("Studio revision is unavailable")?;
        if let Some(url) = crate::naming::legacy_file_url(self, &variant.id, path) {
            return Ok(url);
        }
        if !repo.strip_prefix("ethiopic/").is_some_and(valid_id)
            || revision.len() != 40
            || !revision.bytes().all(|b| b.is_ascii_hexdigit())
            || !valid_id(&variant.folder)
            || !safe_path(path)
        {
            return Err("Studio hosting must pin a public Ethiopic HF commit".into());
        }
        let mut url =
            reqwest::Url::parse("https://huggingface.co/").map_err(|_| "Invalid resource host")?;
        url.path_segments_mut()
            .map_err(|_| "Invalid resource host")?
            .extend(repo.split('/'))
            .push("resolve")
            .push(revision)
            .push(&variant.folder)
            .extend(path.split('/'));
        Ok(url.into())
    }
    /// Fetch only a manifest-listed file, using the same bounded/resumable verified transport
    /// as release archives. The manifest is data; entry-point strings are never evaluated.
    pub fn download_file(
        &self,
        id: &str,
        path: &str,
        cache: &std::path::Path,
        paused: &std::sync::atomic::AtomicBool,
        mut progress: impl FnMut(u64),
    ) -> Result<std::path::PathBuf> {
        let file = self
            .find(id)
            .and_then(|v| v.files.iter().find(|f| f.path == path))
            .ok_or("Undeclared Studio file")?;
        if !hash_valid(&file.sha256) || file.bytes > 8 * 1024 * 1024 * 1024 {
            return Err("Invalid Studio file metadata".into());
        }
        if paused.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("paused".into());
        }
        let expected = file.sha256.to_ascii_lowercase();
        let directory = cache.join(&expected);
        std::fs::create_dir_all(&directory)
            .map_err(|_| "Could not prepare model download storage")?;
        if let Some(bytes) = crate::naming::bundled_file(self, id, path) {
            use sha2::{Digest, Sha256};
            if bytes.len() as u64 != file.bytes
                || format!("{:x}", Sha256::digest(bytes)) != expected
            {
                return Err("The bundled model naming update is damaged".into());
            }
            let target = directory.join("download.tar.gz");
            std::fs::write(&target, bytes)
                .map_err(|_| "Could not prepare updated model support files")?;
            progress(file.bytes);
            return Ok(target);
        }
        if file.bytes == 0 {
            use sha2::{Digest, Sha256};
            if format!("{:x}", Sha256::digest([])) != expected {
                return Err("Invalid empty model file checksum".into());
            }
            let target = directory.join("download.tar.gz");
            std::fs::write(&target, []).map_err(|_| "Could not save model file")?;
            progress(0);
            return Ok(target);
        }
        let artifact = crate::Artifact {
            kind: "model".into(),
            url: self.file_url(id, path)?,
            sha256: expected,
            size: file.bytes,
            unpacked_size: file.bytes,
        };
        crate::download_with(
            &crate::client()?,
            &artifact,
            &directory,
            paused,
            &mut progress,
        )
    }
    fn fits(&self, id: &str, hardware: &Hardware, selected_tier: Tier) -> bool {
        let Ok(variants) = self.dependencies(id) else {
            return false;
        };
        let mut ram = 0_u64;
        let mut vram = 0_u64;
        for variant in variants {
            if variant.runtime != "onnxruntime-cpu"
                && !variant.runtime.starts_with("torch-cuda")
                && variant.runtime != "torch (cuda or cpu)"
                && !(variant.id == "tts-flow_lm-90m-amh-fp32"
                    && variant.runtime == "torch-cpu (pocket-tts)")
            {
                return false;
            }
            let gpu = variant.runtime.contains("cuda") && !variant.runtime.contains("cpu");
            // Codecs follow the consuming model's device. Other CPU components stay on CPU.
            let gpu = gpu
                || (variant.component == "tts-codec"
                    && matches!(
                        selected_tier,
                        Tier::GpuModern | Tier::GpuLow | Tier::GpuPascal
                    )
                    && self.find(id).is_some_and(|v| v.runtime.contains("cuda")));
            if variant.component == "tts-codec" && variant.precision != "fp32" {
                return false;
            }
            if variant.runtime.contains("torch")
                && variant.precision.contains("int8")
                && variant.id != "tts-flow_lm-90m-amh-fp32"
                && matches!(hardware.arch.as_str(), "aarch64" | "arm64")
            {
                return false;
            }
            if gpu {
                if !matches!(
                    selected_tier,
                    Tier::GpuModern | Tier::GpuLow | Tier::GpuPascal
                ) || (selected_tier == Tier::GpuPascal && variant.precision.contains("fp16"))
                {
                    return false;
                }
                let Some(estimate) = variant.vram_mb_est else {
                    return false;
                };
                vram += estimate * MIB;
            } else {
                let Some(estimate) = variant.ram_mb_est else {
                    return false;
                };
                ram += estimate * MIB;
            }
        }
        let gpu_free = hardware
            .nvidia
            .iter()
            .filter(|g| gpu_tier(g) == Some(selected_tier))
            .map(|g| g.free_vram_bytes.min(g.total_vram_bytes))
            .max()
            .unwrap_or(0);
        ram <= hardware.free_ram_bytes.min(hardware.total_ram_bytes) / 10 * 7
            && vram <= gpu_free / 10 * 7
    }
    pub fn select(
        &self,
        hardware: &Hardware,
        task: Task,
        preference: Preference,
    ) -> Result<Selection> {
        let detected = tier(hardware);
        let mut result = Selection {
            tier: detected,
            variant: None,
            downloads: vec![],
            download_bytes: 0,
            reason: "This tool is not available for this computer yet.".into(),
            model: None,
            model_version: None,
            tts_num_step: None,
            codec_fp32: false,
            pascal_fp32: false,
            licenses: vec![],
        };
        let Some(bundle) = self.bundles.get(detected.name()) else {
            return Ok(result);
        };
        let desired = match task {
            Task::Transcription => Some(bundle.asr.clone()),
            Task::Dictation => Some(bundle.asr_live.clone()),
            Task::Corrector => Some(bundle.corrector.clone()),
            Task::Tts { opt_in } => bundle.tts.clone().or_else(|| {
                if opt_in {
                    bundle.tts_optin.clone()
                } else {
                    None
                }
            }),
        };
        let Some(mut desired) = desired else {
            if matches!(task, Task::Tts { opt_in: false }) && bundle.tts_optin.is_some() {
                result.reason =
                    "Speech generation on this CPU is slow and requires opting in.".into();
            }
            return Ok(result);
        };
        if matches!(task, Task::Transcription | Task::Dictation) {
            desired = match preference {
                Preference::Faster => desired.replace("-1b-", "-300m-"),
                Preference::MoreAccurate => desired.replace("-300m-", "-1b-"),
                Preference::Automatic => desired,
            };
        }
        let mut candidates = vec![desired.clone()];
        if desired.contains("-1b-") {
            candidates.push(desired.replace("-1b-", "-300m-"));
        }
        if !candidates.iter().any(|id| self.find(id).is_some()) {
            return Ok(result);
        }
        let selected = candidates
            .iter()
            .find(|id| self.find(id).is_some() && self.fits(id, hardware, detected));
        let Some(selected) = selected else {
            result.reason="No compatible model fits the currently available memory. Close other applications and retry.".into();
            return Ok(result);
        };
        let variant = self
            .find(selected)
            .ok_or("Selected Studio variant is missing")?;
        let dependencies = self.dependencies(selected)?;
        result.variant = Some(variant.id.clone());
        result.model = variant.model.clone();
        result.model_version = variant.model_version.clone();
        result.downloads = dependencies.iter().map(|v| v.id.clone()).collect();
        result.download_bytes = dependencies.iter().map(|v| v.total_bytes).sum();
        result.licenses = dependencies.iter().map(|v| v.license.clone()).collect();
        result.pascal_fp32 = detected == Tier::GpuPascal;
        result.codec_fp32 = dependencies.iter().any(|v| v.component == "tts-codec");
        if matches!(task, Task::Tts { .. }) {
            result.tts_num_step = Some(bundle.tts_num_step.unwrap_or(16).max(16));
        }
        result.reason = if variant.id != desired {
            "Using the smaller model to fit the available memory."
        } else if matches!(task, Task::Tts { .. }) && detected == Tier::CpuHigh {
            "Slow CPU speech generation enabled by your choice."
        } else {
            "Selected for this computer and the available memory."
        }
        .into();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn editing_tiers_choose_only_the_required_omnivoice_alternative() {
        let manifest = Manifest::parse(include_str!(
            "../../../apps/desktop/resources/studio-manifest.json"
        ))
        .unwrap();
        let mut h = Hardware {
            os: "windows".into(),
            arch: "x86_64".into(),
            total_ram_bytes: 32 * 1024 * MIB,
            free_ram_bytes: 24 * 1024 * MIB,
            physical_cores: Some(8),
            avx2: Some(true),
            nvidia: vec![],
        };
        let off = manifest.select_editing(&h, false).unwrap();
        assert!(!off.available && off.opt_in_required);
        let cpu = manifest.select_editing(&h, true).unwrap();
        assert!(cpu.available && cpu.slow);
        assert_eq!(cpu.tts.as_deref(), Some("tts-codec_llm-0.6b-amh-int8_onnx"));
        assert!(!cpu
            .downloads
            .iter()
            .any(|id| id == "tts-flow_lm-90m-amh-fp32"));
        for (major, total, expected, steps) in [
            (8, 8, "tts-codec_llm-0.6b-amh-fp16", 32),
            (8, 5, "tts-codec_llm-0.6b-amh-fp16", 16),
            (6, 8, "tts-codec_llm-0.6b-amh-fp32", 16),
        ] {
            h.nvidia = vec![Nvidia {
                index: 1,
                compute_major: major,
                compute_minor: 1,
                total_vram_bytes: total * 1024 * MIB,
                free_vram_bytes: total * 1024 * MIB,
            }];
            let plan = manifest.select_editing(&h, false).unwrap();
            assert!(plan.available);
            assert_eq!(plan.device.as_deref(), Some("cuda:1"));
            assert_eq!(plan.steps, steps);
            assert_eq!(plan.tts.as_deref(), Some(expected));
            for id in [
                "edit-codec_llm-0.6b-amh",
                "codec-higgs_audio_v2-fp32",
                "asr-wav2vec_ctc-1b-amh-int8_onnx",
                expected,
            ] {
                assert!(plan.downloads.contains(&id.to_string()));
            }
            assert_eq!(
                plan.downloads
                    .iter()
                    .filter(|id| id.starts_with("tts-codec_llm-0.6b-amh-"))
                    .count(),
                1
            );
        }
        h.nvidia.clear();
        h.physical_cores = Some(4);
        h.total_ram_bytes = 12 * 1024 * MIB;
        assert!(!manifest.select_editing(&h, true).unwrap().available);
        h.physical_cores = Some(2);
        assert!(!manifest.select_editing(&h, true).unwrap().available);
        h.os = "macos".into();
        h.arch = "aarch64".into();
        h.total_ram_bytes = 64 * 1024 * MIB;
        h.physical_cores = Some(12);
        assert!(!manifest.select_editing(&h, true).unwrap().available);
    }
    #[test]
    fn editing_rejects_low_memory_and_missing_alternative_dependencies() {
        let source = include_str!("../../../apps/desktop/resources/studio-manifest.json");
        let m = Manifest::parse(source).unwrap();
        let mut h = hardware();
        h.os = "windows".into();
        h.arch = "x86_64".into();
        h.avx2 = Some(true);
        h.free_ram_bytes = 1024 * MIB;
        assert!(!m.select_editing(&h, true).unwrap().available);
        let mut value: Value = serde_json::from_str(source).unwrap();
        let editor = value["variants"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|v| v["id"] == "edit-codec_llm-0.6b-amh")
            .unwrap();
        editor["requires_one_of"] = json!(["missing-voice-model"]);
        assert!(Manifest::parse(&value.to_string()).is_err());
    }
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let suffix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "ethiopic-studio-cache-{}-{suffix}",
                std::process::id()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn hardware() -> Hardware {
        Hardware {
            os: "macos".into(),
            arch: "aarch64".into(),
            total_ram_bytes: 16 * 1024 * MIB,
            free_ram_bytes: 8 * 1024 * MIB,
            physical_cores: Some(8),
            avx2: None,
            nvidia: vec![],
        }
    }
    fn variant(
        id: &str,
        component: &str,
        runtime: &str,
        precision: &str,
        ram: Option<u64>,
        vram: Option<u64>,
        requires: Vec<&str>,
    ) -> Value {
        json!({"id":id,"component":component,"runtime":runtime,"precision":precision,"folder":id,
            "model":if component=="asr" {Some(if id.contains("300m") {"amharic-ctc-300m"} else {"amharic-ctc"})} else {None},
            "model_version":if component=="asr" {Some("v0.3")} else {None},
            "ram_mb_est":ram,"vram_mb_est":vram,"license":"non-commercial weights; MIT reference code",
            "files":[{"path":"weights.onnx","bytes":10,"sha256":"a".repeat(64)}],"total_bytes":10,"requires":requires})
    }
    fn fixture() -> Value {
        let mut m = json!({"manifest_version":1,"generated":"2026-11-13","bundles":{},"variants":[
            variant("asr-wav2vec_ctc-1b-amh-int8_onnx","asr","onnxruntime-cpu","int8",Some(2600),Some(0),vec![]),
            variant("asr-wav2vec_ctc-300m-amh-int8_onnx","asr","onnxruntime-cpu","int8",Some(1400),Some(0),vec![]),
            variant("asr-wav2vec_ctc-1b-amh-fp16","asr","torch-cuda","fp16",Some(2800),Some(2600),vec![]),
            variant("asr-wav2vec_ctc-300m-amh-fp16","asr","torch-cuda","fp16",Some(1600),Some(1300),vec![]),
            variant("textcorrect-char_bart-7m-amh-int8_onnx","corrector","onnxruntime-cpu","int8",Some(120),Some(0),vec![]),
            variant("tts-codec_llm-0.6b-amh-int8_onnx","tts","onnxruntime-cpu","int8",Some(3800),None,vec!["codec-higgs_audio_v2-fp32"]),
            variant("tts-codec_llm-0.6b-amh-fp16","tts","torch-cuda","fp16",None,Some(3000),vec!["codec-higgs_audio_v2-fp32"]),
            variant("tts-codec_llm-0.6b-amh-fp32","tts","torch-cuda","fp32",None,Some(4500),vec!["codec-higgs_audio_v2-fp32"]),
            variant("codec-higgs_audio_v2-fp32","tts-codec","torch (cuda or cpu)","fp32",Some(1000),Some(900),vec![])
        ]});
        for tier in [
            "gpu-modern",
            "gpu-low",
            "gpu-pascal",
            "apple-arm",
            "cpu-low",
            "cpu-mid",
            "cpu-high",
        ] {
            m["bundles"][tier] = json!({"asr":if tier=="gpu-modern" {"asr-wav2vec_ctc-1b-amh-fp16"} else if tier=="cpu-low" {"asr-wav2vec_ctc-300m-amh-int8_onnx"} else {"asr-wav2vec_ctc-1b-amh-int8_onnx"},
                "asr_live":if tier=="gpu-modern" {"asr-wav2vec_ctc-1b-amh-fp16"} else {"asr-wav2vec_ctc-300m-amh-int8_onnx"},"corrector":"textcorrect-char_bart-7m-amh-int8_onnx",
                "tts":if tier=="gpu-pascal" {Some("tts-codec_llm-0.6b-amh-fp32")} else if tier.starts_with("gpu-") {Some("tts-codec_llm-0.6b-amh-fp16")} else {None},
                "tts_optin":if tier=="cpu-high" {Some("tts-codec_llm-0.6b-amh-int8_onnx")} else {None},"tts_num_step":if tier=="gpu-modern" {32} else {16}});
        }
        m
    }
    fn manifest() -> Manifest {
        Manifest::parse(&fixture().to_string()).unwrap()
    }
    #[test]
    fn informational_fields_survive_validation_without_becoming_executable_settings() {
        let mut source = fixture();
        source["research"] = json!({"measurement":"shared ARM host"});
        source["variants"][0]["notes"] = json!("Do not infer baseline hardware qualification.");
        source["variants"][0]["entry"] =
            json!("informational call description, not a shell command");
        let parsed = Manifest::parse(&source.to_string()).unwrap();
        let saved = serde_json::to_value(&parsed).unwrap();
        assert_eq!(saved["research"], source["research"]);
        assert_eq!(
            saved["variants"][0]["notes"],
            source["variants"][0]["notes"]
        );
        let selected = serde_json::to_value(
            parsed
                .select(&hardware(), Task::Transcription, Preference::Automatic)
                .unwrap(),
        )
        .unwrap();
        assert!(selected.get("entry").is_none());
    }
    #[test]
    fn apple_arm_uses_distinct_asr_models_for_files_and_live_with_explicit_identity() {
        let m = manifest();
        let h = hardware();
        let file = m
            .select(&h, Task::Transcription, Preference::Automatic)
            .unwrap();
        let live = m
            .select(&h, Task::Dictation, Preference::Automatic)
            .unwrap();
        assert_eq!(
            file.variant.as_deref(),
            Some("asr-wav2vec_ctc-1b-amh-int8_onnx")
        );
        assert_eq!(
            live.variant.as_deref(),
            Some("asr-wav2vec_ctc-300m-amh-int8_onnx")
        );
        assert_eq!(file.model.as_deref(), Some("amharic-ctc"));
        assert_eq!(live.model.as_deref(), Some("amharic-ctc-300m"));
        assert_eq!(file.model_version.as_deref(), Some("v0.3"));
        assert!(m
            .select(&h, Task::Tts { opt_in: false }, Preference::Automatic)
            .unwrap()
            .variant
            .is_none());
    }
    #[test]
    fn windows_arm_is_not_mistaken_for_x86_without_avx2() {
        let mut h = hardware();
        h.os = "windows".into();
        h.total_ram_bytes = 4 * 1024 * MIB;
        h.physical_cores = Some(2);
        assert_eq!(tier(&h), Tier::AppleArm);
    }
    #[test]
    fn cpu_rules_use_physical_cores_and_require_avx2_on_x86() {
        let mut h = hardware();
        h.os = "windows".into();
        h.arch = "x86_64".into();
        h.avx2 = Some(true);
        assert_eq!(tier(&h), Tier::CpuHigh);
        h.avx2 = Some(false);
        assert_eq!(tier(&h), Tier::CpuLow);
        h.avx2 = Some(true);
        h.physical_cores = Some(3);
        assert_eq!(tier(&h), Tier::CpuLow);
        h.physical_cores = Some(4);
        assert_eq!(tier(&h), Tier::CpuMid);
        h.physical_cores = None;
        assert_eq!(tier(&h), Tier::CpuLow);
    }
    #[test]
    fn gpu_tiers_respect_compute_and_vram_boundaries_and_old_cards_fall_back() {
        let mut h = hardware();
        h.os = "windows".into();
        h.arch = "x86_64".into();
        h.avx2 = Some(true);
        h.nvidia = vec![Nvidia {
            index: 0,
            compute_major: 7,
            compute_minor: 5,
            total_vram_bytes: 6 * 1024 * MIB,
            free_vram_bytes: 6 * 1024 * MIB,
        }];
        assert_eq!(tier(&h), Tier::GpuModern);
        h.nvidia[0].total_vram_bytes = 4 * 1024 * MIB;
        assert_eq!(tier(&h), Tier::GpuLow);
        h.nvidia[0].compute_major = 6;
        h.nvidia[0].total_vram_bytes = 6 * 1024 * MIB;
        assert_eq!(tier(&h), Tier::GpuPascal);
        h.nvidia[0].total_vram_bytes = 4 * 1024 * MIB;
        assert_eq!(tier(&h), Tier::CpuHigh);
        h.nvidia[0].compute_major = 5;
        h.nvidia[0].total_vram_bytes = 12 * 1024 * MIB;
        assert_eq!(tier(&h), Tier::CpuHigh);
    }
    #[test]
    fn memory_budget_falls_back_then_disables_instead_of_ignoring_limits() {
        let m = manifest();
        let mut h = hardware();
        h.free_ram_bytes = 3 * 1024 * MIB;
        let result = m
            .select(&h, Task::Transcription, Preference::MoreAccurate)
            .unwrap();
        assert_eq!(
            result.variant.as_deref(),
            Some("asr-wav2vec_ctc-300m-amh-int8_onnx")
        );
        assert!(result.reason.contains("smaller"));
        h.free_ram_bytes = 1024 * MIB;
        assert!(m
            .select(&h, Task::Dictation, Preference::Automatic)
            .unwrap()
            .variant
            .is_none());
    }
    #[test]
    fn faster_override_changes_only_choice_and_still_obeys_memory() {
        let m = manifest();
        let mut h = hardware();
        assert_eq!(
            m.select(&h, Task::Transcription, Preference::Faster)
                .unwrap()
                .model
                .as_deref(),
            Some("amharic-ctc-300m")
        );
        h.free_ram_bytes = 0;
        assert!(m
            .select(&h, Task::Transcription, Preference::Faster)
            .unwrap()
            .variant
            .is_none());
    }
    #[test]
    fn cpu_tts_requires_opt_in_counts_codec_and_enforces_quality_floor() {
        let m = manifest();
        let mut h = hardware();
        h.os = "windows".into();
        h.arch = "x86_64".into();
        h.avx2 = Some(true);
        assert!(m
            .select(&h, Task::Tts { opt_in: false }, Preference::Automatic)
            .unwrap()
            .variant
            .is_none());
        let tts = m
            .select(&h, Task::Tts { opt_in: true }, Preference::Automatic)
            .unwrap();
        assert_eq!(
            tts.downloads,
            vec![
                "codec-higgs_audio_v2-fp32",
                "tts-codec_llm-0.6b-amh-int8_onnx"
            ]
        );
        assert_eq!(tts.download_bytes, 20);
        assert!(tts.codec_fp32);
        assert_eq!(tts.tts_num_step, Some(16));
        h.free_ram_bytes = 6 * 1024 * MIB;
        assert!(m
            .select(&h, Task::Tts { opt_in: true }, Preference::Automatic)
            .unwrap()
            .variant
            .is_none());
        let mut invalid = fixture();
        invalid["bundles"]["gpu-modern"]["tts_num_step"] = json!(8);
        assert!(Manifest::parse(&invalid.to_string()).is_err());
    }
    #[test]
    fn pascal_never_selects_fp16_and_gpu_codec_stays_fp32() {
        let m = manifest();
        let mut h = hardware();
        h.os = "windows".into();
        h.arch = "x86_64".into();
        h.avx2 = Some(true);
        h.nvidia = vec![Nvidia {
            index: 0,
            compute_major: 6,
            compute_minor: 1,
            total_vram_bytes: 12 * 1024 * MIB,
            free_vram_bytes: 12 * 1024 * MIB,
        }];
        let tts = m
            .select(&h, Task::Tts { opt_in: false }, Preference::Automatic)
            .unwrap();
        assert_eq!(tts.variant.as_deref(), Some("tts-codec_llm-0.6b-amh-fp32"));
        assert!(tts.pascal_fp32 && tts.codec_fp32);
        let mut invalid = fixture();
        invalid["bundles"]["gpu-pascal"]["tts"] = json!("tts-codec_llm-0.6b-amh-fp16");
        let invalid = Manifest::parse(&invalid.to_string()).unwrap();
        assert!(invalid
            .select(&h, Task::Tts { opt_in: false }, Preference::Automatic)
            .unwrap()
            .variant
            .is_none());
    }
    #[test]
    fn unsafe_incomplete_duplicate_or_cyclic_manifests_are_rejected() {
        for case in 0..7 {
            let mut m = fixture();
            match case {
                0 => m["variants"][0]["files"][0]["path"] = json!("../escape"),
                1 => m["variants"][0]["total_bytes"] = json!(11),
                2 => m["variants"][0]["model_version"] = Value::Null,
                3 => {
                    let duplicate = m["variants"][0].clone();
                    m["variants"].as_array_mut().unwrap().push(duplicate);
                }
                4 => m["variants"][0]["requires"] = json!(["asr-wav2vec_ctc-1b-amh-int8_onnx"]),
                5 => m["variants"][0]["requires"] = json!(["missing"]),
                _ => m["variants"][0]["license"] = json!(""),
            }
            assert!(Manifest::parse(&m.to_string()).is_err(), "case {case}");
        }
    }
    #[test]
    fn missing_small_tts_stays_unavailable_and_does_not_fabricate_a_download() {
        let mut m = fixture();
        m["bundles"]["apple-arm"]["tts"] = json!("tts-flow_lm-90m-amh-fp32");
        let m = Manifest::parse(&m.to_string()).unwrap();
        let result = m
            .select(
                &hardware(),
                Task::Tts { opt_in: false },
                Preference::Automatic,
            )
            .unwrap();
        assert!(result.variant.is_none() && result.downloads.is_empty());
        assert!(result.reason.contains("not available"));
    }
    #[test]
    fn hosted_manifest_uses_nested_paths_at_a_pinned_commit_only() {
        let mut m = fixture();
        m["hf_repo"] = json!("ethiopic/ethiopic-studio-models");
        m["hf_revision"] = json!("a".repeat(40));
        let parsed = Manifest::parse(&m.to_string()).unwrap();
        assert_eq!(parsed.file_url("asr-wav2vec_ctc-1b-amh-int8_onnx","weights.onnx").unwrap(),format!("https://huggingface.co/ethiopic/ethiopic-studio-models/resolve/{}/asr-wav2vec_ctc-1b-amh-int8_onnx/weights.onnx","a".repeat(40)));
        assert!(parsed
            .file_url("asr-wav2vec_ctc-1b-amh-int8_onnx", "not-declared.py")
            .is_err());
        m["hf_revision"] = json!("main");
        assert!(Manifest::parse(&m.to_string()).is_err());
    }
    #[test]
    fn cached_model_file_reuses_the_verified_transport_without_network() {
        use sha2::{Digest, Sha256};
        let bytes = b"model-data";
        let sha = format!("{:x}", Sha256::digest(bytes));
        let mut m = fixture();
        m["hf_repo"] = json!("ethiopic/ethiopic-studio-models");
        m["hf_revision"] = json!("a".repeat(40));
        m["variants"][0]["files"][0]["sha256"] = json!(sha.to_uppercase());
        let m = Manifest::parse(&m.to_string()).unwrap();
        let temp = Temp::new();
        let directory = temp.0.join(&sha);
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("download.tar.gz"), bytes).unwrap();
        let path = m
            .download_file(
                "asr-wav2vec_ctc-1b-amh-int8_onnx",
                "weights.onnx",
                &temp.0,
                &std::sync::atomic::AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
