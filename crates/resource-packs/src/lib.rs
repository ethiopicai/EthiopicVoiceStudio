//! Pinned public HF resources. No Python, package manager, credentials or inference network access.
pub mod naming;
pub mod routing;
pub mod studio;
use flate2::read::GzDecoder;
use reqwest::{blocking::Client, header, redirect, StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub type Result<T> = std::result::Result<T, String>;
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub releases: Vec<Release>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub os: String,
    pub arch: String,
    pub model_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub minimum_macos: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_windows: Option<String>,
    pub packs: Vec<Artifact>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub kind: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub unpacked_size: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub schema_version: u32,
    pub kind: String,
    pub model_version: String,
    pub python: Option<String>,
    pub site_packages: Option<String>,
    pub files: Vec<PackFile>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub executable: bool,
}
fn hash_valid(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn safe_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['\\', ':', '\0'])
        && !value.starts_with('/')
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
        && Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn hf_host(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some_and(|host| {
            host == "huggingface.co"
                || host.ends_with(".huggingface.co")
                || host.ends_with(".hf.co")
        })
}
/// Explicitly pinned optional inference dependencies. No dependency resolution or
/// arbitrary package hosts; callers ship reviewed URLs and hashes in the app.
pub fn download_inference_file(
    url: &str,
    sha256: &str,
    size: u64,
    cache: &Path,
    paused: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf> {
    let parsed = Url::parse(url).map_err(|_| "Invalid inference resource URL")?;
    let parts: Vec<_> = parsed.path().trim_start_matches('/').split('/').collect();
    let model = parsed.host_str() == Some("huggingface.co")
        && parts.len() == 5
        && parts[0] == "inworld"
        && parts[1] == "voice-isolator"
        && parts[2] == "resolve"
        && parts[3].len() == 40
        && parts[3].bytes().all(|b| b.is_ascii_hexdigit())
        && parts[4] == "denoiser_realtime.safetensors";
    let wheel = parsed.host_str() == Some("files.pythonhosted.org")
        && parsed.path().starts_with("/packages/")
        && parsed.path().ends_with(".whl");
    if (!model && !wheel)
        || parsed.scheme() != "https"
        || parsed.port_or_known_default() != Some(443)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !hash_valid(sha256)
        || size == 0
        || size > 512 * 1024 * 1024
        || parsed.path().contains("..")
        || parsed.path().contains('%')
    {
        return Err("Invalid pinned inference resource".into());
    }
    let artifact = Artifact {
        kind: "model".into(),
        url: url.into(),
        sha256: sha256.into(),
        size,
        unpacked_size: size,
    };
    let directory = cache.join(sha256);
    fs::create_dir_all(&directory).map_err(|_| "Could not prepare inference download")?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 8
                && (hf_host(attempt.url())
                    || attempt.url().scheme() == "https"
                        && attempt.url().host_str() == Some("files.pythonhosted.org")
                        && attempt.url().port_or_known_default() == Some(443)
                        && attempt.url().username().is_empty()
                        && attempt.url().password().is_none())
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|_| "Could not prepare inference download")?;
    download_with(&client, &artifact, &directory, paused, &mut progress)
}
impl Artifact {
    pub fn validate(&self) -> Result<()> {
        let url = Url::parse(&self.url).map_err(|_| "Invalid resource URL")?;
        let parts: Vec<_> = url.path().trim_start_matches('/').split('/').collect();
        if !matches!(self.kind.as_str(), "model" | "runtime")
            || !hash_valid(&self.sha256)
            || self.size == 0
            || self.size > 8 * 1024 * 1024 * 1024
            || self.unpacked_size == 0
            || self.unpacked_size > 12 * 1024 * 1024 * 1024
            || !hf_host(&url)
            || url.host_str() != Some("huggingface.co")
            || url.query().is_some()
            || url.fragment().is_some()
            || parts.len() != 5
            || parts[0] != "ethiopic"
            || parts[1].is_empty()
            || !parts[1]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || parts[2] != "resolve"
            || parts[3].len() != 40
            || !parts[3].bytes().all(|b| b.is_ascii_hexdigit())
            || !parts[4].ends_with(".tar.gz")
            || !parts[4]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(
                "Resource catalogue must pin public Ethiopic HF artifacts and checksums".into(),
            );
        }
        Ok(())
    }
}
impl Catalog {
    pub fn parse(data: &str) -> Result<Self> {
        let value: Self =
            serde_json::from_str(data).map_err(|_| "Resource catalogue is invalid")?;
        if value.schema_version != 1 || value.releases.len() > 8 {
            return Err("Resource catalogue version is unsupported".into());
        }
        let mut platforms = HashSet::new();
        for release in &value.releases {
            let platform_valid = match (release.os.as_str(), release.arch.as_str()) {
                ("macos", "aarch64") => {
                    release.minimum_macos == "14.0" && release.minimum_windows.is_none()
                }
                ("windows", "x86_64") => {
                    release.minimum_macos.is_empty()
                        && release.minimum_windows.as_deref() == Some("10.0.19041")
                        && matches!(
                            release.profile.as_deref(),
                            Some(
                                "cpu-low"
                                    | "cpu-mid"
                                    | "cpu-high"
                                    | "gpu-low"
                                    | "gpu-pascal"
                                    | "gpu-modern"
                            )
                        )
                }
                _ => false,
            };
            if !platform_valid
                || release.model_version.is_empty()
                || release.model_version.len() > 64
                || release.profile.as_ref().is_some_and(|p| {
                    p.is_empty()
                        || p.len() > 80
                        || !p
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                })
                || !platforms.insert((
                    &release.os,
                    &release.arch,
                    &release.model_version,
                    &release.profile,
                ))
                || release.packs.len() != 2
                || release.packs.iter().filter(|p| p.kind == "model").count() != 1
                || release.packs.iter().filter(|p| p.kind == "runtime").count() != 1
            {
                return Err("Resource catalogue has an incompatible or duplicate release".into());
            }
            for pack in &release.packs {
                pack.validate()?;
            }
        }
        Ok(value)
    }
    pub fn select(&self, os: &str, arch: &str) -> Option<&Release> {
        let mut candidates = self
            .releases
            .iter()
            .filter(|r| r.os == os && r.arch == arch);
        let first = candidates.next()?;
        if candidates.next().is_some() {
            None
        } else {
            Some(first)
        }
    }
}
pub fn client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        // A bounded request also bounds pause/network-loss recovery. Retry uses byte ranges.
        .timeout(Duration::from_secs(30))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 8 && hf_host(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|_| "Could not prepare resource download".into())
}
fn check_pause(paused: &AtomicBool) -> Result<()> {
    if paused.load(Ordering::SeqCst) {
        Err("paused".into())
    } else {
        Ok(())
    }
}
pub fn sha256(path: &Path, paused: &AtomicBool) -> Result<String> {
    let mut input = File::open(path).map_err(|_| "Resource file is unavailable")?;
    let mut hash = Sha256::new();
    let mut buf = [0; 128 * 1024];
    loop {
        check_pause(paused)?;
        let n = input
            .read(&mut buf)
            .map_err(|_| "Could not read resource file")?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
/// Hash-addressed cache makes partials independent of URL signatures or model aliases.
pub fn download(
    artifact: &Artifact,
    cache: &Path,
    paused: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf> {
    artifact.validate()?;
    fs::create_dir_all(cache).map_err(|_| "Could not create download storage")?;
    let directory = cache.join(&artifact.sha256);
    fs::create_dir_all(&directory).map_err(|_| "Could not create download storage")?;
    let client = client()?;
    download_with(&client, artifact, &directory, paused, &mut progress)
}
fn download_with(
    client: &Client,
    artifact: &Artifact,
    directory: &Path,
    paused: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<PathBuf> {
    let partial = directory.join("download.part");
    let complete = directory.join("download.tar.gz");
    for candidate in [&complete, &partial] {
        if candidate.is_file()
            && fs::metadata(candidate)
                .map_err(|_| "Download unavailable")?
                .len()
                == artifact.size
            && sha256(candidate, paused)? == artifact.sha256
        {
            if candidate == &partial {
                fs::rename(&partial, &complete).map_err(|_| "Could not save download")?;
            }
            progress(artifact.size);
            return Ok(complete.clone());
        }
    }
    if complete.exists() {
        fs::remove_file(&complete).map_err(|_| "Could not repair download")?;
    }
    let mut retries = 0;
    loop {
        check_pause(paused)?;
        let before = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        if before >= artifact.size {
            fs::remove_file(&partial).map_err(|_| "Could not reset download")?;
        }
        match transfer(client, artifact, &partial, paused, progress) {
            Ok(()) => break,
            Err(error) if error == "paused" => return Err(error),
            Err(_) => {
                let after = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
                // Long successful ranges may exceed the bounded HTTP request time.
                if after > before {
                    retries = 0;
                } else {
                    retries += 1;
                }
                if retries >= 3 {
                    return Err("Download interrupted. Check your connection and choose Continue; your progress is saved.".into());
                }
                for _ in 0..10 {
                    check_pause(paused)?;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    if sha256(&partial, paused)? != artifact.sha256 {
        let _ = fs::remove_file(&partial);
        return Err("The downloaded resource did not pass its safety check. Retry to download a fresh copy.".into());
    }
    fs::rename(&partial, &complete).map_err(|_| "Could not finish download")?;
    Ok(complete)
}
fn transfer(
    client: &Client,
    artifact: &Artifact,
    partial: &Path,
    paused: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<()> {
    let mut offset = fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
    let mut request = client
        .get(&artifact.url)
        .header(header::ACCEPT_ENCODING, "identity");
    if offset > 0 {
        request = request.header(header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .map_err(|_| "Download connection interrupted")?;
    if response.status() == StatusCode::OK {
        offset = 0;
    } else if response.status() == StatusCode::PARTIAL_CONTENT {
        let expected = format!("bytes {}-{}/{}", offset, artifact.size - 1, artifact.size);
        if response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|h| h.to_str().ok())
            != Some(expected.as_str())
        {
            return Err("Resource server returned an unexpected byte range".into());
        }
    } else {
        return Err("Resource download is unavailable".into());
    }
    if response
        .content_length()
        .is_some_and(|n| n != artifact.size - offset)
    {
        return Err("Resource download size changed".into());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(offset == 0)
        .append(offset > 0)
        .open(partial)
        .map_err(|_| "Could not save download. Check free storage.")?;
    progress(offset);
    let mut buffer = [0; 128 * 1024];
    let result = (|| {
        loop {
            check_pause(paused)?;
            let n = response
                .read(&mut buffer)
                .map_err(|_| "Download connection interrupted")?;
            if n == 0 {
                break;
            }
            if n as u64 > artifact.size - offset {
                return Err("Resource exceeds its expected size".into());
            }
            file.write_all(&buffer[..n])
                .map_err(|_| "Could not save download. Check free storage.")?;
            offset += n as u64;
            progress(offset);
        }
        if offset != artifact.size {
            return Err("Download interrupted".into());
        }
        Ok(())
    })();
    file.sync_all().map_err(|_| "Could not save download")?;
    result
}
/// Extract only declared regular files. Never restore links, ownership or archive permissions.
pub fn extract(
    artifact: &Artifact,
    archive_path: &Path,
    stage: &Path,
    model_version: &str,
    paused: &AtomicBool,
) -> Result<Pack> {
    check_pause(paused)?;
    let input = File::open(archive_path).map_err(|_| "Downloaded pack is unavailable")?;
    let mut archive = tar::Archive::new(GzDecoder::new(input));
    let mut entries = archive
        .entries()
        .map_err(|_| "Resource archive is damaged")?;
    let mut first = entries
        .next()
        .ok_or("Resource manifest is missing")?
        .map_err(|_| "Resource archive is damaged")?;
    if !first.header().entry_type().is_file()
        || first.path().map_err(|_| "Invalid archive path")?.as_ref() != Path::new("pack.json")
        || first.size() > MAX_MANIFEST
    {
        return Err("Resource manifest is invalid".into());
    }
    let manifest_size = first.size();
    let mut bytes = Vec::new();
    first
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read resource manifest")?;
    let pack: Pack = serde_json::from_slice(&bytes).map_err(|_| "Resource manifest is invalid")?;
    let prefix = if artifact.kind == "model" {
        "bundle/"
    } else {
        "runtime/"
    };
    if pack.schema_version != 1
        || pack.kind != artifact.kind
        || pack.model_version != model_version
        || pack.files.is_empty()
        || pack.files.len() > 50_000
    {
        return Err("Resource pack is incompatible".into());
    }
    let mut remaining = HashMap::new();
    let mut size = manifest_size;
    for item in &pack.files {
        if !safe_path(&item.path)
            || !item.path.starts_with(prefix)
            || !hash_valid(&item.sha256)
            || remaining.insert(item.path.clone(), item).is_some()
        {
            return Err("Resource manifest contains an unsafe or duplicate file".into());
        }
        size = size
            .checked_add(item.size)
            .ok_or("Resource size overflow")?;
    }
    if size != artifact.unpacked_size {
        return Err("Resource unpacked size does not match its catalogue".into());
    }
    if artifact.kind == "runtime" {
        for required in [&pack.python, &pack.site_packages] {
            if !required
                .as_ref()
                .is_some_and(|p| safe_path(p) && p.starts_with("runtime/"))
            {
                return Err("Runtime entry points are invalid".into());
            }
        }
        if !pack
            .python
            .as_ref()
            .is_some_and(|p| remaining.get(p).is_some_and(|f| f.executable))
        {
            return Err("Private Python executable is missing".into());
        }
    }
    drop(first);
    let mut buffer = [0; 128 * 1024];
    for entry in entries {
        check_pause(paused)?;
        let mut entry = entry.map_err(|_| "Resource archive is damaged")?;
        if !entry.header().entry_type().is_file() {
            return Err("Resource links and special files are not allowed".into());
        }
        let path = entry.path().map_err(|_| "Invalid resource path")?;
        let name = path.to_str().ok_or("Invalid resource path")?;
        if !safe_path(name) {
            return Err("Unsafe resource path".into());
        }
        let item = remaining
            .remove(name)
            .ok_or("Resource contains an undeclared or duplicate file")?;
        if entry.size() != item.size {
            return Err("Resource file has an unexpected size".into());
        }
        let target = stage.join(name);
        fs::create_dir_all(target.parent().ok_or("Invalid resource path")?)
            .map_err(|_| "Could not prepare resource storage")?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .map_err(|_| "Could not save resource file")?;
        let mut hash = Sha256::new();
        loop {
            check_pause(paused)?;
            let n = entry
                .read(&mut buffer)
                .map_err(|_| "Resource archive is damaged")?;
            if n == 0 {
                break;
            }
            output
                .write_all(&buffer[..n])
                .map_err(|_| "Could not save resource file. Check free storage.")?;
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != item.sha256 {
            return Err("Resource file failed verification".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            output
                .set_permissions(fs::Permissions::from_mode(if item.executable {
                    0o755
                } else {
                    0o644
                }))
                .map_err(|_| "Could not set private runtime permissions")?;
        }
    }
    if !remaining.is_empty() {
        return Err("Resource pack is incomplete".into());
    }
    Ok(pack)
}

fn rename_install_directory(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // Antivirus and DLL teardown may briefly retain Windows directory handles.
        // Retry only sharing/access conflicts, with a strict bound; never replace
        // permissions or delete the working installation to force activation.
        for _ in 0..30 {
            match fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(error) if matches!(error.raw_os_error(), Some(5 | 32 | 33)) => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(error),
            }
        }
    }
    fs::rename(from, to)
}

/// Call under the recognition/install lock, before deciding which resources are available.
pub fn recover(base: &Path) -> Result<()> {
    if !base.join("active").exists() && base.join("previous").is_dir() {
        rename_install_directory(&base.join("previous"), &base.join("active"))
            .map_err(|_| "Could not restore voice tools")?;
    }
    Ok(())
}
/// A verified stage becomes active without ever deleting the only working installation.
pub fn activate(base: &Path) -> Result<()> {
    recover(base)?;
    let active = base.join("active");
    let previous = base.join("previous");
    let stage = base.join("staging");
    if !stage.join("installation.json").is_file() {
        return Err("Voice tools have not passed setup".into());
    }
    if active.exists() {
        if previous.exists() {
            fs::remove_dir_all(&previous).map_err(|_| "Could not prepare resource update")?;
        }
        rename_install_directory(&active, &previous)
            .map_err(|_| "Could not retain previous voice tools")?;
    }
    if rename_install_directory(&stage, &active).is_err() {
        recover(base)?;
        return Err(
            "Could not activate voice tools; the previous installation was retained".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::atomic::AtomicU64,
    };
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ethiopic-packs-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn artifact() -> Artifact {
        Artifact {
            kind: "model".into(),
            url: format!(
                "https://huggingface.co/ethiopic/studio/resolve/{}/asr.tar.gz",
                "a".repeat(40)
            ),
            sha256: "b".repeat(64),
            size: 1,
            unpacked_size: 1,
        }
    }
    fn release() -> Release {
        let mut runtime = artifact();
        runtime.kind = "runtime".into();
        Release {
            profile: None,
            os: "macos".into(),
            arch: "aarch64".into(),
            model_version: "v0.2".into(),
            minimum_macos: "14.0".into(),
            minimum_windows: None,
            packs: vec![artifact(), runtime],
        }
    }
    #[test]
    fn inference_download_rejects_unpinned_or_untrusted_sources_before_network() {
        let cache = std::env::temp_dir().join("invalid-inference-download");
        for url in ["http://files.pythonhosted.org/packages/a.whl", "https://example.com/model.whl",
            "https://huggingface.co/inworld/voice-isolator/resolve/main/denoiser_realtime.safetensors",
            "https://files.pythonhosted.org/packages/a.whl?token=secret",
            "https://files.pythonhosted.org/packages/%2e%2e/a.whl",
            "https://user:secret@files.pythonhosted.org/packages/a.whl",
            "https://files.pythonhosted.org:444/packages/a.whl"] {
            assert!(download_inference_file(url,&"a".repeat(64),10,&cache,&AtomicBool::new(false),|_|{}).is_err());
        }
        assert!(!cache.exists());
    }
    #[test]
    fn catalog_requires_pinned_first_party_public_resources() {
        let good = artifact();
        assert!(good.validate().is_ok());
        for url in [
            good.url.replace(&"a".repeat(40), "main"),
            good.url.replace("ethiopic/", "someone/"),
            good.url.replace("https:", "http:"),
            good.url.replace("huggingface.co", "example.com"),
            format!("{}?token=secret", good.url),
            good.url.replace("asr.tar.gz", "%2e%2e.tar.gz"),
        ] {
            let mut bad = good.clone();
            bad.url = url;
            assert!(bad.validate().is_err());
        }
        let mut bad = good;
        bad.size = 0;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn catalog_selects_only_supported_unique_complete_profiles() {
        let catalog = Catalog {
            schema_version: 1,
            releases: vec![release()],
        };
        let json = serde_json::to_string(&catalog).unwrap();
        let parsed = Catalog::parse(&json).unwrap();
        assert!(parsed.select("macos", "aarch64").is_some());
        assert!(parsed.select("macos", "x86_64").is_none());
        assert!(Catalog::parse("{\"schema_version\":1,\"releases\":[]}")
            .unwrap()
            .releases
            .is_empty());
        let mut duplicate = catalog.clone();
        duplicate.releases.push(release());
        assert!(Catalog::parse(&serde_json::to_string(&duplicate).unwrap()).is_err());
        let mut incomplete = catalog;
        incomplete.releases[0].packs.pop();
        assert!(Catalog::parse(&serde_json::to_string(&incomplete).unwrap()).is_err());
    }
    #[test]
    fn windows_catalog_requires_x64_and_a_declared_windows_baseline() {
        let mut windows = release();
        windows.os = "windows".into();
        windows.arch = "x86_64".into();
        windows.minimum_macos.clear();
        windows.minimum_windows = Some("10.0.19041".into());
        windows.profile = Some("cpu-high".into());
        let mut catalog = Catalog {
            schema_version: 1,
            releases: vec![windows],
        };
        let parsed = Catalog::parse(&serde_json::to_string(&catalog).unwrap()).unwrap();
        assert!(parsed.select("windows", "x86_64").is_some());
        assert!(parsed.select("macos", "aarch64").is_none());
        catalog.releases[0].arch = "aarch64".into();
        assert!(Catalog::parse(&serde_json::to_string(&catalog).unwrap()).is_err());
        catalog.releases[0].arch = "x86_64".into();
        catalog.releases[0].minimum_windows = None;
        assert!(Catalog::parse(&serde_json::to_string(&catalog).unwrap()).is_err());
    }
    #[test]
    fn multiple_hardware_profiles_share_a_model_version_but_require_explicit_routing() {
        let mut small = release();
        small.profile = Some("cpu-small".into());
        let mut balanced = small.clone();
        balanced.profile = Some("cpu-balanced".into());
        let catalog = Catalog::parse(
            &serde_json::to_string(&Catalog {
                schema_version: 1,
                releases: vec![small, balanced],
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(catalog.releases.len(), 2);
        assert!(
            catalog.select("macos", "aarch64").is_none(),
            "OS-only selection must not silently pick a hardware profile"
        );
    }
    #[test]
    fn unsafe_cross_platform_paths_are_rejected() {
        for path in [
            "",
            "/tmp/evil",
            "../evil",
            "bundle/../evil",
            "bundle//file",
            "bundle/./file",
            "C:/evil",
            "runtime\\evil",
            "file\0",
        ] {
            assert!(!safe_path(path), "{path:?}");
        }
        assert!(safe_path("bundle/model/version.json"));
    }
    fn archive(
        temp: &Temp,
        pack: &Pack,
        files: &[(&str, &[u8])],
        link: bool,
    ) -> (Artifact, PathBuf) {
        let manifest = serde_json::to_vec(pack).unwrap();
        let target = temp.0.join("fixture.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            File::create(&target).unwrap(),
            flate2::Compression::fast(),
        );
        let mut builder = tar::Builder::new(encoder);
        let mut add = |name: &str, data: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o777);
            header.set_cksum();
            builder.append_data(&mut header, name, data).unwrap();
        };
        add("pack.json", &manifest);
        for (path, data) in files {
            add(path, data);
        }
        if link {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            builder
                .append_link(&mut header, "bundle/link", "../../outside")
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        let mut a = artifact();
        a.kind = pack.kind.clone();
        a.size = fs::metadata(&target).unwrap().len();
        a.unpacked_size = manifest.len() as u64 + pack.files.iter().map(|f| f.size).sum::<u64>();
        a.sha256 = sha256(&target, &AtomicBool::new(false)).unwrap();
        (a, target)
    }
    fn pack() -> Pack {
        Pack {
            schema_version: 1,
            kind: "model".into(),
            model_version: "v0.2".into(),
            python: None,
            site_packages: None,
            files: vec![PackFile {
                path: "bundle/model/version.json".into(),
                sha256: hash(b"model"),
                size: 5,
                executable: false,
            }],
        }
    }
    #[test]
    fn verified_archive_restores_only_declared_files_with_safe_permissions() {
        let temp = Temp::new();
        let stage = temp.0.join("stage");
        fs::create_dir(&stage).unwrap();
        let (a, path) = archive(
            &temp,
            &pack(),
            &[("bundle/model/version.json", b"model")],
            false,
        );
        extract(&a, &path, &stage, "v0.2", &AtomicBool::new(false)).unwrap();
        let output = stage.join("bundle/model/version.json");
        assert_eq!(fs::read(&output).unwrap(), b"model");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(output).unwrap().permissions().mode() & 0o777,
                0o644
            );
        }
        assert!(!stage.join("pack.json").exists());
    }
    #[test]
    fn archive_rejects_missing_duplicate_undeclared_and_link_entries() {
        for files in [
            vec![],
            vec![
                ("bundle/model/version.json", b"model".as_slice()),
                ("bundle/model/version.json", b"model".as_slice()),
            ],
            vec![("bundle/unlisted", b"model".as_slice())],
        ] {
            let temp = Temp::new();
            let stage = temp.0.join("stage");
            fs::create_dir(&stage).unwrap();
            let (a, path) = archive(&temp, &pack(), &files, false);
            assert!(extract(&a, &path, &stage, "v0.2", &AtomicBool::new(false)).is_err());
        }
        let temp = Temp::new();
        let stage = temp.0.join("stage");
        fs::create_dir(&stage).unwrap();
        let (a, path) = archive(
            &temp,
            &pack(),
            &[("bundle/model/version.json", b"model")],
            true,
        );
        assert!(extract(&a, &path, &stage, "v0.2", &AtomicBool::new(false))
            .unwrap_err()
            .contains("links"));
        assert!(!temp.0.join("outside").exists());
    }
    #[test]
    fn archive_rejects_content_version_and_size_mismatch() {
        for case in 0..3 {
            let temp = Temp::new();
            let stage = temp.0.join("stage");
            fs::create_dir(&stage).unwrap();
            let mut manifest = pack();
            if case == 0 {
                manifest.files[0].sha256 = hash(b"other");
            }
            let (mut a, path) = archive(
                &temp,
                &manifest,
                &[("bundle/model/version.json", b"model")],
                false,
            );
            if case == 2 {
                a.unpacked_size += 1;
            }
            assert!(extract(
                &a,
                &path,
                &stage,
                if case == 1 { "v0.3" } else { "v0.2" },
                &AtomicBool::new(false)
            )
            .is_err());
        }
    }
    #[test]
    fn interrupted_activation_restores_previous_and_keeps_it_on_update() {
        let temp = Temp::new();
        let base = &temp.0;
        fs::create_dir(base.join("previous")).unwrap();
        fs::write(base.join("previous/installation.json"), "old").unwrap();
        recover(base).unwrap();
        assert_eq!(
            fs::read(base.join("active/installation.json")).unwrap(),
            b"old"
        );
        assert!(activate(base).is_err());
        assert!(base.join("active").is_dir());
        fs::create_dir(base.join("staging")).unwrap();
        fs::write(base.join("staging/installation.json"), "new").unwrap();
        activate(base).unwrap();
        assert_eq!(
            fs::read(base.join("active/installation.json")).unwrap(),
            b"new"
        );
        assert_eq!(
            fs::read(base.join("previous/installation.json")).unwrap(),
            b"old"
        );
    }
    fn server(
        response: impl FnOnce(&str) -> Vec<u8> + Send + 'static,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(connection.try_clone().unwrap());
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                headers.push_str(&line);
            }
            connection.write_all(&response(&headers)).unwrap();
        });
        (format!("http://{address}/fixture"), thread)
    }
    fn net_artifact(url: String) -> Artifact {
        Artifact {
            url,
            sha256: hash(b"abcdef"),
            size: 6,
            ..artifact()
        }
    }
    #[test]
    fn download_resumes_saved_bytes_and_verifies_full_content() {
        let temp = Temp::new();
        fs::write(temp.0.join("download.part"), b"abc").unwrap();
        let (url, thread) = server(|headers| {
            assert!(headers.to_lowercase().contains("range: bytes=3-"));
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef".to_vec()
        });
        let path = download_with(
            &Client::new(),
            &net_artifact(url),
            &temp.0,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        thread.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"abcdef");
    }
    #[test]
    fn sustained_progress_outlives_request_timeouts_without_manual_retry() {
        let temp = Temp::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            for offset in 0..5 {
                let mut connection = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "Missing resumed request"
                            );
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                connection
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(connection.try_clone().unwrap());
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                if offset > 0 {
                    assert!(headers
                        .to_lowercase()
                        .contains(&format!("range: bytes={offset}-")));
                }
                let response = if offset == 0 {
                    "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n".into()
                } else {
                    format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {offset}-5/6\r\nConnection: close\r\n\r\n",6-offset)
                };
                connection.write_all(response.as_bytes()).unwrap();
                if offset < 4 {
                    connection
                        .write_all(&b"abcdef"[offset..offset + 1])
                        .unwrap();
                    connection.flush().unwrap();
                    // A long healthy transfer can cross many per-request deadlines.
                    std::thread::sleep(Duration::from_millis(300));
                } else {
                    connection.write_all(b"ef").unwrap();
                }
            }
        });
        let client = Client::builder()
            .timeout(Duration::from_millis(150))
            .build()
            .unwrap();
        let path = download_with(
            &client,
            &net_artifact(format!("http://{address}/fixture")),
            &temp.0,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        thread.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"abcdef");
    }
    #[test]
    fn range_ignored_restarts_instead_of_appending() {
        let temp = Temp::new();
        fs::write(temp.0.join("download.part"), b"old").unwrap();
        let (url, thread) = server(|_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef".to_vec()
        });
        let path = download_with(
            &Client::new(),
            &net_artifact(url),
            &temp.0,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        thread.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"abcdef");
    }
    #[test]
    fn interrupted_response_keeps_bytes_and_retries_the_remaining_range() {
        let temp = Temp::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            for phase in 0..2 {
                let (mut connection, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(connection.try_clone().unwrap());
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    headers.push_str(&line);
                }
                if phase == 0 {
                    connection
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc",
                        )
                        .unwrap();
                } else {
                    assert!(headers.to_lowercase().contains("range: bytes=3-"));
                    connection.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nConnection: close\r\n\r\ndef").unwrap();
                }
            }
        });
        let artifact = net_artifact(format!("http://{address}/fixture"));
        let output = download_with(
            &Client::new(),
            &artifact,
            &temp.0,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        thread.join().unwrap();
        assert_eq!(fs::read(output).unwrap(), b"abcdef");
    }
    #[test]
    fn wrong_range_never_appends_to_partial() {
        let temp = Temp::new();
        let partial = temp.0.join("download.part");
        fs::write(&partial, b"abc").unwrap();
        let (url, thread) = server(|_| {
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 1-3/6\r\nConnection: close\r\n\r\ndef".to_vec()
        });
        assert!(transfer(
            &Client::new(),
            &net_artifact(url),
            &partial,
            &AtomicBool::new(false),
            &mut |_| {}
        )
        .is_err());
        thread.join().unwrap();
        assert_eq!(fs::read(partial).unwrap(), b"abc");
    }
    #[test]
    fn corrupt_download_is_discarded_without_activation() {
        let temp = Temp::new();
        let (url, thread) = server(|_| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nxxxxxx".to_vec()
        });
        assert!(download_with(
            &Client::new(),
            &net_artifact(url),
            &temp.0,
            &AtomicBool::new(false),
            &mut |_| {}
        )
        .unwrap_err()
        .contains("safety check"));
        thread.join().unwrap();
        assert!(!temp.0.join("download.part").exists());
        assert!(!temp.0.join("download.tar.gz").exists());
    }
    #[test]
    fn pause_does_not_open_network_or_destroy_partial() {
        let temp = Temp::new();
        let partial = temp.0.join("download.part");
        fs::write(&partial, b"abc").unwrap();
        assert_eq!(
            download_with(
                &Client::new(),
                &net_artifact("http://127.0.0.1:1/no-request".into()),
                &temp.0,
                &AtomicBool::new(true),
                &mut |_| {}
            )
            .unwrap_err(),
            "paused"
        );
        assert_eq!(fs::read(partial).unwrap(), b"abc");
    }
}
