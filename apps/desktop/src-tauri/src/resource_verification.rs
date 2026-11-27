//! Cache successful checksums, bound to exact installation paths and file metadata.
//! The cache is a performance receipt, not a signed release authenticity mechanism.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::SystemTime,
};

const LIMIT: u64 = 32 * 1024 * 1024;
const REPAIR: &str = "Installed voice files need repair. Check voice tools and retry.";
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Verified,
    Unchanged,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Stamp {
    resolved: PathBuf,
    length: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64, u32),
    #[cfg(windows)]
    attributes: u32,
}
#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Receipt {
    schema: u32,
    base: PathBuf,
    manifest: String,
    files: Vec<Stamp>,
}
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("cancelled".into())
    } else {
        Ok(())
    }
}
fn snapshot(base: &Path, entries: &[(&str, &str)], cancel: &AtomicBool) -> Result<Receipt, String> {
    let base = base.canonicalize().map_err(|_| REPAIR)?;
    let mut manifest = Sha256::new();
    let installation = base.join("installation.json");
    if installation.exists() {
        let resolved = installation.canonicalize().map_err(|_| REPAIR)?;
        if !resolved.starts_with(&base) {
            return Err(REPAIR.into());
        }
        let file = fs::File::open(resolved).map_err(|_| REPAIR)?;
        let mut data = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut data)
            .map_err(|_| REPAIR)?;
        if data.len() as u64 > LIMIT {
            return Err(REPAIR.into());
        }
        manifest.update(&data);
    }

    let mut files = Vec::with_capacity(entries.len());
    for &(name, hash) in entries {
        cancelled(cancel)?;
        let path = Path::new(name);
        if path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(REPAIR.into());
        }
        // Resolve each target once; never trust paths supplied by a cached receipt.
        let resolved = base.join(path).canonicalize().map_err(|_| REPAIR)?;
        let relative = resolved
            .strip_prefix(&base)
            .map_err(|_| REPAIR)?
            .to_path_buf();
        // Windows path metadata may lag a just-closed writer. Query an open
        // handle so the stamp describes the file we can actually read.
        #[cfg(windows)]
        let metadata = fs::File::open(&resolved)
            .and_then(|file| file.metadata())
            .map_err(|_| REPAIR)?;
        #[cfg(not(windows))]
        let metadata = fs::metadata(&resolved).map_err(|_| REPAIR)?;
        if !metadata.is_file() {
            return Err(REPAIR.into());
        }
        manifest.update((name.len() as u64).to_le_bytes());
        manifest.update(name.as_bytes());
        manifest.update((hash.len() as u64).to_le_bytes());
        manifest.update(hash.as_bytes());
        files.push(Stamp {
            resolved: relative,
            length: metadata.len(),
            modified: metadata.modified().map_err(|_| REPAIR)?,
            created: metadata.created().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                    metadata.mode(),
                )
            },
            #[cfg(windows)]
            attributes: {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes()
            },
        });
    }
    Ok(Receipt {
        // Recheck receipts written before the Windows timestamp fix once.
        schema: 2,
        base,
        manifest: format!("{:x}", manifest.finalize()),
        files,
    })
}
fn read_receipt(path: &Path) -> Option<Receipt> {
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > LIMIT {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
fn save_receipt(path: &Path, receipt: &Receipt) {
    // Failure to cache never makes unverified resources usable, or blocks a valid job.
    let pending = path.with_extension(format!("{}.pending", std::process::id()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let bytes = serde_json::to_vec(receipt)?;
        if bytes.len() as u64 > LIMIT {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&pending)?;
        use std::io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&pending, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(pending);
    }
}
pub fn verify(
    base: &Path,
    entries: &[(&str, &str)],
    receipt_path: &Path,
    cancel: &AtomicBool,
) -> Result<Outcome, String> {
    verify_with(base, entries, receipt_path, cancel, |path, cancel| {
        let mut file = fs::File::open(path).map_err(|_| REPAIR)?;
        let mut hash = Sha256::new();
        let mut bytes = [0; 65536];
        loop {
            cancelled(cancel)?;
            let count = file.read(&mut bytes).map_err(|_| REPAIR)?;
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
        }
        Ok(format!("{:x}", hash.finalize()))
    })
}
fn verify_with(
    base: &Path,
    entries: &[(&str, &str)],
    receipt_path: &Path,
    cancel: &AtomicBool,
    mut checksum: impl FnMut(&Path, &AtomicBool) -> Result<String, String>,
) -> Result<Outcome, String> {
    cancelled(cancel)?;
    if entries.is_empty() {
        return Err("Voice-file checksums are missing. Repair voice tools.".into());
    }
    let before = snapshot(base, entries, cancel)?;
    if receipt_can_be_reused(&before) && read_receipt(receipt_path).as_ref() == Some(&before) {
        cancelled(cancel)?;
        return Ok(Outcome::Unchanged);
    }
    // Never retain a prior receipt after a changed installation fails verification.
    let _ = fs::remove_file(receipt_path);
    for ((_, expected), stamp) in entries.iter().zip(&before.files) {
        cancelled(cancel)?;
        if checksum(&before.base.join(&stamp.resolved), cancel)? != *expected {
            return Err(REPAIR.into());
        }
    }
    let after = snapshot(base, entries, cancel)?;
    if before != after {
        return Err("Voice files changed while being checked. Please retry.".into());
    }
    cancelled(cancel)?;
    save_receipt(receipt_path, &after);
    Ok(Outcome::Verified)
}

fn receipt_can_be_reused(receipt: &Receipt) -> bool {
    // Windows filesystems can coalesce fast same-size writes into one timestamp
    // tick (FAT timestamps are as coarse as two seconds). Hash recent/future-dated
    // files again; stable installed models still take the cheap receipt path.
    #[cfg(windows)]
    {
        let now = SystemTime::now();
        receipt.files.iter().all(|file| {
            now.duration_since(file.modified)
                .is_ok_and(|age| age >= std::time::Duration::from_secs(2))
        })
    }
    #[cfg(not(windows))]
    {
        let _ = receipt;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        expected: String,
        cancel: AtomicBool,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "verify-cache-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(root.join("bundle")).unwrap();
            fs::write(root.join("bundle/model"), b"model content").unwrap();
            // This fixture represents an established installation, not a file
            // still inside a filesystem's timestamp-coalescing window.
            fs::OpenOptions::new()
                .write(true)
                .open(root.join("bundle/model"))
                .unwrap()
                .set_modified(SystemTime::now() - std::time::Duration::from_secs(5))
                .unwrap();
            Self {
                root,
                expected: format!("{:x}", Sha256::digest(b"model content")),
                cancel: AtomicBool::new(false),
            }
        }
        fn receipt(&self) -> PathBuf {
            self.root.join("receipt.json")
        }
        fn verify(&self) -> Result<Outcome, String> {
            verify(
                &self.root,
                &[("bundle/model", &self.expected)],
                &self.receipt(),
                &self.cancel,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn unchanged_receipt_skips_content_reads_even_after_reopening() {
        let f = Fixture::new();
        assert_eq!(f.verify().unwrap(), Outcome::Verified);
        assert_eq!(
            verify_with(
                &f.root,
                &[("bundle/model", &f.expected)],
                &f.receipt(),
                &f.cancel,
                |_, _| panic!("unchanged file must not be read")
            )
            .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(f.verify().unwrap(), Outcome::Unchanged);
    }
    #[test]
    fn changed_missing_and_corrupt_files_cannot_use_old_receipt() {
        let f = Fixture::new();
        f.verify().unwrap();
        fs::write(f.root.join("bundle/model"), b"other content").unwrap(); // same length
        assert!(f.verify().is_err());
        assert!(!f.receipt().exists());
        fs::remove_file(f.root.join("bundle/model")).unwrap();
        assert!(f.verify().is_err());
    }
    #[cfg(windows)]
    #[test]
    fn future_dated_files_are_rehashed_but_stable_models_reuse_receipts() {
        let f = Fixture::new();
        let path = f.root.join("bundle/model");
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + std::time::Duration::from_secs(3600))
            .unwrap();
        f.verify().unwrap();
        let mut reads = 0;
        assert_eq!(
            verify_with(
                &f.root,
                &[("bundle/model", &f.expected)],
                &f.receipt(),
                &f.cancel,
                |_, _| {
                    reads += 1;
                    Ok(f.expected.clone())
                }
            )
            .unwrap(),
            Outcome::Verified
        );
        assert_eq!(reads, 1);
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - std::time::Duration::from_secs(5))
            .unwrap();
        f.verify().unwrap();
        assert_eq!(
            verify_with(
                &f.root,
                &[("bundle/model", &f.expected)],
                &f.receipt(),
                &f.cancel,
                |_, _| panic!("stable model must use its receipt")
            )
            .unwrap(),
            Outcome::Unchanged
        );
    }
    #[test]
    fn manifest_changes_or_invalid_receipt_force_checksums() {
        let mut f = Fixture::new();
        f.verify().unwrap();
        fs::write(f.receipt(), b"invalid").unwrap();
        assert_eq!(f.verify().unwrap(), Outcome::Verified);
        fs::write(f.root.join("installation.json"), b"manifest change").unwrap();
        assert_eq!(f.verify().unwrap(), Outcome::Verified);
        f.expected = "wrong".into();
        assert!(f.verify().is_err());
        assert!(!f.receipt().exists());
    }
    #[test]
    fn receipts_from_before_the_timestamp_fix_are_reverified_once() {
        let f = Fixture::new();
        f.verify().unwrap();
        let mut old: Receipt = serde_json::from_slice(&fs::read(f.receipt()).unwrap()).unwrap();
        old.schema = 1;
        fs::write(f.receipt(), serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(f.verify().unwrap(), Outcome::Verified);
        assert_eq!(f.verify().unwrap(), Outcome::Unchanged);
    }
    #[test]
    fn cancellation_and_mid_verification_changes_never_publish_a_receipt() {
        let f = Fixture::new();
        f.cancel.store(true, Ordering::SeqCst);
        assert_eq!(f.verify().unwrap_err(), "cancelled");
        assert!(!f.receipt().exists());
        f.cancel.store(false, Ordering::SeqCst);
        let result = verify_with(
            &f.root,
            &[("bundle/model", &f.expected)],
            &f.receipt(),
            &f.cancel,
            |path, _| {
                fs::write(path, b"changed").unwrap();
                Ok(f.expected.clone())
            },
        );
        assert!(result.is_err());
        assert!(!f.receipt().exists());
    }
    #[test]
    fn paths_cannot_escape_using_manifest_or_symlink() {
        let f = Fixture::new();
        assert!(verify(
            &f.root,
            &[("../outside", &f.expected)],
            &f.receipt(),
            &f.cancel
        )
        .is_err());
        #[cfg(unix)]
        {
            let other = Fixture::new();
            std::os::unix::fs::symlink(other.root.join("bundle/model"), f.root.join("escape"))
                .unwrap();
            assert!(verify(&f.root, &[("escape", &f.expected)], &f.receipt(), &f.cancel).is_err());
        }
    }
    #[test]
    #[ignore = "requires installed ASR resources; reports full and cached verification time"]
    fn installed_resources_skip_checksum_reads_on_second_check() {
        let root = PathBuf::from(std::env::var("ETHIOPIC_ASR_TEST_ROOT").unwrap());
        let base = root.join("asr/active");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(base.join("installation.json")).unwrap()).unwrap();
        let entries: Vec<_> = manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| {
                (
                    file["path"].as_str().unwrap(),
                    file["sha256"].as_str().unwrap(),
                )
            })
            .collect();
        let receipt = std::env::temp_dir().join(format!(
            "asr-verification-benchmark-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&receipt);
        let cancel = AtomicBool::new(false);
        let start = std::time::Instant::now();
        assert_eq!(
            verify(&base, &entries, &receipt, &cancel).unwrap(),
            Outcome::Verified
        );
        let full = start.elapsed();
        let start = std::time::Instant::now();
        assert_eq!(
            verify_with(&base, &entries, &receipt, &cancel, |_, _| panic!(
                "cached check must not hash model/runtime contents"
            ))
            .unwrap(),
            Outcome::Unchanged
        );
        println!(
            "Verified {} resources: full {:?}, metadata-only {:?}",
            entries.len(),
            full,
            start.elapsed()
        );
        fs::remove_file(receipt).unwrap();
    }
}
