//! Small, resumable fixture downloader for the Phase 0 prototype.
use reqwest::{blocking::Client, header, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub struct DownloadRequest {
    pub url: String,
    pub output_dir: PathBuf,
    pub expected_sha256: String,
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Downloading,
    Verifying,
    Complete,
    Paused,
}
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub state: DownloadState,
}
#[derive(Debug)]
pub enum DownloadOutcome {
    Complete(PathBuf),
    Paused,
}
#[derive(Debug)]
pub struct DownloadError(pub String);
impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DownloadError {}
impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}
impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        Self(e.to_string())
    }
}
#[derive(Serialize, Deserialize)]
struct Resume {
    url: String,
    hash: String,
    etag: String,
}
fn invalid(message: &str) -> DownloadError {
    DownloadError(message.into())
}
fn strong_etag(value: Option<&header::HeaderValue>) -> Option<String> {
    value
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.starts_with('"') && s.ends_with('"'))
        .map(str::to_owned)
}
fn range(value: &str) -> Option<(u64, u64, u64)> {
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    (start <= end && end < total).then_some((start, end, total))
}

fn matches_digest(path: &std::path::Path, expected: &str) -> Result<bool, DownloadError> {
    if !path.exists() {
        return Ok(false);
    }
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 32768];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected))
}
/// Inspect local progress without accessing the network. Completed files are hash checked.
pub fn inspect(request: &DownloadRequest) -> Result<Progress, DownloadError> {
    let destination = request.output_dir.join("fixture.bin");
    if matches_digest(&destination, &request.expected_sha256)? {
        let size = fs::metadata(destination)?.len();
        return Ok(Progress {
            downloaded: size,
            total: Some(size),
            state: DownloadState::Complete,
        });
    }
    let saved = fs::read(request.output_dir.join("fixture.resume.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Resume>(&b).ok());
    let bytes = if saved.is_some_and(|m| {
        m.url == request.url && m.hash.eq_ignore_ascii_case(&request.expected_sha256)
    }) {
        fs::metadata(request.output_dir.join("fixture.part"))
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        0
    };
    Ok(Progress {
        downloaded: bytes,
        total: None,
        state: DownloadState::Paused,
    })
}

/// The caller supplies a trusted fixture URL and digest; this function never discovers sources.
/// A pause preserves verified identity metadata and the partial file for the next call.
pub fn download(
    request: &DownloadRequest,
    paused: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<DownloadOutcome, DownloadError> {
    if request.expected_sha256.len() != 64
        || !request
            .expected_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid(
            "Expected SHA256 must contain 64 hexadecimal characters",
        ));
    }
    fs::create_dir_all(&request.output_dir)?;
    let partial = request.output_dir.join("fixture.part");
    let metadata = request.output_dir.join("fixture.resume.json");
    let destination = request.output_dir.join("fixture.bin");
    // A crash may leave a complete partial file before its final rename.
    if matches_digest(&destination, &request.expected_sha256)? {
        let size = fs::metadata(&destination)?.len();
        progress(Progress {
            downloaded: size,
            total: Some(size),
            state: DownloadState::Complete,
        });
        return Ok(DownloadOutcome::Complete(destination));
    }
    if matches_digest(&partial, &request.expected_sha256)? {
        fs::rename(&partial, &destination)?;
        let _ = fs::remove_file(&metadata);
        let size = fs::metadata(&destination)?.len();
        progress(Progress {
            downloaded: size,
            total: Some(size),
            state: DownloadState::Complete,
        });
        return Ok(DownloadOutcome::Complete(destination));
    }
    let saved = fs::read(&metadata)
        .ok()
        .and_then(|b| serde_json::from_slice::<Resume>(&b).ok())
        .filter(|s| s.url == request.url && s.hash.eq_ignore_ascii_case(&request.expected_sha256));
    let mut offset = if saved.is_some() {
        fs::metadata(&partial).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    if paused.load(Ordering::SeqCst) {
        progress(Progress {
            downloaded: offset,
            total: None,
            state: DownloadState::Paused,
        });
        return Ok(DownloadOutcome::Paused);
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut builder = client
        .get(&request.url)
        .header(header::ACCEPT_ENCODING, "identity");
    if offset > 0 {
        builder = builder
            .header(header::RANGE, format!("bytes={offset}-"))
            .header(header::IF_RANGE, &saved.as_ref().unwrap().etag);
    }
    let mut response = builder.send()?;
    // A server may legitimately replace the object or ignore Range. Its 200 response
    // starts a new file; a mismatching partial response is never appended.
    if response.status() == StatusCode::RANGE_NOT_SATISFIABLE
        || (response.status() == StatusCode::PARTIAL_CONTENT
            && offset > 0
            && strong_etag(response.headers().get(header::ETAG)).as_deref()
                != saved.as_ref().map(|s| s.etag.as_str()))
    {
        offset = 0;
        response = client
            .get(&request.url)
            .header(header::ACCEPT_ENCODING, "identity")
            .send()?;
    }
    let total = if response.status() == StatusCode::OK {
        offset = 0;
        response.content_length()
    } else if response.status() == StatusCode::PARTIAL_CONTENT && offset > 0 {
        let (start, end, total) = response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|h| h.to_str().ok())
            .and_then(range)
            .ok_or_else(|| invalid("Malformed Content-Range"))?;
        if start != offset
            || end != total - 1
            || response
                .content_length()
                .is_some_and(|n| n != end - start + 1)
        {
            return Err(invalid("Content-Range does not match requested remainder"));
        }
        Some(total)
    } else {
        return Err(invalid(&format!(
            "Unexpected download status {}",
            response.status()
        )));
    };
    let etag = strong_etag(response.headers().get(header::ETAG));
    if let Some(etag) = etag {
        fs::write(
            &metadata,
            serde_json::to_vec(&Resume {
                url: request.url.clone(),
                hash: request.expected_sha256.clone(),
                etag,
            })
            .map_err(|e| invalid(&e.to_string()))?,
        )?;
    } else {
        let _ = fs::remove_file(&metadata);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(offset == 0)
        .append(offset > 0)
        .open(&partial)?;
    let mut downloaded = offset;
    progress(Progress {
        downloaded,
        total,
        state: DownloadState::Downloading,
    });
    let mut buffer = [0u8; 32 * 1024];
    loop {
        if paused.load(Ordering::SeqCst) {
            file.sync_all()?;
            progress(Progress {
                downloaded,
                total,
                state: DownloadState::Paused,
            });
            return Ok(DownloadOutcome::Paused);
        }
        let n = response.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        downloaded += n as u64;
        progress(Progress {
            downloaded,
            total,
            state: DownloadState::Downloading,
        });
    }
    file.sync_all()?;
    drop(file);
    if total.is_some_and(|n| n != downloaded) {
        return Err(invalid("Incomplete download; resume to retry"));
    }
    progress(Progress {
        downloaded,
        total,
        state: DownloadState::Verifying,
    });
    let mut hasher = Sha256::new();
    let mut file = File::open(&partial)?;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    if format!("{:x}", hasher.finalize()) != request.expected_sha256.to_ascii_lowercase() {
        fs::remove_file(&partial)?;
        let _ = fs::remove_file(&metadata);
        return Err(invalid(
            "SHA256 verification failed; corrupt download discarded",
        ));
    }
    drop(file);
    fs::rename(&partial, &destination)?;
    let _ = fs::remove_file(&metadata);
    progress(Progress {
        downloaded,
        total,
        state: DownloadState::Complete,
    });
    Ok(DownloadOutcome::Complete(destination))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn setup(responses: Vec<String>) -> (DownloadRequest, thread::JoinHandle<Vec<String>>) {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture", server.local_addr().unwrap());
        let handle = thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut socket, _) = server.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut b = [0];
                        socket.read_exact(&mut b).unwrap();
                        request.push(b[0]);
                        if request.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    socket.write_all(response.as_bytes()).unwrap();
                    String::from_utf8(request).unwrap()
                })
                .collect()
        });
        let output_dir = std::env::temp_dir().join(format!(
            "ethiopic-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        (
            DownloadRequest {
                url,
                output_dir,
                expected_sha256: format!("{:x}", Sha256::digest(b"abcdef")),
            },
            handle,
        )
    }
    fn seed(r: &DownloadRequest) {
        fs::create_dir_all(&r.output_dir).unwrap();
        fs::write(r.output_dir.join("fixture.part"), b"abc").unwrap();
        fs::write(
            r.output_dir.join("fixture.resume.json"),
            serde_json::to_vec(&Resume {
                url: r.url.clone(),
                hash: r.expected_sha256.clone(),
                etag: "\"v1\"".into(),
            })
            .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn resumes_with_range_and_validator() {
        let (r,server)=setup(vec!["HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nETag: \"v1\"\r\n\r\ndef".into()]);
        seed(&r);
        assert!(matches!(
            download(&r, &AtomicBool::new(false), |_| {}).unwrap(),
            DownloadOutcome::Complete(_)
        ));
        assert_eq!(
            fs::read(r.output_dir.join("fixture.bin")).unwrap(),
            b"abcdef"
        );
        let requests = server.join().unwrap();
        assert!(requests[0].contains("range: bytes=3-"));
        assert!(requests[0].contains("if-range: \"v1\""));
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn ignored_range_restarts_instead_of_appending() {
        let (r, server) = setup(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nETag: \"v2\"\r\n\r\nabcdef".into(),
        ]);
        seed(&r);
        download(&r, &AtomicBool::new(false), |_| {}).unwrap();
        server.join().unwrap();
        assert_eq!(
            fs::read(r.output_dir.join("fixture.bin")).unwrap(),
            b"abcdef"
        );
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn changed_validator_requests_fresh_copy() {
        let (r, server) = setup(vec![
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nETag: \"v2\"\r\n\r\ndef".into(),
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nETag: \"v2\"\r\n\r\nabcdef".into(),
        ]);
        seed(&r);
        download(&r, &AtomicBool::new(false), |_| {}).unwrap();
        let requests = server.join().unwrap();
        assert!(requests[0].contains("range: bytes=3-"));
        assert!(!requests[1].contains("range:"));
        assert_eq!(
            fs::read(r.output_dir.join("fixture.bin")).unwrap(),
            b"abcdef"
        );
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn malformed_range_preserves_existing_partial() {
        let (r,server)=setup(vec!["HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/6\r\nETag: \"v1\"\r\n\r\ndef".into()]);
        seed(&r);
        assert!(download(&r, &AtomicBool::new(false), |_| {}).is_err());
        server.join().unwrap();
        assert_eq!(fs::read(r.output_dir.join("fixture.part")).unwrap(), b"abc");
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn corrupt_download_never_becomes_installed() {
        let (r, server) = setup(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nbroken".into()
        ]);
        assert!(download(&r, &AtomicBool::new(false), |_| {})
            .unwrap_err()
            .0
            .contains("SHA256"));
        server.join().unwrap();
        assert!(!r.output_dir.join("fixture.bin").exists());
        assert!(!r.output_dir.join("fixture.part").exists());
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn paused_before_request_can_resume_on_next_invocation() {
        let (r,server)=setup(vec!["HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nETag: \"v1\"\r\n\r\ndef".into()]);
        seed(&r);
        assert!(matches!(
            download(&r, &AtomicBool::new(true), |_| {}).unwrap(),
            DownloadOutcome::Paused
        ));
        assert_eq!(inspect(&r).unwrap().downloaded, 3);
        assert!(matches!(
            download(&r, &AtomicBool::new(false), |_| {}).unwrap(),
            DownloadOutcome::Complete(_)
        ));
        server.join().unwrap();
        // Reusing the installed file must work without a running server.
        assert!(matches!(
            download(&r, &AtomicBool::new(false), |_| {}).unwrap(),
            DownloadOutcome::Complete(_)
        ));
        assert!(matches!(
            inspect(&r).unwrap().state,
            DownloadState::Complete
        ));
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn fully_downloaded_partial_is_promoted_without_network() {
        let (r, server) = setup(vec![]);
        server.join().unwrap();
        seed(&r);
        fs::write(r.output_dir.join("fixture.part"), b"abcdef").unwrap();
        assert!(matches!(
            download(&r, &AtomicBool::new(false), |_| {}).unwrap(),
            DownloadOutcome::Complete(_)
        ));
        assert!(!r.output_dir.join("fixture.part").exists());
        fs::remove_dir_all(r.output_dir).unwrap();
    }
    #[test]
    fn pause_preserves_partial() {
        let body = "abcdef".repeat(12000);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"v1\"\r\n\r\n{}",
            body.len(),
            body
        );
        let (mut r, server) = setup(vec![response]);
        r.expected_sha256 = format!("{:x}", Sha256::digest(body.as_bytes()));
        let pause = AtomicBool::new(false);
        assert!(matches!(
            download(&r, &pause, |p| {
                if p.downloaded > 0 {
                    pause.store(true, Ordering::SeqCst);
                }
            })
            .unwrap(),
            DownloadOutcome::Paused
        ));
        server.join().unwrap();
        assert!(r.output_dir.join("fixture.part").exists());
        assert!(!r.output_dir.join("fixture.bin").exists());
        fs::remove_dir_all(r.output_dir).unwrap();
    }
}
