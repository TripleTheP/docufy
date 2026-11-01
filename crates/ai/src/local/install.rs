//! Downloading the runtime and the models: resumable (an interrupted download goes on where it
//! stopped), checked against the catalog's size and SHA-256 before a file is used, and stoppable.
//! Blocking; the front end calls it on a worker thread.
//!
//! Layout under [`super::data_dir`]: `runtime/<version>/` with the libraries, `models/<id>/<build>/`
//! with a model's files. A folder counts as installed once its `installed` marker is written,
//! after every file was checked.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::catalog::{Build, CatalogModel};
use super::runtime;

/// Written last into a folder whose files were all checked.
const MARKER: &str = "installed";

/// What a download reports while it runs.
#[derive(Debug, Default)]
pub struct Progress {
    /// Bytes downloaded or found already downloaded.
    pub done: AtomicU64,
    /// Bytes in all.
    pub total: AtomicU64,
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(ureq::Proxy::try_from_env())
        .http_status_as_error(false)
        .user_agent(concat!("Docufy/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        // A stall of a minute between chunks is an error; the next try resumes.
        .timeout_per_read(Some(Duration::from_secs(60)))
        .build()
        .new_agent()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads `url` to `dest`, which must end up `size` bytes with SHA-256 `sha256`. A `.part` file
/// left by an earlier try is continued. Adds to `progress.done` as bytes arrive. Stops with
/// `Err("cancelled")` when `cancel` is set, keeping the part for later.
pub fn download(url: &str, dest: &Path, size: u64, sha256: &str, cancel: &AtomicBool, progress: &Progress) -> Result<(), String> {
    let part = PathBuf::from(format!("{}.part", dest.display()));
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut hasher = Sha256::new();
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > size {
        let _ = std::fs::remove_file(&part);
        have = 0;
    }
    // What is already there counts toward the hash and the progress.
    if have > 0 {
        let mut f = File::open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        progress.done.fetch_add(have, Ordering::Relaxed);
    }
    if have < size {
        let mut req = agent().get(url);
        if have > 0 {
            req = req.header("range", format!("bytes={have}-"));
        }
        let resp = req.call().map_err(|e| format!("{url}: {e}"))?;
        let status = resp.status().as_u16();
        let mut file = OpenOptions::new().create(true).write(true).truncate(false).open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        match status {
            206 if have > 0 => {
                file.seek(SeekFrom::Start(have)).map_err(|e| e.to_string())?;
            }
            200 => {
                // The server sent the whole file: start over.
                progress.done.fetch_sub(have, Ordering::Relaxed);
                have = 0;
                hasher = Sha256::new();
                file.set_len(0).map_err(|e| e.to_string())?;
                file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            }
            s => return Err(format!("HTTP {s} for {url}")),
        }
        let mut body = resp.into_body().into_reader();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let n = body.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            have += n as u64;
            if have > size {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(format!("{url} is larger than expected"));
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            progress.done.fetch_add(n as u64, Ordering::Relaxed);
        }
        file.flush().map_err(|e| e.to_string())?;
    }
    if have != size {
        return Err(format!("{url} ended after {have} of {size} bytes; try again to continue"));
    }
    let got = hex(&hasher.finalize());
    if got != sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{url} does not match its checksum (got {got}); the download was discarded"));
    }
    std::fs::rename(&part, dest).map_err(|e| format!("{}: {e}", dest.display()))
}

/// Whether `dir` holds a completed installation.
pub fn is_installed(dir: &Path) -> bool {
    dir.join(MARKER).is_file()
}

/// Downloads the runtime into `dir` (`runtime/<version>`), taking the libraries out of each
/// package and removing the package.
pub fn install_runtime(dir: &Path, cancel: &AtomicBool, progress: &Progress) -> Result<(), String> {
    if !runtime::supported() {
        return Err("local models need Windows on x64 for now".into());
    }
    progress.total.store(runtime::download_bytes(), Ordering::Relaxed);
    let packages = dir.join("packages");
    for p in &runtime::PACKAGES {
        let done = p.entries.iter().all(|(_, name)| dir.join(name).is_file());
        if done {
            progress.done.fetch_add(p.size, Ordering::Relaxed);
            continue;
        }
        let file = packages.join(p.url.rsplit('/').next().unwrap_or("package"));
        if !file.is_file() {
            download(p.url, &file, p.size, p.sha256, cancel, progress)?;
        } else {
            progress.done.fetch_add(p.size, Ordering::Relaxed);
        }
        extract(&file, p.entries, dir)?;
        let _ = std::fs::remove_file(&file);
    }
    let _ = std::fs::remove_dir(&packages);
    std::fs::write(dir.join(MARKER), runtime::VERSION).map_err(|e| e.to_string())
}

/// Takes `entries` (path in the archive, file name) out of the zip archive `archive` into `dir`.
fn extract(archive: &Path, entries: &[(&str, &str)], dir: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", archive.display()))?;
    for (inner, name) in entries {
        let mut entry = zip.by_name(inner).map_err(|e| format!("{inner}: {e}"))?;
        let tmp = dir.join(format!("{name}.part"));
        let mut out = File::create(&tmp).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        drop(out);
        std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Downloads `model`'s `build` into `dir` (`models/<id>/<build>`).
pub fn install_model(model: &CatalogModel, build: Build, dir: &Path, cancel: &AtomicBool, progress: &Progress) -> Result<(), String> {
    let variant = model.variant(build).ok_or_else(|| format!("{} has no {} build", model.name, build.id()))?;
    progress.total.store(variant.bytes(), Ordering::Relaxed);
    for f in variant.files {
        let dest = dir.join(f.name);
        if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) == f.size {
            progress.done.fetch_add(f.size, Ordering::Relaxed);
            continue;
        }
        download(&variant.url(f), &dest, f.size, f.sha256, cancel, progress)?;
    }
    std::fs::write(dir.join(MARKER), variant.revision).map_err(|e| e.to_string())
}

/// Removes an installed (or half-downloaded) folder.
pub fn remove(dir: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    /// A server for `body` that honours `Range: bytes=N-`, for `requests` requests.
    fn serve(body: Vec<u8>, requests: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().take(requests) {
                let mut stream = stream.unwrap();
                let mut from = 0usize;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(r) = line.to_lowercase().strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                let part = &body[from..];
                let status = if from > 0 { "206 Partial Content" } else { "200 OK" };
                let head = format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", part.len());
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(part).unwrap();
            }
        });
        url
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("docufy-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_download_is_checked_and_resumed() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let sha = hex(&Sha256::digest(&body));
        let d = dir("resume");
        let dest = d.join("model.bin");
        std::fs::create_dir_all(&d).unwrap();
        // An earlier try left the first 100 000 bytes.
        std::fs::write(d.join("model.bin.part"), &body[..100_000]).unwrap();
        let url = serve(body.clone(), 1);
        let progress = Progress::default();
        download(&url, &dest, body.len() as u64, &sha, &AtomicBool::new(false), &progress).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(progress.done.load(Ordering::Relaxed), body.len() as u64);
        assert!(!d.join("model.bin.part").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_wrong_checksum_discards_the_download() {
        let body = b"not the model".to_vec();
        let d = dir("checksum");
        let dest = d.join("model.bin");
        let url = serve(body.clone(), 1);
        let e = download(&url, &dest, body.len() as u64, &"0".repeat(64), &AtomicBool::new(false), &Progress::default()).unwrap_err();
        assert!(e.contains("checksum"), "{e}");
        assert!(!dest.exists() && !d.join("model.bin.part").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cancelled_download_keeps_its_part() {
        let body = vec![7u8; 50_000];
        let d = dir("cancel");
        let dest = d.join("model.bin");
        let url = serve(body.clone(), 1);
        let e = download(&url, &dest, body.len() as u64, &hex(&Sha256::digest(&body)), &AtomicBool::new(true), &Progress::default()).unwrap_err();
        assert_eq!(e, "cancelled");
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn libraries_are_taken_out_of_a_package() {
        let d = dir("extract");
        std::fs::create_dir_all(&d).unwrap();
        let archive = d.join("p.nupkg");
        let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
        zip.start_file("runtimes/win-x64/native/onnxruntime.dll", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"MZ library").unwrap();
        zip.finish().unwrap();
        extract(&archive, &[("runtimes/win-x64/native/onnxruntime.dll", "onnxruntime.dll")], &d).unwrap();
        assert_eq!(std::fs::read(d.join("onnxruntime.dll")).unwrap(), b"MZ library");
        assert!(extract(&archive, &[("missing.dll", "x.dll")], &d).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
