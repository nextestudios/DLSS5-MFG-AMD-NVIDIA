//! The app updates itself from this repository's latest stable release (SHA-256 checked like every download).
use std::fs;
use std::path::{Path, PathBuf};

use crate::net::{self, Log};

const REPO: &str = "nextestudios/DLSS5-MFG-AMD-NVIDIA";
const ASSET: &str = "DLSS5-FrameGen.exe";

/// `1.2.0` > `1.1.0` > `1.1.0-rc.2` > `1.1.0-rc.1`. Tags may start with "v".
pub fn newer(candidate: &str, current: &str) -> bool {
    fn key(v: &str) -> Option<(Vec<u64>, bool, String)> {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
        let nums = core.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<u64>>>()?;
        // A final version sorts after its pre-releases.
        Some((nums, pre.is_none(), pre.unwrap_or("").to_string()))
    }
    match (key(candidate), key(current)) {
        (Some((a, af, ap)), Some((b, bf, bp))) => (a, af, pre_key(&ap)) > (b, bf, pre_key(&bp)),
        _ => false,
    }
}

/// "rc.10" > "rc.9": numeric parts compare as numbers.
fn pre_key(p: &str) -> Vec<(u64, String)> {
    p.split('.').map(|s| s.parse().map_or((0, s.to_string()), |n| (n, String::new()))).collect()
}

/// The newer release tag, if there is one. Only on Windows (the asset is the Windows exe).
pub fn check() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let tag = net::latest_tag(REPO, |n| n == ASSET).ok()?;
    newer(&tag, env!("CARGO_PKG_VERSION")).then_some(tag)
}

/// Downloads the new exe and swaps it in. Windows lets a running exe be renamed, not overwritten.
pub fn install(log: Log) -> Result<PathBuf, String> {
    let new = net::latest_release(REPO, |n| n == ASSET, log)?;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    replace(&current, &new.file)?;
    log(format!("App atualizado para {}", new.tag));
    Ok(current)
}

fn old_path(current: &Path) -> PathBuf {
    current.with_extension("old.exe")
}

fn replace(current: &Path, new: &Path) -> Result<(), String> {
    let old = old_path(current);
    let _ = fs::remove_file(&old);
    fs::rename(current, &old).map_err(|e| format!("Não deu para trocar o app: {e}"))?;
    if let Err(e) = fs::copy(new, current) {
        let _ = fs::rename(&old, current);
        return Err(format!("Não deu para gravar a versão nova: {e}"));
    }
    Ok(())
}

/// Turned off by the user when this file exists (on by default).
fn auto_off_flag() -> PathBuf {
    net::cache_dir().with_file_name("auto-update.off")
}

pub fn auto_enabled() -> bool {
    !auto_off_flag().exists()
}

pub fn set_auto(on: bool) {
    let _ = if on { fs::remove_file(auto_off_flag()) } else { fs::write(auto_off_flag(), b"") };
}

/// Deletes the previous exe left by an update (it was still running then).
pub fn cleanup() {
    if let Ok(c) = std::env::current_exe() {
        let _ = fs::remove_file(old_path(&c));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(newer("v1.2.0", "1.1.0"));
        assert!(newer("1.1.0", "1.1.0-rc.2"));
        assert!(newer("1.1.0-rc.10", "1.1.0-rc.9"));
        assert!(newer("2.0.0", "1.99.99"));
        assert!(!newer("v1.1.0", "1.1.0"));
        assert!(!newer("1.0.9", "1.1.0"));
        assert!(!newer("1.1.0-rc.2", "1.1.0"));
        assert!(!newer("garbage", "1.0.0"));
    }

    #[test]
    fn replace_swaps_and_keeps_the_old_one_aside() {
        let d = std::env::temp_dir().join(format!("gpumods-upd-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let (cur, new) = (d.join("app.exe"), d.join("new.exe"));
        fs::write(&cur, "v1").unwrap();
        fs::write(&new, "v2").unwrap();
        replace(&cur, &new).unwrap();
        assert_eq!(fs::read_to_string(&cur).unwrap(), "v2");
        assert_eq!(fs::read_to_string(old_path(&cur)).unwrap(), "v1");
        // A missing download leaves the current exe in place.
        assert!(replace(&cur, &d.join("missing.exe")).is_err());
        assert_eq!(fs::read_to_string(&cur).unwrap(), "v2");
    }
}
