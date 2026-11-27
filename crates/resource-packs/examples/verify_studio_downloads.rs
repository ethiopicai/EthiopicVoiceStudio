//! Opt-in real HF transport check using the app's downloader; never installs or executes models.
//! Usage: verify_studio_downloads MANIFEST CACHE REPORT pause|resume|offline|prefetch:VARIANT
use resource_packs::studio::{Manifest, DEFAULT_MANIFEST_URL, DEFAULT_REPOSITORY};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn write_report(path: &Path, report: &Value) -> Result<(), String> {
    let pending = path.with_extension("pending");
    fs::write(&pending, serde_json::to_vec_pretty(report).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(pending, path).map_err(|e| e.to_string())
}

fn run(manifest_path: &Path, cache: &Path, report_path: &Path, phase: &str) -> Result<(), String> {
    let raw = fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let manifest = Manifest::parse(&raw)?;
    if manifest.hf_repo.as_deref() != Some(DEFAULT_REPOSITORY) {
        return Err("Only the approved public model repository can be tested".into());
    }
    let paused = AtomicBool::new(false);
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    // Independent caches let maintainers verify large variants concurrently without
    // sharing a partially written file. Completed verified objects can be promoted later.
    if let Some(id) = phase.strip_prefix("prefetch:") {
        let variant = manifest.find(id).ok_or("Unknown prefetch variant")?;
        let file = variant
            .files
            .iter()
            .max_by_key(|f| f.bytes)
            .ok_or("Missing checkpoint")?;
        let started = Instant::now();
        let mut last = Instant::now();
        let path = manifest.download_file(id, &file.path, cache, &paused, |bytes| {
            if last.elapsed() > Duration::from_secs(3) {
                eprintln!(
                    "{id}: {:.0}%",
                    100.0 * bytes as f64 / file.bytes.max(1) as f64
                );
                last = Instant::now();
            }
        })?;
        write_report(
            report_path,
            &json!({"revision":manifest.hf_revision,"variant":id,"path":file.path,
            "verified_sha256":file.sha256,"bytes":file.bytes,"cache_file":path,"seconds":started.elapsed().as_secs_f64()}),
        )?;
        println!("Verified prefetched checkpoint: {id}");
        return Ok(());
    }
    if phase == "pause" {
        if report_path.exists() {
            return Err("Use a fresh report/cache for a fresh-download test".into());
        }
        let variant = manifest
            .find("asr-wav2vec_ctc-1b-amh-int8_onnx")
            .ok_or("Missing test variant")?;
        let file = variant
            .files
            .iter()
            .max_by_key(|f| f.bytes)
            .ok_or("Missing checkpoint")?;
        let directory = cache.join(&file.sha256);
        if directory.exists() {
            return Err("The initial checkpoint cache must be empty".into());
        }
        let result = manifest.download_file(&variant.id, &file.path, cache, &paused, |bytes| {
            if bytes >= 8 * 1024 * 1024 {
                paused.store(true, Ordering::SeqCst);
            }
        });
        if result.as_ref().err().map(String::as_str) != Some("paused") {
            return Err(format!("Expected a deliberate pause; got {result:?}"));
        }
        let saved = fs::metadata(directory.join("download.part"))
            .map_err(|e| e.to_string())?
            .len();
        if saved == 0 || saved >= file.bytes {
            return Err("The pause did not preserve a partial checkpoint".into());
        }
        let report = json!({"schema_version":1,"phase":"paused","repo":manifest.hf_repo,
            "revision":manifest.hf_revision,"resume_variant":variant.id,"resume_file":file.path,
            "resume_sha256":file.sha256,"saved_bytes":saved,
            "manifest_sha256":resource_packs::sha256(manifest_path,&AtomicBool::new(false))?});
        write_report(report_path, &report)?;
        println!("Paused with {saved} checkpoint bytes saved. Resume in a new process.");
        return Ok(());
    }
    let mut report: Value =
        serde_json::from_slice(&fs::read(report_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if report["revision"].as_str() != manifest.hf_revision.as_deref()
        || report["manifest_sha256"] != resource_packs::sha256(manifest_path, &paused)?
    {
        return Err("The release manifest changed between test phases".into());
    }
    if phase == "offline" {
        if report["phase"] != "complete" {
            return Err("Finish the online test before checking offline cache reuse".into());
        }
        if resource_packs::client()?
            .get(DEFAULT_MANIFEST_URL)
            .send()
            .is_ok()
        {
            return Err(
                "Offline validation requires blocked network access; use a closed local proxy"
                    .into(),
            );
        }
        // Run this phase with network proxies pointing to a closed local port.
        let mut verified = 0;
        for variant in &manifest.variants {
            for file in &variant.files {
                let path =
                    manifest.download_file(&variant.id, &file.path, cache, &paused, |_| {})?;
                if fs::metadata(path).map_err(|e| e.to_string())?.len() != file.bytes {
                    return Err("Cached file size mismatch".into());
                }
                verified += 1;
            }
        }
        report["offline_cache_files_verified"] = json!(verified);
        report["offline_network_probe_blocked"] = json!(true);
        write_report(report_path, &report)?;
        println!("Offline cache: {verified} files verified without network access.");
        return Ok(());
    }
    if phase != "resume" {
        return Err("Expected pause, resume or offline phase".into());
    }
    let id = report["resume_variant"]
        .as_str()
        .ok_or("Missing resume variant")?
        .to_string();
    let file_path = report["resume_file"]
        .as_str()
        .ok_or("Missing resume file")?
        .to_string();
    if report["phase"] == "paused" {
        let mut first_offset = None;
        let started = Instant::now();
        let path = manifest.download_file(&id, &file_path, cache, &paused, |bytes| {
            first_offset.get_or_insert(bytes);
        })?;
        if first_offset != report["saved_bytes"].as_u64() {
            return Err("HF did not resume from the saved byte offset".into());
        }
        report["resume_verified"] = json!(true);
        report["resume_first_offset"] = json!(first_offset);
        report["resume_seconds"] = json!(started.elapsed().as_secs_f64());
        report["resume_completed_bytes"] =
            json!(fs::metadata(path).map_err(|e| e.to_string())?.len());
        report["phase"] = json!("downloading");
        write_report(report_path, &report)?;
        println!(
            "HF Range resume verified at {} bytes.",
            first_offset.unwrap()
        );
    }
    if report["resume_verified"] != true {
        return Err("Missing successful cross-process resume check".into());
    }
    let started = Instant::now();
    let mut files = vec![];
    let mut bytes = 0_u64;
    let mut repair = None;
    for variant in &manifest.variants {
        for file in &variant.files {
            let file_started = Instant::now();
            let mut last = Instant::now();
            let path = manifest.download_file(&variant.id, &file.path, cache, &paused, |done| {
                if last.elapsed() > Duration::from_secs(3) {
                    eprintln!(
                        "{} / {}: {:.0}%",
                        variant.id,
                        file.path,
                        100.0 * done as f64 / file.bytes.max(1) as f64
                    );
                    last = Instant::now();
                }
            })?;
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() != file.bytes {
                return Err("Downloaded file size mismatch".into());
            }
            bytes += file.bytes;
            files.push(
                json!({"variant":variant.id,"path":file.path,"bytes":file.bytes,
                "verified_sha256":file.sha256,"seconds":file_started.elapsed().as_secs_f64()}),
            );
            if repair.is_none() && file.bytes > 0 && file.bytes < 65536 {
                repair = Some((variant.id.clone(), file.clone(), path));
            }
            report["files"] = json!(files);
            report["verified_bytes"] = json!(bytes);
            write_report(report_path, &report)?;
            println!(
                "Verified {}/{} · {} / {}",
                files.len(),
                manifest
                    .variants
                    .iter()
                    .map(|v| v.files.len())
                    .sum::<usize>(),
                variant.id,
                file.path
            );
        }
    }
    if let Some((variant, file, path)) = repair {
        let repair_cache = cache.join("corruption-check");
        let directory = repair_cache.join(&file.sha256);
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let mut data = fs::read(path).map_err(|e| e.to_string())?;
        data[0] ^= 1;
        fs::write(directory.join("download.tar.gz"), data).map_err(|e| e.to_string())?;
        let repaired =
            manifest.download_file(&variant, &file.path, &repair_cache, &paused, |_| {})?;
        if resource_packs::sha256(&repaired, &paused)? != file.sha256 {
            return Err("Corrupt-cache repair failed".into());
        }
        report["corrupt_cache_repair_verified"] = json!(true);
    }
    report["phase"] = json!("complete");
    report["variants"] = json!(manifest.variants.len());
    report["download_loop_seconds"] = json!(started.elapsed().as_secs_f64());
    report["verification"] =
        json!("Manifest::download_file: exact size and full SHA-256 before accepting each file");
    write_report(report_path, &report)?;
    println!(
        "Verified {} files, {bytes} bytes. Models were downloaded only; none were activated.",
        files.len()
    );
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = if args.len() == 4 {
        run(
            Path::new(&args[0]),
            Path::new(&args[1]),
            Path::new(&args[2]),
            &args[3],
        )
    } else {
        Err("Usage: verify_studio_downloads MANIFEST CACHE REPORT pause|resume|offline|prefetch:VARIANT".into())
    };
    if let Err(error) = result {
        eprintln!("Download validation stopped: {error}");
        std::process::exit(1);
    }
}
