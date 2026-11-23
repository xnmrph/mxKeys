//! Resumable, checksum-verified downloads.
//!
//! Files are written to `<dest>.part`; an interrupted download resumes with an HTTP Range
//! request. The sha256 is checked before the file is renamed into place, so a file at
//! `dest` is always complete and verified. Proxies come from the standard environment
//! variables (`HTTPS_PROXY`…) and TLS uses the OS trust store, so corporate proxies work.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("couldn't download {url}: {message}")]
    Http { url: String, message: String },
    #[error("the download of {url} was corrupted (checksum mismatch). It was deleted — please try again.")]
    Checksum {
        url: String,
        expected: String,
        actual: String,
    },
    #[error("couldn't write {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> DownloadError + '_ {
    move |source| DownloadError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// sha256 of a file, hex-encoded.
pub fn sha256_file(path: &Path) -> Result<String, DownloadError> {
    let mut f = File::open(path).map_err(io(path))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(io(path))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("mxKeys/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        // No overall timeout: big files on slow links legitimately take long.
        .timeout(None)
        .build()
        .expect("HTTP client")
}

/// Download `url` to `dest`, verifying `sha256`. `progress(done, total)` is called as
/// bytes arrive (total may be unknown). Returns immediately if `dest` already exists
/// and matches the checksum.
pub fn download(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), DownloadError> {
    if dest.is_file() && sha256_file(dest)?.eq_ignore_ascii_case(sha256) {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let part = dest.with_extension(format!(
        "{}part",
        dest.extension()
            .map(|e| format!("{}.", e.to_string_lossy()))
            .unwrap_or_default()
    ));
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let http = |message: String| DownloadError::Http {
        url: url.to_string(),
        message,
    };
    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut resp = req.send().map_err(|e| http(describe(&e)))?;
    let status = resp.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // The .part is already complete (or bogus) — verify below / restart.
    } else if !status.is_success() {
        return Err(http(format!("the server answered {status}")));
    }
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resumed && status != reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        have = 0; // server ignored the Range header: start over
    }
    let total = resp.content_length().map(|l| l + have);

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&part)
        .map_err(io(&part))?;
    file.set_len(have).map_err(io(&part))?;
    file.seek(SeekFrom::Start(have)).map_err(io(&part))?;
    if status != reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let mut buf = vec![0u8; 1 << 16];
        let mut done = have;
        progress(done, total);
        loop {
            let n = resp.read(&mut buf).map_err(|e| http(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(io(&part))?;
            done += n as u64;
            progress(done, total);
        }
    }
    file.flush().map_err(io(&part))?;
    drop(file);

    let actual = sha256_file(&part)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        let _ = fs::remove_file(&part);
        return Err(DownloadError::Checksum {
            url: url.to_string(),
            expected: sha256.to_string(),
            actual,
        });
    }
    fs::rename(&part, dest).map_err(io(dest))
}

fn describe(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "couldn't connect — check your internet connection (and proxy settings)".into()
    } else if e.is_timeout() {
        "the connection timed out".into()
    } else {
        e.to_string()
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! A minimal HTTP/1.1 file server with Range support, for tests.
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    /// Serve `body` at any path. `cut_after`: close the first connection after this many
    /// body bytes (simulates a dropped connection). Returns the base URL.
    pub fn serve(body: Vec<u8>, cut_after: Option<usize>, honour_range: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body = Arc::new(body);
        std::thread::spawn(move || {
            let mut first = true;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut start = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if honour_range {
                        if let Some(r) = lower.strip_prefix("range: bytes=") {
                            start = r.trim().trim_end_matches('-').parse().unwrap_or(0);
                        }
                    }
                }
                let slice = &body[start.min(body.len())..];
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        slice.len(),
                        start,
                        body.len() - 1,
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        slice.len()
                    )
                };
                let _ = stream.write_all(head.as_bytes());
                let n = match (first, cut_after) {
                    (true, Some(c)) => c.min(slice.len()),
                    _ => slice.len(),
                };
                let _ = stream.write_all(&slice[..n]);
                first = false;
            }
        });
        format!("http://{addr}/file.bin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mxkeys-dl-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn body() -> (Vec<u8>, String) {
        let b: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let h = hex::encode(Sha256::digest(&b));
        (b, h)
    }

    #[test]
    fn downloads_and_verifies() {
        let (b, h) = body();
        let url = test_server::serve(b.clone(), None, true);
        let dir = tmp("ok");
        let dest = dir.join("f.tar.xz");
        let mut calls = 0;
        download(&client(), &url, &dest, &h, |_, _| calls += 1).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b);
        assert!(calls > 1);
        // Second call is a no-op.
        download(&client(), "http://127.0.0.1:9/unreachable", &dest, &h, |_, _| {}).unwrap();
    }

    #[test]
    fn resumes_after_a_dropped_connection() {
        let (b, h) = body();
        let url = test_server::serve(b.clone(), Some(100_000), true);
        let dir = tmp("resume");
        let dest = dir.join("f.zip");
        // First attempt: connection drops → checksum of the partial file fails? No — the
        // reader sees EOF after Content-Length isn't reached, which is an error.
        let first = download(&client(), &url, &dest, &h, |_, _| {});
        assert!(first.is_err());
        let part = dir.join("f.zip.part");
        let partial = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        assert!(partial > 0, "partial data is kept");
        download(&client(), &url, &dest, &h, |_, _| {}).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b);
        assert!(!part.exists());
    }

    #[test]
    fn server_without_range_support_restarts_cleanly() {
        let (b, h) = body();
        let url = test_server::serve(b.clone(), None, false);
        let dir = tmp("norange");
        let dest = dir.join("f.bin");
        fs::write(dir.join("f.bin.part"), &b[..1000]).unwrap();
        download(&client(), &url, &dest, &h, |_, _| {}).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b);
    }

    #[test]
    fn checksum_mismatch_deletes_the_file() {
        let (b, _) = body();
        let url = test_server::serve(b, None, true);
        let dir = tmp("bad");
        let dest = dir.join("f.bin");
        let err = download(&client(), &url, &dest, &"0".repeat(64), |_, _| {}).unwrap_err();
        assert!(matches!(err, DownloadError::Checksum { .. }), "{err}");
        assert!(!dest.exists() && !dir.join("f.bin.part").exists());
    }
}
