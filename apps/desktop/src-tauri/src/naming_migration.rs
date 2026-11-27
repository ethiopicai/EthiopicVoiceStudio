//! Upgrade only app-owned model metadata/cache paths. The source model release is never modified.
use resource_packs::{naming, studio::Manifest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
const ERROR: &str = "The model naming update could not finish. Your recordings are safe. Check voice tools to retry.";

fn safe(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path.strip_prefix(root).map_err(|_| ERROR)?;
    let mut current = root.to_path_buf();
    if current.is_symlink() {
        return Err(ERROR.into());
    }
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(ERROR.into());
        }
        current.push(part);
        if current.is_symlink() {
            return Err(ERROR.into());
        }
    }
    Ok(())
}
fn read(path: &Path) -> Result<Value, String> {
    let info = fs::symlink_metadata(path).map_err(|_| ERROR)?;
    if !info.is_file() || info.file_type().is_symlink() || info.len() > 32 * 1024 * 1024 {
        return Err(ERROR.into());
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| ERROR)?).map_err(|_| ERROR.into())
}
fn write(root: &Path, path: &Path, value: &Value) -> Result<(), String> {
    safe(root, path)?;
    let pending = path.with_extension("naming-pending");
    safe(root, &pending)?;
    fs::write(
        &pending,
        serde_json::to_vec_pretty(value).map_err(|_| ERROR)?,
    )
    .map_err(|_| ERROR)?;
    fs::rename(pending, path).map_err(|_| ERROR.into())
}
fn hash(path: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|_| ERROR)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("paused".into());
        }
        let n = file.read(&mut buffer).map_err(|_| ERROR)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn signature(m: &Manifest) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&m.variants).unwrap_or_default())
    )
}

/// Renames are atomic and resumable. Changed small files come from this app's checksummed bridge;
/// no network or weight download occurs. Successful SHA checks use the normal metadata-bound cache.
pub(crate) fn models(directory: &Path, m: &Manifest, cancel: &AtomicBool) -> Result<bool, String> {
    if !directory.exists() {
        return Ok(false);
    }
    safe(directory, directory)?;
    let mut changed = false;
    for (old, new) in &m.aliases {
        let from = directory.join(old);
        let to = directory.join(new);
        safe(directory, &from)?;
        safe(directory, &to)?;
        if from.exists() && !to.exists() {
            if !from.is_dir() {
                return Err(ERROR.into());
            }
            fs::rename(from, to).map_err(|_| ERROR)?;
            changed = true;
        }
    }
    for variant in &m.variants {
        let folder = directory.join(&variant.folder);
        if !folder.exists() {
            continue;
        }
        let mut entries = vec![];
        for file in &variant.files {
            let path = folder.join(&file.path);
            safe(directory, &path)?;
            if let Some(bytes) = naming::bundled_file(m, &variant.id, &file.path) {
                if !path.is_file() || hash(&path, cancel)? != file.sha256 {
                    if bytes.len() as u64 != file.bytes
                        || format!("{:x}", Sha256::digest(bytes)) != file.sha256
                    {
                        return Err(ERROR.into());
                    }
                    let pending = path.with_extension("naming-pending");
                    safe(directory, &pending)?;
                    fs::write(&pending, bytes).map_err(|_| ERROR)?;
                    fs::rename(pending, &path).map_err(|_| ERROR)?;
                    changed = true;
                }
            }
            // A paused download can already have an allocated, incomplete variant folder.
            // Preserve it for normal resumable setup; this receipt does not mark models ready.
            if path.exists() {
                entries.push((
                    format!("{}/{}", variant.folder, file.path),
                    file.sha256.clone(),
                ));
            }
        }
        let receipts = directory.join("naming-receipts");
        safe(directory, &receipts)?;
        fs::create_dir_all(&receipts).map_err(|_| ERROR)?;
        if !entries.is_empty() {
            crate::resource_verification::verify(
                directory,
                &entries
                    .iter()
                    .map(|(p, h)| (p.as_str(), h.as_str()))
                    .collect::<Vec<_>>(),
                &receipts.join(format!("{}.json", variant.id)),
                cancel,
            )?;
        }
        // variant.json is a local descriptor, not a file fetched from the model owner's folders.
        let descriptor = folder.join("variant.json");
        let expected = serde_json::to_value(variant).map_err(|_| ERROR)?;
        if read(&descriptor).ok().as_ref() != Some(&expected) {
            write(directory, &descriptor, &expected)?;
            changed = true;
        }
    }
    for name in ["verified.json", "tts-verified.json"] {
        let path = directory.join(name);
        safe(directory, &path)?;
        if path.is_file() {
            let mut value = read(&path)?;
            if naming::normalize_metadata(&mut value) {
                write(directory, &path, &value)?;
                changed = true;
            }
        }
    }
    let descriptor = directory.join("manifest.json");
    if descriptor.exists() {
        write(
            directory,
            &descriptor,
            &serde_json::to_value(m).map_err(|_| ERROR)?,
        )?;
    }
    // Download caches are keyed by content SHA and remain reusable. Verification
    // receipts include paths: discard only obsolete app receipts and let the normal
    // integrity check recreate their canonical keys, rather than adopting stale proof.
    let receipts = directory.join("receipts");
    safe(directory, &receipts)?;
    if receipts.is_dir() {
        for entry in fs::read_dir(&receipts).map_err(|_| ERROR)? {
            let path = entry.map_err(|_| ERROR)?.path();
            safe(directory, &path)?;
            let known_name = path.extension().and_then(|v| v.to_str()) == Some("json")
                && path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()));
            if !known_name {
                continue;
            }
            let obsolete = read(&path)
                .ok()
                .and_then(|v| v["files"].as_array().cloned())
                .is_some_and(|files| {
                    files.iter().any(|f| {
                        f["resolved"].as_str().is_some_and(|p| {
                            p.replace('\\', "/")
                                .split('/')
                                .any(|part| m.aliases.contains_key(part))
                        })
                    })
                });
            if obsolete {
                fs::remove_file(&path).map_err(|_| ERROR)?;
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// Used for both a staged old archive and an existing installed recognition bundle.
pub(crate) fn recognition(base: &Path, m: &Manifest, cancel: &AtomicBool) -> Result<(), String> {
    let config_path = base.join("installation.json");
    if !config_path.exists() {
        return Ok(());
    }
    safe(base, &config_path)?;
    let mut config = read(&config_path)?;
    let bundle = base.join("bundle");
    safe(base, &bundle)?;
    let profile_path = bundle.join("recognition.json");
    safe(base, &profile_path)?;
    if !profile_path.exists() {
        // Earlier Mac archives have a flat 1B PyTorch bundle. Verify exact unchanged model
        // hashes before assigning the new release; older/different weights keep their provenance.
        let raw = read(&bundle.join("model/version.json"))?;
        if raw["model_version"] != "v0.3" {
            return Ok(());
        }
        let variant = m.find("asr-wav2vec_ctc-1b-amh-fp16").ok_or(ERROR)?;
        let entries: Vec<_> = variant
            .files
            .iter()
            .filter(|f| f.path.starts_with("model/") || f.path.starts_with("omniasr_hf/"))
            .map(|f| (f.path.as_str(), f.sha256.as_str()))
            .collect();
        let receipts = bundle.join("naming-receipts");
        safe(base, &receipts)?;
        fs::create_dir_all(&receipts).map_err(|_| ERROR)?;
        crate::resource_verification::verify(
            &bundle,
            &entries,
            &receipts.join("legacy-release.json"),
            cancel,
        )?;
        let release = json!({"schema_version":1,"variant":variant.id,"model_version":variant.model_version,"weight_version":"v0.3"});
        write(base, &bundle.join("release.json"), &release)?;
        config["model_version"] = release["model_version"].clone();
        let files = config["files"].as_array_mut().ok_or(ERROR)?;
        files.retain(|f| f["path"] != "bundle/release.json");
        files.push(json!({"path":"bundle/release.json","sha256":hash(&bundle.join("release.json"),cancel)?}));
        return write(base, &config_path, &config);
    }
    let mut profile = read(&profile_path)?;
    naming::normalize_metadata(&mut profile);
    models(&bundle, m, cancel)?;
    for task in ["transcription", "dictation"] {
        let id = profile[task]["variant"].as_str().ok_or(ERROR)?;
        let variant = m.find(id).ok_or(ERROR)?;
        profile[task]["folder"] = json!(variant.folder);
        profile[task]["model_version"] = json!(variant.model_version);
    }
    write(base, &profile_path, &profile)?;
    naming::normalize_metadata(&mut config);
    config["model_version"] = profile["transcription"]["model_version"].clone();
    let files = config["files"].as_array_mut().ok_or(ERROR)?;
    for file in files {
        let relative = file["path"].as_str().ok_or(ERROR)?.to_owned();
        let path = base.join(&relative);
        safe(base, &path)?;
        if relative == "bundle/recognition.json" || relative.ends_with("/variant.json") {
            file["sha256"] = json!(hash(&path, cancel)?);
        } else if let Some(rest) = relative.strip_prefix("bundle/") {
            if let Some((id, inner)) = rest.split_once('/') {
                if let Some(expected) = m
                    .find(id)
                    .and_then(|v| v.files.iter().find(|f| f.path == inner))
                {
                    file["sha256"] = json!(expected.sha256);
                }
            }
        }
    }
    write(base, &config_path, &config)?;
    Ok(())
}

pub(crate) fn legacy_version(bundle: &Path, raw: &Value) -> Option<String> {
    let marker = read(&bundle.join("release.json")).ok()?;
    let m = Manifest::parse(include_str!(
        "../../resources/naming-migration/manifest-v2.json"
    ))
    .ok()?;
    let variant = m.find(marker["variant"].as_str()?)?;
    (raw["model_version"] == "v0.3"
        && marker["weight_version"] == "v0.3"
        && marker["schema_version"] == 1
        && variant.id == "asr-wav2vec_ctc-1b-amh-fp16"
        && marker["model_version"].as_str() == variant.model_version.as_deref())
    .then(|| variant.model_version.clone())
    .flatten()
}

fn metadata(root: &Path, relative: &str) -> Result<(), String> {
    let path = root.join(relative);
    safe(root, &path)?;
    if path.is_file() {
        let mut value = read(&path)?;
        if naming::normalize_metadata(&mut value) {
            write(root, &path, &value)?;
        }
    }
    Ok(())
}

fn project_models(value: &mut Value) -> bool {
    let mut changed = false;
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if [
                    "model", "model_id", "modelId", "provider", "tts", "editor", "aligner",
                ]
                .contains(&key.as_str())
                {
                    changed |= naming::normalize_metadata(value);
                } else if value.is_object() || value.is_array() {
                    changed |= project_models(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                changed |= project_models(value);
            }
        }
        _ => {}
    }
    changed
}

pub(crate) fn run(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    run_naming(root, cancel)?;
    publication(root, cancel)
}

fn publication(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    let current = Manifest::parse(include_str!("../../resources/studio-manifest.json"))?;
    let previous = Manifest::parse(include_str!(
        "../../resources/naming-migration/manifest-v2.json"
    ))?;
    publication_from(root, cancel, &current, &previous)
}

fn publication_from(
    root: &Path,
    cancel: &AtomicBool,
    current: &Manifest,
    previous: &Manifest,
) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        return Err("paused".into());
    }
    if current.hf_revision == previous.hf_revision {
        return Ok(());
    }
    safe(root, root)?;
    let receipt = root.join("publication-migration-v1.json");
    safe(root, &receipt)?;
    let expected = format!(
        "{}:{}",
        current.hf_revision.as_deref().unwrap_or_default(),
        signature(current)
    );
    if read(&receipt)
        .ok()
        .is_some_and(|v| v["signature"] == expected && v["complete"] == true)
    {
        return Ok(());
    }
    crate::asr_runtime::release_for_setup()?;
    let asr = root.join("asr/active");
    safe(root, &asr)?;
    if read(&asr.join("installation.json"))
        .ok()
        .is_some_and(|v| v["model_version"] == "v0.3" || v["model_version"] == "v1.0")
    {
        recognition(&asr, current, cancel)?;
    }
    let parent = root.join("voice-edit/models");
    let old = parent.join(previous.hf_revision.as_deref().ok_or(ERROR)?);
    let new = parent.join(current.hf_revision.as_deref().ok_or(ERROR)?);
    safe(root, &old)?;
    safe(root, &new)?;
    if old.is_dir() {
        // Both releases have identical weights. Repair only checksummed support files,
        // then adopt the revision directory without downloading or copying weights.
        if !new.exists() {
            models(&old, current, cancel)?;
            fs::rename(&old, &new).map_err(|_| ERROR)?;
        } else {
            // Resume an interrupted adoption; never overwrite an existing target file.
            for variant in &current.variants {
                let source = old.join(&variant.folder);
                let target = new.join(&variant.folder);
                safe(root, &source)?;
                safe(root, &target)?;
                if source.is_dir() && !target.exists() {
                    fs::rename(source, target).map_err(|_| ERROR)?;
                } else if source.is_dir() && target.is_dir() {
                    for f in &variant.files {
                        let from = source.join(&f.path);
                        let to = target.join(&f.path);
                        safe(root, &from)?;
                        safe(root, &to)?;
                        if from.is_file() && !to.exists() {
                            fs::create_dir_all(to.parent().ok_or(ERROR)?).map_err(|_| ERROR)?;
                            fs::hard_link(from, to).map_err(|_| ERROR)?;
                        }
                    }
                }
            }
            models(&new, current, cancel)?;
        }
        write(
            root,
            &new.join("manifest.json"),
            &serde_json::to_value(current).map_err(|_| ERROR)?,
        )?;
        let verified = new.join("verified.json");
        if let Ok(mut value) = read(&verified) {
            if value["revision"].as_str() == previous.hf_revision.as_deref() {
                value["revision"] = json!(current.hf_revision);
                write(root, &verified, &value)?;
            }
        }
    }
    // Never reinterpret unrelated releases. Cached manifests from this known cohort
    // are replaced only after adoption; project transcripts and inference versions stay intact.
    let cached = root.join("voice-edit/manifest.json");
    safe(root, &cached)?;
    if read(&cached)
        .ok()
        .is_some_and(|v| v["hf_revision"].as_str() == previous.hf_revision.as_deref())
    {
        write(
            root,
            &cached,
            &serde_json::to_value(current).map_err(|_| ERROR)?,
        )?;
    }
    write(
        root,
        &receipt,
        &json!({"signature":expected,"complete":true}),
    )
}

fn run_naming(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    safe(root, root)?;
    let m = Manifest::parse(include_str!(
        "../../resources/naming-migration/manifest-v2.json"
    ))?;
    let receipt = root.join("naming-migration-v2.json");
    safe(root, &receipt)?;
    let expected = signature(&m);
    if read(&receipt)
        .ok()
        .is_some_and(|v| v["schema"] == 2 && v["signature"] == expected && v["complete"] == true)
    {
        return Ok(());
    }
    crate::asr_runtime::release_for_setup()?;
    safe(root, &root.join("asr/active"))?;
    let config = read(&root.join("asr/active/installation.json")).ok();
    if config
        .as_ref()
        .is_some_and(|v| v["model_version"] == "v0.3" || v["model_version"] == "v1.0")
    {
        recognition(&root.join("asr/active"), &m, cancel)?;
    }
    let parent = root.join("voice-edit/models");
    safe(root, &parent)?;
    if parent.is_dir() {
        // Flat developer installs and revision-keyed managed installs are both supported.
        let legacy_flat = m.aliases.keys().any(|id| parent.join(id).exists())
            || read(&parent.join("manifest.json"))
                .ok()
                .is_some_and(|v| v["hf_revision"].as_str() == m.hf_revision.as_deref());
        if legacy_flat {
            models(&parent, &m, cancel)?;
        }
        for entry in fs::read_dir(&parent).map_err(|_| ERROR)? {
            let path = entry.map_err(|_| ERROR)?.path();
            safe(root, &path)?;
            // A naming migration must never verify unrelated/newer model caches against
            // this frozen release's weights.
            if path.is_dir()
                && path.file_name().and_then(|v| v.to_str()) == m.hf_revision.as_deref()
            {
                models(&path, &m, cancel)?;
            }
        }
    }
    for relative in [
        "voice-edit/generation-defaults.json",
        "voice-edit/preferences.json",
        "voice-edit/setup.json",
        "voice-setup-progress.json",
        "voice-tools-approval.json",
        "tts-runtime/active/installation.json",
        "studio-runtime/active/installation.json",
    ] {
        metadata(root, relative)?;
    }
    let projects = root.join("voice-edit/projects");
    safe(root, &projects)?;
    if projects.is_dir() {
        for entry in fs::read_dir(&projects).map_err(|_| ERROR)? {
            let folder = entry.map_err(|_| ERROR)?.path();
            safe(root, &folder)?;
            if !folder.is_dir() {
                continue;
            }
            for name in [
                "project.json",
                "synthesis-context.json",
                "speech-group.json",
                "edit-log.json",
            ] {
                let path = folder.join(name);
                safe(root, &path)?;
                if path.is_file() {
                    let mut value = read(&path)?;
                    if project_models(&mut value) {
                        write(root, &path, &value)?;
                    }
                }
            }
        }
    }
    let cached = root.join("voice-edit/manifest.json");
    safe(root, &cached)?;
    if read(&cached)
        .ok()
        .is_some_and(|v| v["manifest_version"] == 1)
    {
        write(root, &cached, &serde_json::to_value(&m).map_err(|_| ERROR)?)?;
    }
    write(
        root,
        &receipt,
        &json!({"schema":2,"signature":expected,"complete":true}),
    )
}

#[cfg(test)]
mod tests {
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn temporary() -> Temp {
        let path = std::env::temp_dir().join(format!(
            "ethiopic-naming-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Temp(path)
    }

    use super::*;
    fn manifest() -> Manifest {
        Manifest::parse(include_str!(
            "../../resources/naming-migration/manifest-v2.json"
        ))
        .unwrap()
    }
    fn tiny() -> Manifest {
        let mut m = manifest();
        for v in &mut m.variants {
            v.files = vec![resource_packs::studio::ModelFile {
                path: "weights.bin".into(),
                bytes: 6,
                sha256: format!("{:x}", Sha256::digest(b"weight")),
            }];
            v.total_bytes = 6;
        }
        m
    }
    fn seed(directory: &Path, m: &Manifest, old: bool) {
        fs::create_dir_all(directory).unwrap();
        for v in &m.variants {
            let name = if old {
                v.aliases.first().unwrap()
            } else {
                &v.id
            };
            let folder = directory.join(name);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("weights.bin"), b"weight").unwrap();
        }
    }
    #[test]
    fn publication_adopts_existing_weights_offline_and_preserves_history() {
        let temp = temporary();
        let previous = tiny();
        let mut current = previous.clone();
        current.hf_revision = Some("f".repeat(40));
        let root = temp.path();
        let parent = root.join("voice-edit/models");
        let old = parent.join(previous.hf_revision.as_ref().unwrap());
        seed(&old, &previous, false);
        let file = old.join(&previous.variants[0].folder).join("weights.bin");
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        let cached = root.join("voice-edit/manifest.json");
        fs::write(&cached, serde_json::to_vec(&previous).unwrap()).unwrap();
        let history = root.join("voice-edit/projects");
        fs::create_dir_all(&history).unwrap();
        fs::write(
            history.join("saved.json"),
            b"original text and version v0.3",
        )
        .unwrap();
        publication_from(root, &AtomicBool::new(false), &current, &previous).unwrap();
        assert!(!old.exists());
        let new = parent.join(current.hf_revision.as_ref().unwrap());
        let file = new.join(&previous.variants[0].folder).join("weights.bin");
        assert_eq!(fs::read(&file).unwrap(), b"weight");
        assert_eq!(fs::metadata(file).unwrap().modified().unwrap(), modified);
        assert_eq!(
            read(&cached).unwrap()["hf_revision"],
            json!(current.hf_revision)
        );
        assert_eq!(
            fs::read(history.join("saved.json")).unwrap(),
            b"original text and version v0.3"
        );
        publication_from(root, &AtomicBool::new(false), &current, &previous).unwrap();
    }
    #[test]
    fn publication_resumes_partial_adoption_without_overwriting_existing_files() {
        let temp = temporary();
        let previous = tiny();
        let mut current = previous.clone();
        current.hf_revision = Some("f".repeat(40));
        let parent = temp.path().join("voice-edit/models");
        let old = parent.join(previous.hf_revision.as_ref().unwrap());
        let new = parent.join(current.hf_revision.as_ref().unwrap());
        seed(&old, &previous, false);
        fs::create_dir_all(new.join(&current.variants[0].folder)).unwrap();
        publication_from(temp.path(), &AtomicBool::new(false), &current, &previous).unwrap();
        for v in &current.variants {
            assert_eq!(
                fs::read(new.join(&v.folder).join("weights.bin")).unwrap(),
                b"weight"
            );
        }
        let cancelled = temporary();
        assert_eq!(
            publication_from(
                cancelled.path(),
                &AtomicBool::new(true),
                &current,
                &previous
            )
            .unwrap_err(),
            "paused"
        );
        assert!(!cancelled
            .path()
            .join("publication-migration-v1.json")
            .exists());
    }
    #[test]
    fn upgrade_renames_all_twelve_variants_preserving_weight_bytes_and_timestamps() {
        let temp = temporary();
        let m = tiny();
        seed(temp.path(), &m, true);
        let before: Vec<_> = m
            .variants
            .iter()
            .map(|v| {
                fs::metadata(temp.path().join(&v.aliases[0]).join("weights.bin"))
                    .unwrap()
                    .modified()
                    .unwrap()
            })
            .collect();
        let receipts = temp.path().join("receipts");
        fs::create_dir(&receipts).unwrap();
        let obsolete = receipts.join(format!("{}.json", "a".repeat(64)));
        fs::write(&obsolete,serde_json::to_vec(&json!({"files":[{"resolved":format!("{}\\weights.bin",m.variants[0].aliases[0])}]})).unwrap()).unwrap();
        assert!(models(temp.path(), &m, &AtomicBool::new(false)).unwrap());
        assert!(!obsolete.exists());
        for (index, v) in m.variants.iter().enumerate() {
            assert!(!temp.path().join(&v.aliases[0]).exists());
            let file = temp.path().join(&v.id).join("weights.bin");
            assert_eq!(fs::read(&file).unwrap(), b"weight");
            assert_eq!(
                fs::metadata(file).unwrap().modified().unwrap(),
                before[index]
            );
        }
        assert!(!models(temp.path(), &m, &AtomicBool::new(false)).unwrap());
    }
    #[test]
    fn small_changed_script_comes_from_app_without_fetching_or_rewriting_weights() {
        let temp = temporary();
        let mut m = tiny();
        let real = manifest();
        let selected = real
            .variants
            .iter()
            .find(|v| v.component == "edit")
            .unwrap();
        let script = selected
            .files
            .iter()
            .find(|f| f.path == "edit_omnivoice_infer.py")
            .unwrap()
            .clone();
        let v = m.variants.iter_mut().find(|v| v.id == selected.id).unwrap();
        v.files.push(script.clone());
        v.total_bytes += script.bytes;
        seed(temp.path(), &m, true);
        fs::write(
            temp.path().join(&selected.aliases[0]).join(&script.path),
            b"previous small script",
        )
        .unwrap();
        models(temp.path(), &m, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            hash(
                &temp.path().join(&selected.id).join(&script.path),
                &AtomicBool::new(false)
            )
            .unwrap(),
            script.sha256
        );
        assert_eq!(
            fs::read(temp.path().join(&selected.id).join("weights.bin")).unwrap(),
            b"weight"
        );
    }
    #[test]
    fn interrupted_rename_resumes_and_corrupt_weights_are_not_silently_accepted() {
        let temp = temporary();
        let m = tiny();
        seed(temp.path(), &m, true);
        assert!(models(temp.path(), &m, &AtomicBool::new(true)).is_err());
        models(temp.path(), &m, &AtomicBool::new(false)).unwrap();
        let path = temp.path().join(&m.variants[0].id).join("weights.bin");
        fs::write(&path, b"broken").unwrap();
        assert!(models(temp.path(), &m, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(path).unwrap(), b"broken");
    }
    #[test]
    fn recognition_upgrade_rewrites_profile_and_integrity_refs_before_the_worker_starts() {
        let temp = temporary();
        let m = tiny();
        let base = temp.path();
        let bundle = base.join("bundle");
        seed(&bundle, &m, true);
        let make = |id: &str| {
            let v = m.find(id).unwrap();
            json!({"variant":v.aliases[0],"folder":v.aliases[0],"model_version":"v0.3","model":v.model,"backend":"onnx","device":"cpu","dtype":"float32","threads":4})
        };
        let batch = &m.bundles["cpu-high"].asr;
        let live = &m.bundles["cpu-high"].asr_live;
        let profile =
            json!({"schema_version":1,"transcription":make(batch),"dictation":make(live)});
        fs::write(
            bundle.join("recognition.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
        let mut files = vec![
            json!({"path":"bundle/recognition.json","sha256":hash(&bundle.join("recognition.json"),&AtomicBool::new(false)).unwrap()}),
        ];
        for id in [batch, live] {
            let v = m.find(id).unwrap();
            let folder = &v.aliases[0];
            let p = bundle.join(folder).join("variant.json");
            fs::write(&p, b"{}").unwrap();
            files.push(json!({"path":format!("bundle/{folder}/variant.json"),"sha256":hash(&p,&AtomicBool::new(false)).unwrap()}));
            files.push(
                json!({"path":format!("bundle/{folder}/weights.bin"),"sha256":v.files[0].sha256}),
            );
        }
        fs::write(
            base.join("installation.json"),
            serde_json::to_vec(&json!({"model_version":"v0.3","files":files})).unwrap(),
        )
        .unwrap();
        recognition(base, &m, &AtomicBool::new(false)).unwrap();
        let config = read(&base.join("installation.json")).unwrap();
        assert_eq!(config["model_version"], "v1.0");
        let files = config["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| (f["path"].as_str().unwrap(), f["sha256"].as_str().unwrap()))
            .collect();
        crate::recognition_bundle::read(&bundle)
            .unwrap()
            .unwrap()
            .validate_files(&bundle, &m, &files)
            .unwrap();
        for f in config["files"].as_array().unwrap() {
            assert_eq!(
                hash(
                    &base.join(f["path"].as_str().unwrap()),
                    &AtomicBool::new(false)
                )
                .unwrap(),
                f["sha256"].as_str().unwrap()
            );
        }
    }
    #[test]
    fn history_model_ids_change_without_changing_user_text_timings_or_original_version() {
        let m = manifest();
        let (old, new) = m.aliases.iter().next().unwrap();
        let mut value = json!({"text":old,"words":[{"word":old,"start":1.0,"end":2.0}],"versions":[{"model":{"tts":old,"model_version":"v0.3"},"text":old}]});
        assert!(project_models(&mut value));
        assert_eq!(value["text"], old.as_str());
        assert_eq!(value["words"][0]["word"], old.as_str());
        assert_eq!(value["versions"][0]["model"]["tts"], new.as_str());
        assert_eq!(value["versions"][0]["model"]["model_version"], "v0.3");
    }
    #[test]
    fn naming_upgrade_preserves_newer_cached_releases_and_their_weights() {
        let temp = temporary();
        let root = temp.path();
        let future = root
            .join("voice-edit/models")
            .join("b".repeat(40))
            .join("tts-flow_lm-90m-amh-fp32");
        fs::create_dir_all(&future).unwrap();
        fs::write(future.join("weights.bin"), b"a different, newer model").unwrap();
        let cached = root.join("voice-edit/manifest.json");
        let value = json!({"manifest_version":2,"future_release":true});
        fs::write(&cached, serde_json::to_vec(&value).unwrap()).unwrap();
        let asr = root.join("asr/active");
        fs::create_dir_all(&asr).unwrap();
        fs::write(
            asr.join("installation.json"),
            br#"{"model_version":"v1.1"}"#,
        )
        .unwrap();
        run(root, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::read(future.join("weights.bin")).unwrap(),
            b"a different, newer model"
        );
        assert_eq!(read(&cached).unwrap(), value);
        assert_eq!(
            read(&asr.join("installation.json")).unwrap()["model_version"],
            "v1.1"
        );
    }
    #[cfg(unix)]
    #[test]
    fn redirected_model_folders_are_rejected_without_touching_external_files() {
        let temp = temporary();
        let outside = temporary();
        let m = tiny();
        fs::write(outside.path().join("weights.bin"), b"weight").unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join(&m.variants[0].aliases[0]))
            .unwrap();
        assert!(models(temp.path(), &m, &AtomicBool::new(false)).is_err());
        assert_eq!(
            fs::read(outside.path().join("weights.bin")).unwrap(),
            b"weight"
        );
    }
    #[test]
    #[ignore = "requires an explicitly supplied disposable copy of real legacy model caches"]
    fn real_local_cache_upgrade() {
        let root = std::path::PathBuf::from(
            std::env::var("ETHIOPIC_NAMING_TEST_ROOT").expect("supply a disposable model cache"),
        );
        assert!(root.starts_with(std::env::temp_dir()) || root.starts_with("/tmp"));
        run(&root, &AtomicBool::new(false)).unwrap();
        let m = Manifest::parse(include_str!("../../resources/studio-manifest.json")).unwrap();
        let directory = root
            .join("voice-edit/models")
            .join(m.hf_revision.as_deref().unwrap());
        assert!(directory.is_dir());
        for old in m.aliases.keys() {
            assert!(!directory.join(old).exists());
        }
        assert!(read(&root.join("naming-migration-v2.json")).unwrap()["complete"] == true);
        if m.hf_revision != manifest().hf_revision {
            assert!(read(&root.join("publication-migration-v1.json")).unwrap()["complete"] == true);
        }
    }
}
