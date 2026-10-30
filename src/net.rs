//! Latest stable GitHub releases + download cache.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ureq::tls::{TlsConfig, TlsProvider};

pub type Log<'a> = &'a dyn Fn(String);

const UA: &str = "gpu-mods-app";

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).build())
            .build()
            .new_agent()
    })
}

/// GitHub API request. Honors GITHUB_TOKEN (CI), which lifts the 60 requests/hour anonymous limit.
fn api(path: &str) -> Result<ureq::Body, ureq::Error> {
    let mut req = agent().get(&format!("https://api.github.com/{path}")).header("User-Agent", UA);
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    req.call().map(|r| r.into_body())
}

pub fn get_json(url: &str) -> Result<serde_json::Value, String> {
    agent().get(url).header("User-Agent", UA).call().and_then(|r| r.into_body().read_json()).map_err(|e| e.to_string())
}

pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("GpuMods").join("cache");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect()
}

pub fn download(url: &str, dest: &Path, referer: Option<&str>, log: Log) -> Result<(), String> {
    if dest.exists() {
        log(format!("Já em cache: {}", dest.file_name().unwrap_or_default().to_string_lossy()));
        return Ok(());
    }
    log(format!("Baixando {url}"));
    let mut req = agent().get(url).header("User-Agent", UA);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    let res = req.call().map_err(|e| format!("Falha no download de {url}: {e}"))?;
    let part = dest.with_extension("part");
    let mut file = fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let copied = io::copy(&mut res.into_body().into_reader(), &mut file);
    drop(file);
    if let Err(e) = copied {
        let _ = fs::remove_file(&part);
        return Err(format!("Download interrompido ({url}): {e}"));
    }
    fs::rename(&part, dest).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// "sha256:<hex>", published by GitHub for every asset uploaded since 2025.
    digest: Option<String>,
}

pub struct Fetched {
    pub tag: String,
    pub file: PathBuf,
}

/// Tag, file name and URL of the latest stable release asset whose name matches `pick`.
/// Repos with only pre-releases (nightlies) have no "latest": then the newest release is used.
fn latest_asset(repo: &str, pick: fn(&str) -> bool) -> Result<(String, Asset), String> {
    let get = |path: &str| api(&format!("repos/{repo}/{path}"));
    let rel: Release = match get("releases/latest") {
        Err(ureq::Error::StatusCode(404)) => get("releases?per_page=1")
            .and_then(|mut b| b.read_json::<Vec<Release>>())
            .map_err(|e| e.to_string())?
            .into_iter().next().ok_or("nenhuma release publicada")?,
        r => r.and_then(|mut b| b.read_json()).map_err(|e| e.to_string())?,
    };
    let asset = rel.assets.into_iter().find(|a| pick(&a.name))
        .ok_or_else(|| format!("a release {} não tem o arquivo esperado", rel.tag_name))?;
    Ok((rel.tag_name, asset))
}

/// Newest cache entry starting with `prefix` whose last `__` part passes `ok`.
fn newest_cached(prefix: &str, ok: impl Fn(&str) -> bool) -> Option<fs::DirEntry> {
    fs::read_dir(cache_dir()).ok().into_iter().flatten().flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(prefix) && !n.ends_with(".part") && ok(n.rsplit("__").next().unwrap_or(""))
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
}

/// Downloads the asset of the latest stable release whose name matches `pick`.
/// Offline, falls back to the newest copy already in the cache.
pub fn latest_release(repo: &str, pick: fn(&str) -> bool, log: Log) -> Result<Fetched, String> {
    let prefix = format!("{}__", safe(repo));
    let online = latest_asset(repo, pick).and_then(|(tag, a)| {
        let file = cache_dir().join(format!("{prefix}{}__{}", safe(&tag), a.name));
        let fresh = !file.exists();
        download(&a.browser_download_url, &file, None, log)?;
        if fresh && let Some(want) = a.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
            let got = sha256(&file)?;
            if !got.eq_ignore_ascii_case(want) {
                let _ = fs::remove_file(&file);
                return Err(format!("{}: hash SHA-256 não confere (arquivo corrompido ou adulterado), download apagado", a.name));
            }
            log(format!("SHA-256 conferido: {}", a.name));
        }
        Ok(Fetched { tag, file })
    });
    online.or_else(|err| {
        let c = newest_cached(&prefix, |n| pick(n) && !n.ends_with(".dir"))
            .ok_or_else(|| format!("Não foi possível baixar {repo}: {err}"))?;
        let name = c.file_name().to_string_lossy().into_owned();
        log(format!("GitHub indisponível para {repo} ({err}); usando o cache: {name}"));
        Ok(Fetched { tag: name.split("__").nth(1).unwrap_or("cache").to_string(), file: c.path() })
    })
}

/// Pulls only the `wanted` entries out of the latest release zip, without downloading the rest
/// (HTTP range requests). Uses the full zip instead when it is already in the cache.
pub fn zip_files(repo: &str, pick: fn(&str) -> bool, wanted: &[&str], log: Log) -> Result<Fetched, String> {
    let prefix = format!("{}__", safe(repo));
    let (tag, name, url) = match latest_asset(repo, pick) {
        Ok((tag, a)) => (tag, a.name, a.browser_download_url),
        Err(err) => {
            let c = newest_cached(&prefix, |n| n == "files").filter(|c| c.path().join(".ok").exists())
                .ok_or_else(|| format!("Não foi possível baixar {repo}: {err}"))?;
            log(format!("GitHub indisponível para {repo} ({err}); usando o cache"));
            return Ok(Fetched { tag: "cache".into(), file: c.path() });
        }
    };
    let dest = cache_dir().join(format!("{prefix}{}__files", safe(&tag)));
    if dest.join(".ok").exists() {
        log(format!("Já em cache: {} ({tag})", wanted.join(", ")));
        return Ok(Fetched { tag, file: dest });
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let full = cache_dir().join(format!("{prefix}{}__{name}", safe(&tag)));
    let res = if full.exists() {
        fs::File::open(&full).map_err(|e| e.to_string()).and_then(|f| unzip_some(f, wanted, &dest, log))
    } else {
        log(format!("Baixando só {} de {url}", wanted.join(", ")));
        Remote::open(&url).and_then(|r| unzip_some(r, wanted, &dest, log))
    };
    res.map_err(|e| format!("{repo} {tag}: {e}"))?;
    fs::write(dest.join(".ok"), b"").map_err(|e| e.to_string())?;
    Ok(Fetched { tag, file: dest })
}

pub fn sha256(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = io::Read::read(&mut f, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn unzip_some(r: impl io::Read + io::Seek, wanted: &[&str], dest: &Path, log: Log) -> Result<(), String> {
    let mut z = zip::ZipArchive::new(r).map_err(|e| e.to_string())?;
    for w in wanted {
        let mut entry = z.by_name(w).map_err(|e| format!("{w}: {e}"))?;
        let name = w.rsplit('/').next().unwrap_or(w);
        let part = dest.join(format!("{name}.part"));
        let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
        // io::copy runs the zip crate's CRC check at the end of the entry.
        io::copy(&mut entry, &mut out).map_err(|e| format!("{w}: {e}"))?;
        drop(out);
        fs::rename(&part, dest.join(name)).map_err(|e| e.to_string())?;
        log(format!("{name} pronto"));
    }
    Ok(())
}

/// A remote file read through HTTP range requests, 8 MB at a time.
struct Remote {
    url: String,
    len: u64,
    pos: u64,
    start: u64,
    buf: Vec<u8>,
}

impl Remote {
    const CHUNK: u64 = 8 << 20;

    fn open(url: &str) -> Result<Remote, String> {
        let res = agent().get(url).header("User-Agent", UA).header("Range", "bytes=0-0").call().map_err(|e| e.to_string())?;
        // "bytes 0-0/629518374"
        let len = res.headers().get("content-range").and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next()).and_then(|v| v.parse().ok())
            .ok_or("o servidor não aceita download parcial")?;
        Ok(Remote { url: url.to_string(), len, pos: 0, start: 0, buf: Vec::new() })
    }
}

impl io::Read for Remote {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        if self.pos < self.start || self.pos >= self.start + self.buf.len() as u64 {
            let end = (self.pos + Self::CHUNK).min(self.len) - 1;
            let res = agent().get(&self.url).header("User-Agent", UA).header("Range", format!("bytes={}-{end}", self.pos))
                .call().map_err(io::Error::other)?;
            if res.status() != 206 {
                return Err(io::Error::other("o servidor ignorou o download parcial"));
            }
            self.buf.clear();
            res.into_body().into_reader().read_to_end(&mut self.buf)?;
            self.start = self.pos;
        }
        let off = (self.pos - self.start) as usize;
        let n = out.len().min(self.buf.len() - off);
        out[..n].copy_from_slice(&self.buf[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl io::Seek for Remote {
    fn seek(&mut self, to: io::SeekFrom) -> io::Result<u64> {
        let p = match to {
            io::SeekFrom::Start(p) => p as i64,
            io::SeekFrom::End(d) => self.len as i64 + d,
            io::SeekFrom::Current(d) => self.pos as i64 + d,
        };
        self.pos = u64::try_from(p).map_err(|_| io::Error::other("seek antes do início"))?;
        Ok(self.pos)
    }
}

#[derive(serde::Deserialize)]
struct Tag {
    name: String,
}

/// ReShade with add-on support. Version comes from crosire/reshade tags; the file from reshade.me.
pub fn reshade(log: Log) -> Result<Fetched, String> {
    let tags: Vec<Tag> = api("repos/crosire/reshade/tags").and_then(|mut b| b.read_json()).unwrap_or_default();
    let version = tags.iter().map(|t| t.name.trim_start_matches('v'))
        .find(|v| v.split('.').count() == 3 && v.split('.').all(|p| p.parse::<u32>().is_ok()));
    let Some(v) = version else {
        let c = newest_cached("ReShade_Setup_", |n| n.ends_with(".exe"))
            .ok_or("Não foi possível descobrir a versão do ReShade (sem internet?)")?;
        return Ok(Fetched { tag: "cache".into(), file: c.path() });
    };
    let name = format!("ReShade_Setup_{v}_Addon.exe");
    let file = cache_dir().join(&name);
    download(&format!("https://reshade.me/downloads/{name}"), &file, Some("https://reshade.me/"), log)
        .map_err(|e| format!("{e}\nBaixe 'ReShade with full add-on support' em https://reshade.me e salve em {}", file.display()))?;
    Ok(Fetched { tag: v.to_string(), file })
}

/// Extracts .zip, .7z or a self-extracting .exe (zip appended) once, into `<archive>.dir`.
pub fn extract(archive: &Path) -> Result<PathBuf, String> {
    let dest = PathBuf::from(format!("{}.dir", archive.display()));
    let ok = dest.join(".ok");
    if ok.exists() {
        return Ok(dest);
    }
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let is_7z = archive.extension().is_some_and(|e| e.eq_ignore_ascii_case("7z"));
    let res = if is_7z {
        sevenz_rust2::decompress_file(archive, &dest).map_err(|e| e.to_string())
    } else {
        fs::File::open(archive).map_err(|e| e.to_string())
            .and_then(|f| zip::ZipArchive::new(f).map_err(|e| e.to_string()))
            .and_then(|mut z| z.extract(&dest).map_err(|e| e.to_string()))
    };
    res.map_err(|e| format!("Não foi possível extrair {}: {e}", archive.display()))?;
    fs::write(ok, b"").map_err(|e| e.to_string())?;
    Ok(dest)
}

/// First file named `name` (case-insensitive) under `dir`.
pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Network test: `cargo test -- --ignored`. Checks the real packages still have what we expect.
    use std::io::{Cursor, Read, Seek, SeekFrom, Write};

    fn zip_bytes(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// Minimal HTTP server for `data`. With `ranges` false it ignores Range headers, like some CDNs.
    fn serve(data: Vec<u8>, ranges: bool) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/pkg.zip", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut conn in listener.incoming().flatten() {
                let mut req = Vec::new();
                let mut b = [0u8; 1];
                while !req.ends_with(b"\r\n\r\n") && conn.read(&mut b).unwrap_or(0) == 1 {
                    req.push(b[0]);
                }
                let req = String::from_utf8_lossy(&req).to_lowercase();
                let range = req.lines().find_map(|l| l.strip_prefix("range: bytes=")).and_then(|r| {
                    let (a, b) = r.trim().split_once('-')?;
                    Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                });
                let resp = match range.filter(|_| ranges) {
                    Some((a, b)) => {
                        let b = b.min(data.len() - 1);
                        let mut r = format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len(), b - a + 1).into_bytes();
                        r.extend_from_slice(&data[a..=b]);
                        r
                    }
                    None => {
                        let mut r = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len()).into_bytes();
                        r.extend_from_slice(&data);
                        r
                    }
                };
                let _ = conn.write_all(&resp);
            }
        });
        url
    }

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8 ^ seed).collect()
    }

    #[test]
    fn unzip_some_extracts_only_wanted_entries() {
        let z = zip_bytes(&[("a/keep.dll", b"keep".to_vec()), ("b/skip.dll", b"skip".to_vec())]);
        let dest = std::env::temp_dir().join(format!("gpumods-unzip-{}", std::process::id()));
        fs::create_dir_all(&dest).unwrap();
        unzip_some(Cursor::new(z.clone()), &["a/keep.dll"], &dest, &|_| {}).unwrap();
        assert_eq!(fs::read(dest.join("keep.dll")).unwrap(), b"keep");
        assert!(!dest.join("skip.dll").exists() && !dest.join("keep.dll.part").exists());
        assert!(unzip_some(Cursor::new(z), &["missing.dll"], &dest, &|_| {}).unwrap_err().contains("missing.dll"));
    }

    #[test]
    fn remote_reads_and_seeks_across_chunks() {
        // Bigger than one 8 MB chunk, so reads cross a chunk boundary.
        let data = pattern(20 << 20, 0);
        let mut r = Remote::open(&serve(data.clone(), true)).unwrap();
        assert_eq!(r.len, data.len() as u64);

        let mut buf = vec![0u8; 64];
        let at = (8 << 20) - 32;
        r.seek(SeekFrom::Start(at as u64)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf, data[at..at + 64]);

        r.seek(SeekFrom::End(-10)).unwrap();
        let mut tail = Vec::new();
        r.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, data[data.len() - 10..]);

        r.seek(SeekFrom::Start(5)).unwrap();
        r.seek(SeekFrom::Current(-2)).unwrap();
        let mut b3 = [0u8; 3];
        r.read_exact(&mut b3).unwrap();
        assert_eq!(b3, data[3..6]);
        assert!(r.seek(SeekFrom::Current(-100)).is_err());
    }

    #[test]
    fn remote_zip_entry_matches_original() {
        let big = pattern(12 << 20, 7);
        let z = zip_bytes(&[("OptiScaler/streamline/nvngx_dlssnr.dll", big.clone()), ("small.ini", b"x=1".to_vec())]);
        let dest = std::env::temp_dir().join(format!("gpumods-remotezip-{}", std::process::id()));
        fs::create_dir_all(&dest).unwrap();
        let r = Remote::open(&serve(z, true)).unwrap();
        unzip_some(r, &["OptiScaler/streamline/nvngx_dlssnr.dll", "small.ini"], &dest, &|_| {}).unwrap();
        assert_eq!(fs::read(dest.join("nvngx_dlssnr.dll")).unwrap(), big);
        assert_eq!(fs::read(dest.join("small.ini")).unwrap(), b"x=1");
    }

    #[test]
    fn remote_refuses_servers_without_ranges() {
        let err = Remote::open(&serve(pattern(1000, 0), false)).err().unwrap();
        assert!(err.contains("parcial"), "{err}");
    }

    #[test]
    fn safe_names_for_cache_files() {
        assert_eq!(safe("optiscaler/OptiScaler"), "optiscaler_OptiScaler");
        assert_eq!(safe("v1.0 beta+2"), "v1.0_beta_2");
    }

    #[test]
    fn sha256_known_value() {
        let p = std::env::temp_dir().join(format!("gpumods-sha-{}", std::process::id()));
        fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    #[ignore]
    fn real_packages_extract() {
        let log = |s: String| println!("{s}");
        let opti = latest_release("optiscaler/OptiScaler", |n| n.ends_with(".7z") || n.ends_with(".zip"), &log).unwrap();
        assert!(find_file(&extract(&opti.file).unwrap(), "OptiScaler.ini").is_some());
        let rs = reshade(&log).unwrap();
        assert!(find_file(&extract(&rs.file).unwrap(), "ReShade64.dll").is_some());
        let mfg = latest_release("mavismmg/MFGAdaUnlock-RenoDx", |n| n.ends_with(".addon64"), &log).unwrap();
        assert!(mfg.file.exists());
        let nightly = latest_release("optiscaler/OptiScaler-nightly", |n| n.ends_with(".7z") || n.ends_with(".zip"), &log).unwrap();
        assert!(find_file(&extract(&nightly.file).unwrap(), "libxess_fg.dll").is_some());
        let mu = latest_release("dashdogy/MFG-Unlocker", |n| n.starts_with("MFGUnlocker") && n.ends_with(".zip"), &log).unwrap();
        assert!(find_file(&extract(&mu.file).unwrap(), "MFGUnlocker.dll").is_some());
        assert!(latest_release("dashdogy/OptiScaler", |n| n == "OptiScaler.dll", &log).unwrap().file.exists());
        // Range requests: two small entries out of a 600 MB zip.
        let f = zip_files("ShyVortex/dlss-unlocked", |n| n.contains("standalone") && n.ends_with(".zip"),
            &["OptiScaler.ini", "OptiScaler/nvngx.ini"], &log).unwrap();
        assert!(f.file.join("OptiScaler.ini").exists() && f.file.join("nvngx.ini").exists());
    }
}
