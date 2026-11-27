//! Pure setup routing. This does not inspect the host, download files or execute models.
use crate::{
    studio::{tier, EditingSelection, Hardware, Manifest, Selection, Task, Tier},
    Result,
};
use serde::Serialize;
use std::collections::HashSet;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingPlan {
    pub tier: Tier,
    pub transcription: Selection,
    pub dictation: Selection,
    pub speech: Selection,
    pub editing: EditingSelection,
    pub downloads: Vec<String>,
    pub download_bytes: u64,
    pub warnings: Vec<String>,
}
/// Models for everyday features are selected automatically; editing remains lazy/explicit.
pub fn route(
    m: &Manifest,
    h: &Hardware,
    include_editing: bool,
    slow_editing: bool,
) -> Result<RoutingPlan> {
    if h.total_ram_bytes == 0 {
        return Err(
            "Could not read this computer's memory. Check hardware again before downloading."
                .into(),
        );
    }
    let transcription = m.select_for_setup(h, Task::Transcription)?;
    let dictation = m.select_for_setup(h, Task::Dictation)?;
    let speech = m.select_for_setup(h, Task::Tts { opt_in: false })?;
    let mut capacity = h.clone();
    capacity.free_ram_bytes = capacity.total_ram_bytes;
    for gpu in &mut capacity.nvidia {
        gpu.free_vram_bytes = gpu.total_vram_bytes;
    }
    let editing = m.select_editing(&capacity, slow_editing)?;
    let mut seen = HashSet::new();
    let mut downloads = Vec::new();
    for id in transcription
        .downloads
        .iter()
        .chain(&dictation.downloads)
        .chain(&speech.downloads)
        .chain(if include_editing && editing.available {
            editing.downloads.iter()
        } else {
            [].iter()
        })
    {
        if seen.insert(id.clone()) {
            downloads.push(id.clone());
        }
    }
    let download_bytes = downloads
        .iter()
        .filter_map(|id| m.find(id))
        .map(|v| v.total_bytes)
        .sum();
    let largest_cpu = downloads
        .iter()
        .filter_map(|id| m.find(id))
        .filter_map(|v| v.ram_mb_est)
        .max()
        .unwrap_or(0)
        * 1024
        * 1024;
    let mut warnings = Vec::new();
    if largest_cpu > h.free_ram_bytes.min(h.total_ram_bytes) / 10 * 7 {
        warnings.push("Other apps are using memory. Setup can continue; close unused apps before processing speech.".into());
    }
    if h.nvidia
        .iter()
        .any(|g| g.free_vram_bytes < g.total_vram_bytes / 2)
    {
        warnings.push("The GPU is busy. Close other GPU tasks before generating speech.".into());
    }
    if h.physical_cores.is_none() {
        warnings.push("CPU details could not be read completely. A conservative CPU profile is used where needed.".into());
    }
    Ok(RoutingPlan {
        tier: tier(h),
        transcription,
        dictation,
        speech,
        editing,
        downloads,
        download_bytes,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Nvidia;
    fn manifest() -> Manifest {
        Manifest::parse(include_str!(
            "../../../apps/desktop/resources/studio-manifest.json"
        ))
        .unwrap()
    }
    #[test]
    fn fresh_v2_inventory_routes_every_tier_to_canonical_ids_and_complete_dependencies() {
        let m = manifest();
        let mut apple = cpu(16, Some(8), None);
        apple.os = "macos".into();
        apple.arch = "aarch64".into();
        let mut low = gpu(7, 5);
        low.nvidia[0].compute_minor = 5;
        for (expected, h) in [
            ("gpu-modern", gpu(8, 8)),
            ("gpu-low", low),
            ("gpu-pascal", gpu(6, 8)),
            ("cpu-high", cpu(32, Some(8), Some(true))),
            ("cpu-mid", cpu(8, Some(4), Some(true))),
            ("cpu-low", cpu(4, Some(2), Some(false))),
            ("apple-arm", apple),
        ] {
            let plan = route(&m, &h, true, true).unwrap();
            assert_eq!(plan.tier.name(), expected);
            for id in &plan.downloads {
                assert!(!m.aliases.contains_key(id));
                assert_eq!(m.resolve(id), Some(id.as_str()));
            }
            assert!(plan.transcription.model_version.as_deref() == Some("v1.0"));
            assert!(plan.speech.variant.is_some());
            if m.bundles[expected].edit.is_some() || m.bundles[expected].edit_optin.is_some() {
                assert!(
                    plan.editing.available,
                    "{expected}: {}",
                    plan.editing.reason
                );
                assert!(plan
                    .downloads
                    .iter()
                    .any(|id| m.find(id).unwrap().component == "tts-codec"));
                assert!(plan
                    .downloads
                    .iter()
                    .any(|id| id == "asr-wav2vec_ctc-1b-amh-int8_onnx"));
            } else {
                assert!(!plan.editing.available);
            }
        }
    }
    fn cpu(ram: u64, cores: Option<u32>, avx: Option<bool>) -> Hardware {
        Hardware {
            os: "windows".into(),
            arch: "x86_64".into(),
            total_ram_bytes: ram * 1024 * 1024 * 1024,
            free_ram_bytes: ram * 1024 * 1024 * 1024,
            physical_cores: cores,
            avx2: avx,
            nvidia: vec![],
        }
    }
    fn gpu(major: u32, gib: u64) -> Hardware {
        let mut h = cpu(32, Some(8), Some(true));
        h.nvidia.push(Nvidia {
            index: 0,
            compute_major: major,
            compute_minor: 5,
            total_vram_bytes: gib * 1024 * 1024 * 1024,
            free_vram_bytes: gib * 1024 * 1024 * 1024,
        });
        h
    }
    #[test]
    fn cpu_profiles_select_small_models_without_an_eight_gb_minimum() {
        let m = manifest();
        for (ram, cores, avx, expected) in [
            (
                4,
                Some(2),
                Some(false),
                "asr-wav2vec_ctc-300m-amh-int8_onnx",
            ),
            (8, Some(4), Some(true), "asr-wav2vec_ctc-1b-amh-int8_onnx"),
            (16, Some(8), Some(true), "asr-wav2vec_ctc-1b-amh-int8_onnx"),
            (
                16,
                Some(8),
                Some(false),
                "asr-wav2vec_ctc-300m-amh-int8_onnx",
            ),
        ] {
            let plan = route(&m, &cpu(ram, cores, avx), false, false).unwrap();
            assert_eq!(plan.transcription.variant.as_deref(), Some(expected));
            assert_eq!(
                plan.dictation.variant.as_deref(),
                Some("asr-wav2vec_ctc-300m-amh-int8_onnx")
            );
            assert_eq!(
                plan.speech.variant.as_deref(),
                Some("tts-flow_lm-90m-amh-fp32")
            );
            assert!(!plan
                .downloads
                .iter()
                .any(|id| id == "edit-codec_llm-0.6b-amh"));
        }
    }
    #[test]
    fn apple_cpu_defaults_are_fp32_pocket_and_onnx_asr() {
        let m = manifest();
        let mut h = cpu(8, Some(4), None);
        h.os = "macos".into();
        h.arch = "aarch64".into();
        let p = route(&m, &h, false, false).unwrap();
        assert_eq!(p.tier, Tier::AppleArm);
        assert_eq!(
            p.transcription.variant.as_deref(),
            Some("asr-wav2vec_ctc-1b-amh-int8_onnx")
        );
        assert_eq!(
            p.speech.variant.as_deref(),
            Some("tts-flow_lm-90m-amh-fp32")
        );
        assert!(!p.editing.available);
        assert!(p
            .speech
            .downloads
            .iter()
            .all(|id| !id.contains("omnivoice")));
    }
    #[test]
    fn supported_gpu_routes_preserve_pascal_precision_and_single_dependency_alternatives() {
        let m = manifest();
        for (major, vram, tts) in [
            (8, 8, "tts-codec_llm-0.6b-amh-fp16"),
            (7, 5, "tts-codec_llm-0.6b-amh-fp16"),
            (6, 6, "tts-codec_llm-0.6b-amh-fp32"),
        ] {
            let p = route(&m, &gpu(major, vram), false, false).unwrap();
            assert_eq!(p.speech.variant.as_deref(), Some(tts));
            assert!(p.speech.codec_fp32);
            assert!(p.downloads.contains(&"codec-higgs_audio_v2-fp32".into()));
            assert_eq!(
                p.downloads
                    .iter()
                    .filter(|id| id.starts_with("tts-codec_llm-0.6b-amh-"))
                    .count(),
                1
            );
            assert_eq!(
                p.downloads
                    .iter()
                    .filter(|id| *id == "codec-higgs_audio_v2-fp32")
                    .count(),
                1
            );
        }
    }
    #[test]
    fn four_gb_gpu_and_unsupported_gpu_fall_back_to_cpu_instead_of_fetching_unusable_weights() {
        let m = manifest();
        for h in [gpu(7, 4), gpu(5, 8)] {
            let p = route(&m, &h, false, false).unwrap();
            assert_eq!(
                p.speech.variant.as_deref(),
                Some("tts-flow_lm-90m-amh-fp32")
            );
            assert!(!p
                .downloads
                .iter()
                .any(|id| id.starts_with("tts-omnivoice") || id == "codec-higgs_audio_v2-fp32"));
        }
    }
    #[test]
    fn temporary_memory_pressure_changes_warnings_not_the_download_plan() {
        let m = manifest();
        let mut h = cpu(4, Some(4), Some(true));
        let before = route(&m, &h, false, false).unwrap();
        h.free_ram_bytes = 128 * 1024 * 1024;
        let after = route(&m, &h, false, false).unwrap();
        assert_eq!(before.downloads, after.downloads);
        assert_eq!(
            after.transcription.variant.as_deref(),
            Some("asr-wav2vec_ctc-300m-amh-int8_onnx")
        );
        assert!(!after.warnings.is_empty());
    }
    #[test]
    fn dependency_totals_are_deduplicated_and_editing_is_explicit() {
        let m = manifest();
        let h = cpu(32, Some(8), Some(true));
        let ordinary = route(&m, &h, false, false).unwrap();
        let editing = route(&m, &h, true, true).unwrap();
        assert!(!ordinary
            .downloads
            .contains(&"edit-codec_llm-0.6b-amh".into()));
        assert!(editing
            .downloads
            .contains(&"edit-codec_llm-0.6b-amh".into()));
        let ids: HashSet<_> = editing.downloads.iter().collect();
        assert_eq!(ids.len(), editing.downloads.len());
        assert_eq!(
            editing.download_bytes,
            ids.iter()
                .map(|id| m.find(id).unwrap().total_bytes)
                .sum::<u64>()
        );
    }
    #[test]
    fn unknown_memory_stops_selection_and_unknown_cpu_uses_the_conservative_profile() {
        let m = manifest();
        assert!(route(&m, &cpu(0, None, None), false, false).is_err());
        let p = route(&m, &cpu(8, None, None), false, false).unwrap();
        assert_eq!(p.tier, Tier::CpuLow);
        assert!(!p.warnings.is_empty());
    }
}
