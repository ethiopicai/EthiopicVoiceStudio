//! One-release bridge: canonical IDs, unchanged published weights, and bundled small owner updates.
use crate::studio::Manifest;
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};
#[derive(Deserialize)]
struct Source {
    sha256: String,
    url: String,
}
#[derive(Deserialize)]
struct Transport {
    revision: String,
    #[serde(default)]
    public_revision: Option<String>,
    repository: String,
    aliases: HashMap<String, String>,
    file_sources: HashMap<String, Source>,
    bundled_files: HashMap<String, String>,
    #[serde(default)]
    current_bundled_files: HashMap<String, String>,
}
fn transport() -> &'static Transport {
    static VALUE: OnceLock<Transport> = OnceLock::new();
    VALUE.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../apps/desktop/resources/naming-migration/transport.json"
        ))
        .expect("bundled naming bridge")
    })
}
pub fn canonical(id: &str) -> &str {
    transport()
        .aliases
        .get(id)
        .map(String::as_str)
        .unwrap_or(id)
}
pub fn canonical_path(path: &str) -> String {
    if !crate::safe_path(path) {
        return path.to_owned();
    }
    path.split('/').map(canonical).collect::<Vec<_>>().join("/")
}
/// Only call on model/config metadata, never on user scripts or transcripts.
pub fn normalize_metadata(value: &mut serde_json::Value) -> bool {
    let mut changed = false;
    match value {
        serde_json::Value::String(value) => {
            let next = canonical_path(value);
            if next != *value {
                *value = next;
                changed = true;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                changed |= normalize_metadata(value);
            }
        }
        serde_json::Value::Object(values) => {
            let previous = std::mem::take(values);
            for (key, mut value) in previous {
                let next = canonical(&key).to_owned();
                changed |= next != key;
                changed |= normalize_metadata(&mut value);
                values.insert(next, value);
            }
        }
        _ => {}
    }
    changed
}
fn compatible(m: &Manifest) -> bool {
    m.manifest_version == 2
        && (m.hf_revision.as_deref() == Some(&transport().revision)
            || transport()
                .public_revision
                .as_ref()
                .is_some_and(|revision| m.hf_revision.as_ref() == Some(revision)))
        && m.hf_repo.as_deref() == Some(&transport().repository)
}
pub fn legacy_file_url(m: &Manifest, id: &str, path: &str) -> Option<String> {
    if !compatible(m) || m.hf_revision.as_deref() != Some(&transport().revision) {
        return None;
    }
    let v = m.find(id)?;
    let f = v.files.iter().find(|f| f.path == path)?;
    let source = transport().file_sources.get(&format!("{}/{path}", v.id))?;
    (source.sha256 == f.sha256).then(|| source.url.clone())
}
pub fn bundled_file(m: &Manifest, id: &str, path: &str) -> Option<&'static [u8]> {
    if !compatible(m) {
        return None;
    }
    let v = m.find(id)?;
    let f = v.files.iter().find(|f| f.path == path)?;
    let files = if m.hf_revision.as_deref() == Some(&transport().revision) {
        &transport().bundled_files
    } else {
        &transport().current_bundled_files
    };
    let sha = files.get(&format!("{}/{path}", v.id))?;
    if sha != &f.sha256 {
        return None;
    }
    match sha.as_str() {
        "00a39b2ed1cd725d0f709eda7aefd17db44ab71a9ce8559121c790e5cf4aa9c4" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/00a39b2ed1cd725d0f709eda7aefd17db44ab71a9ce8559121c790e5cf4aa9c4.asset")),
        "1bc50dd769df68f43b2594d011a4fdf8b60c2f2043d6dee5cceeffbc540e0f9c" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/1bc50dd769df68f43b2594d011a4fdf8b60c2f2043d6dee5cceeffbc540e0f9c.asset")),
        "2aba0ac8819b0468e7718d1a73527e5956fcf21574b90485ee66f60aeb524f8a" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/2aba0ac8819b0468e7718d1a73527e5956fcf21574b90485ee66f60aeb524f8a.asset")),
        "437831a22d3ec0be76d39e352eb0a5c7096c55b901eeabf7851cec527cd25c7f" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/437831a22d3ec0be76d39e352eb0a5c7096c55b901eeabf7851cec527cd25c7f.asset")),
        "6ca44fc5de39a451e92b2392dec4d8a7b778be8daca57cca3454bd2452f0815e" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/6ca44fc5de39a451e92b2392dec4d8a7b778be8daca57cca3454bd2452f0815e.asset")),
        "6d6d5a1edcb2283446912d4fbe87d696d7c568991f201bf43b73b1bd7f906990" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/6d6d5a1edcb2283446912d4fbe87d696d7c568991f201bf43b73b1bd7f906990.asset")),
        "9896f19a72204df8adbfd6763e607a8bb0a0a21487f2310aba9eda992fcf6d85" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/9896f19a72204df8adbfd6763e607a8bb0a0a21487f2310aba9eda992fcf6d85.asset")),
        "9974fe1d21cd1ea25edc5fcca329e4929b9fe7e072178a751138ad006f1a95a8" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/9974fe1d21cd1ea25edc5fcca329e4929b9fe7e072178a751138ad006f1a95a8.asset")),
        "a687a8383e6836bdcd3c340e06bb52e42ecf708254f1ef57ca3923dc5857130a" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/a687a8383e6836bdcd3c340e06bb52e42ecf708254f1ef57ca3923dc5857130a.asset")),
        "a83dd21ebc0cf89648ca197345f600fbcb6e1f979c5b04638cd53a869885b80e" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/a83dd21ebc0cf89648ca197345f600fbcb6e1f979c5b04638cd53a869885b80e.asset")),
        "d54688aa0bc307cca426c94788867c51eba2d553490273c5ac779533b2e4e258" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/d54688aa0bc307cca426c94788867c51eba2d553490273c5ac779533b2e4e258.asset")),
        "d9da6da09790750c31f63f3e63fbb05095d06e2396e136e3edbb562fd7a5c0de" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/d9da6da09790750c31f63f3e63fbb05095d06e2396e136e3edbb562fd7a5c0de.asset")),
        "f76169c1f36da16e413085e5f8ce3d26827744b3274310b132dbb1ed13990164" => Some(include_bytes!("../../../apps/desktop/resources/naming-migration/f76169c1f36da16e413085e5f8ce3d26827744b3274310b132dbb1ed13990164.asset")),
        _ => None,
    }
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
    use sha2::{Digest, Sha256};
    fn manifest() -> Manifest {
        Manifest::parse(include_str!(
            "../../../apps/desktop/resources/studio-manifest.json"
        ))
        .unwrap()
    }
    #[test]
    fn all_aliases_resolve_and_metadata_roundtrips_to_only_canonical_ids() {
        let m = manifest();
        assert_eq!(m.manifest_version, 2);
        assert_eq!(m.aliases.len(), 12);
        for (old, new) in &m.aliases {
            assert_eq!(m.resolve(old), Some(new.as_str()));
            let mut value = serde_json::json!({"variant":old,"folder":format!("bundle/{old}/file"),"overrides":{old:old}});
            assert!(normalize_metadata(&mut value));
            assert!(!normalize_metadata(&mut value));
            assert_eq!(value["variant"], new.as_str());
            assert_eq!(value["folder"], format!("bundle/{new}/file"));
            assert_eq!(value["overrides"][new], new.as_str());
        }
    }
    #[test]
    fn every_file_has_verified_existing_transport_or_a_small_bundled_update() {
        let m = manifest();
        let mut bundled = 0;
        let mut weights = 0;
        for v in &m.variants {
            for f in &v.files {
                if let Some(bytes) = bundled_file(&m, &v.id, &f.path) {
                    assert!(f.bytes < 100000);
                    assert_eq!(bytes.len() as u64, f.bytes);
                    assert_eq!(format!("{:x}", Sha256::digest(bytes)), f.sha256);
                    bundled += 1;
                } else {
                    let url = m.file_url(&v.id, &f.path).expect("published model file");
                    assert!(url.contains(m.hf_revision.as_deref().unwrap()));
                    weights += 1;
                }
            }
        }
        let expected = if m.hf_revision.as_deref() == Some(&transport().revision) {
            transport().bundled_files.len()
        } else {
            transport().current_bundled_files.len()
        };
        assert_eq!(bundled, expected);
        assert_eq!(
            bundled + weights,
            m.variants.iter().map(|v| v.files.len()).sum::<usize>()
        );
        let mut untrusted = m.clone();
        untrusted.hf_revision = Some("b".repeat(40));
        assert!(
            legacy_file_url(&untrusted, &m.variants[0].id, &m.variants[0].files[0].path).is_none()
        );
    }
    #[test]
    fn changed_support_files_install_offline_and_refuse_checksum_drift() {
        let m = manifest();
        let cache = temporary();
        for v in &m.variants {
            for f in &v.files {
                if bundled_file(&m, &v.id, &f.path).is_none() {
                    continue;
                }
                let result = m
                    .download_file(
                        &v.id,
                        &f.path,
                        cache.path(),
                        &std::sync::atomic::AtomicBool::new(false),
                        |_| {},
                    )
                    .unwrap();
                assert_eq!(std::fs::read(result).unwrap().len() as u64, f.bytes);
            }
        }
        let mut altered = m.clone();
        let v = altered
            .variants
            .iter_mut()
            .find(|v| v.component == "edit")
            .unwrap();
        let f = v
            .files
            .iter_mut()
            .find(|f| f.path.ends_with(".py"))
            .unwrap();
        f.sha256 = "c".repeat(64);
        let id = v.id.clone();
        let path = f.path.clone();
        assert!(bundled_file(&altered, &id, &path).is_none());
    }
    #[test]
    fn ambiguous_aliases_and_disagreeing_variant_aliases_fail_closed() {
        let m = manifest();
        let original = serde_json::to_value(&m).unwrap();
        for case in 0..3 {
            let mut v = original.clone();
            let key = m.aliases.keys().next().unwrap();
            match case {
                0 => v["aliases"][key] = serde_json::json!(key),
                1 => v["aliases"][&m.variants[0].id] = serde_json::json!(&m.variants[1].id),
                _ => v["variants"][0]["aliases"] = serde_json::json!(["not-an-alias"]),
            }
            assert!(Manifest::parse(&v.to_string()).is_err());
        }
    }
}
