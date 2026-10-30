//! Installed games, read from the store clients' own files and registry (the same sources NeuralHub uses).
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Found {
    pub name: String,
    pub store: &'static str,
    pub dir: PathBuf,
    /// Steam games know their id; it gives the cover art.
    pub steam_appid: Option<u32>,
}

/// Library folder names that are never the game's own name.
const GENERIC: [&str; 9] = ["bin", "bin64", "x64", "win64", "binaries", "content", "game", "retail", "win_x64"];

/// A readable name for a folder picked by hand: the first folder above bin/x64/Binaries/Content...
pub fn display_name(dir: &Path) -> String {
    dir.ancestors().filter_map(|a| a.file_name()).map(|f| f.to_string_lossy().into_owned())
        .find(|n| !GENERIC.contains(&n.to_lowercase().as_str()))
        .unwrap_or_else(|| dir.display().to_string())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Manual {
    name: String,
    dir: PathBuf,
}

fn manual_file() -> PathBuf {
    crate::net::cache_dir().parent().map_or_else(std::env::temp_dir, Path::to_path_buf).join("manual-games.json")
}

fn load_manual(file: &Path) -> Vec<Found> {
    let list: Vec<Manual> = fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.into_iter().map(|m| Found { name: m.name, store: "Manual", dir: m.dir, steam_appid: None }).collect()
}

fn save_manual(file: &Path, games: &[Found]) -> Result<(), String> {
    let list: Vec<Manual> = games.iter().map(|g| Manual { name: g.name.clone(), dir: g.dir.clone() }).collect();
    fs::write(file, serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn add_manual_to(file: &Path, dir: &Path) -> Result<Found, String> {
    let mut list = load_manual(file);
    let found = Found { name: display_name(dir), store: "Manual", dir: dir.to_path_buf(), steam_appid: None };
    list.retain(|g| g.dir != found.dir);
    list.push(found.clone());
    save_manual(file, &list)?;
    Ok(found)
}

/// Remembers a game added by hand (folder picker or drag and drop) so it shows up in the library next time.
pub fn add_manual(dir: &Path) -> Result<Found, String> {
    add_manual_to(&manual_file(), dir)
}

pub fn remove_manual(dir: &Path) -> Result<(), String> {
    let file = manual_file();
    let mut list = load_manual(&file);
    list.retain(|g| g.dir != dir);
    save_manual(&file, &list)
}

/// Every `"key" "value"` pair in a Steam .vdf/.acf file, unescaped.
fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let q: Vec<&str> = l.split('"').collect();
            // `\t"key"\t\t"value"` splits into ["\t", key, "\t\t", value, ""]
            (q.len() >= 5).then(|| (q[1].to_lowercase(), q[3].replace("\\\\", "\\")))
        })
        .collect()
}

fn steam(root: &Path) -> Vec<Found> {
    let vdf = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
    let mut libs: Vec<PathBuf> = vdf_pairs(&vdf).into_iter().filter(|(k, _)| k == "path").map(|(_, v)| PathBuf::from(v)).collect();
    libs.push(root.to_path_buf());
    libs.dedup();
    let mut out = Vec::new();
    for lib in libs {
        for e in fs::read_dir(lib.join("steamapps")).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            let kv = vdf_pairs(&fs::read_to_string(e.path()).unwrap_or_default());
            let get = |k: &str| kv.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
            let (Some(name), Some(dir)) = (get("name"), get("installdir")) else { continue };
            if ["Steamworks", "Proton", "Steam Linux Runtime", "SteamVR"].iter().any(|s| name.starts_with(s)) {
                continue;
            }
            let steam_appid = get("appid").and_then(|a| a.parse().ok());
            out.push(Found { name, store: "Steam", dir: lib.join("steamapps/common").join(dir), steam_appid });
        }
    }
    out
}

fn epic(manifests: &Path) -> Vec<Found> {
    fs::read_dir(manifests).into_iter().flatten().flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "item"))
        .filter_map(|e| {
            let j: serde_json::Value = serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok()?;
            Some(Found { name: j["DisplayName"].as_str()?.into(), store: "Epic", dir: j["InstallLocation"].as_str()?.into(), steam_appid: None })
        })
        .collect()
}

/// `X:\.GamingRoot` lists the Xbox app's library folders: "RGBX", a count, then UTF-16 paths.
fn xbox(drive: &Path) -> Vec<Found> {
    let Ok(b) = fs::read(drive.join(".GamingRoot")) else { return Vec::new() };
    let wide: Vec<u16> = b.get(8..).unwrap_or_default().as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    String::from_utf16_lossy(&wide).split('\0').filter(|s| !s.is_empty())
        .flat_map(|lib| fs::read_dir(drive.join(lib)).into_iter().flatten().flatten())
        .filter(|e| e.path().join("Content").is_dir())
        .map(|e| Found { name: e.file_name().to_string_lossy().into(), store: "Xbox", dir: e.path().join("Content"), steam_appid: None })
        .collect()
}

#[cfg(windows)]
pub mod reg {
    use windows::Win32::System::Registry::*;
    use windows::core::{HSTRING, PWSTR};

    pub use windows::Win32::System::Registry::{HKEY_CURRENT_USER as HKCU, HKEY_LOCAL_MACHINE as HKLM};

    pub fn string(root: HKEY, path: &str, value: &str) -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe {
            RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
        };
        let len = (size as usize / 2).saturating_sub(1);
        (r.is_ok() && len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
    }

    pub fn subkeys(root: HKEY, path: &str) -> Vec<String> {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) }.is_err() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            if unsafe { RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) }.is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
        out
    }
}

/// Games registered as `<base>\<game>` with the install folder in `value`.
#[cfg(windows)]
fn registry_store(base: &str, value: &str, name_value: Option<&str>, store: &'static str) -> Vec<Found> {
    reg::subkeys(reg::HKLM, base).into_iter()
        .filter_map(|k| {
            let path = format!("{base}\\{k}");
            let dir = PathBuf::from(reg::string(reg::HKLM, &path, value)?);
            let name = name_value.and_then(|n| reg::string(reg::HKLM, &path, n))
                .unwrap_or_else(|| dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(k));
            Some(Found { name, store, dir, steam_appid: None })
        })
        .collect()
}

#[cfg(windows)]
pub fn scan() -> Vec<Found> {
    let mut all = Vec::new();
    let steam_root = reg::string(reg::HKCU, r"Software\Valve\Steam", "SteamPath")
        .or_else(|| reg::string(reg::HKLM, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"));
    if let Some(r) = steam_root {
        all.extend(steam(Path::new(&r)));
    }
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    all.extend(epic(&Path::new(&program_data).join(r"Epic\EpicGamesLauncher\Data\Manifests")));
    all.extend(registry_store(r"SOFTWARE\WOW6432Node\GOG.com\Games", "path", Some("gameName"), "GOG"));
    all.extend(registry_store(r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs", "InstallDir", None, "Ubisoft"));
    all.extend(registry_store(r"SOFTWARE\WOW6432Node\EA Games", "Install Dir", None, "EA"));
    all.extend(registry_store(r"SOFTWARE\WOW6432Node\Rockstar Games", "InstallFolder", None, "Rockstar"));
    for d in b'C'..=b'Z' {
        all.extend(xbox(Path::new(&format!("{}:\\", d as char))));
    }
    all.extend(load_manual(&manual_file()));
    finish(all)
}

/// Stores only exist on Windows; elsewhere the library has the games added by hand.
#[cfg(not(windows))]
pub fn scan() -> Vec<Found> {
    finish(load_manual(&manual_file()))
}

/// Drops missing folders and duplicates (same folder from two stores), sorted by name.
fn finish(mut all: Vec<Found>) -> Vec<Found> {
    let mut seen = std::collections::HashSet::new();
    all.retain(|g| g.dir.is_dir() && seen.insert(g.dir.clone()));
    all.sort_by_key(|g| g.name.to_lowercase());
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_library_and_manifests() {
        let root = std::env::temp_dir().join(format!("gpumods-steam-{}", std::process::id()));
        let lib2 = root.join("lib2");
        fs::create_dir_all(root.join("steamapps/common/Cyber")).unwrap();
        fs::create_dir_all(lib2.join("steamapps/common/Proton 9.0")).unwrap();
        fs::write(root.join("steamapps/libraryfolders.vdf"),
            format!("\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n", lib2.display().to_string().replace('\\', "\\\\"))).unwrap();
        fs::write(root.join("steamapps/appmanifest_1.acf"), "\"AppState\"\n{\n\t\"appid\"\t\t\"1091500\"\n\t\"name\"\t\t\"Cyberpunk 2077\"\n\t\"installdir\"\t\t\"Cyber\"\n}\n").unwrap();
        fs::write(lib2.join("steamapps/appmanifest_2.acf"), "\t\"name\"\t\t\"Proton 9.0\"\n\t\"installdir\"\t\t\"Proton 9.0\"\n").unwrap();
        let g = finish(steam(&root));
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "Cyberpunk 2077");
        assert_eq!(g[0].dir, root.join("steamapps/common/Cyber"));
        assert_eq!(g[0].steam_appid, Some(1091500));
    }

    #[test]
    fn epic_manifest_and_xbox_root() {
        let d = std::env::temp_dir().join(format!("gpumods-stores-{}", std::process::id()));
        let game = d.join("Alan Wake 2");
        fs::create_dir_all(&game).unwrap();
        fs::write(d.join("a.item"), serde_json::json!({"DisplayName": "Alan Wake 2", "InstallLocation": game}).to_string()).unwrap();
        assert_eq!(epic(&d)[0].dir, game);

        fs::create_dir_all(d.join("XboxGames/Forza/Content")).unwrap();
        let mut root = b"RGBX\x01\x00\x00\x00".to_vec();
        root.extend("XboxGames\0".encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(d.join(".GamingRoot"), root).unwrap();
        let x = xbox(&d);
        assert_eq!((x[0].name.as_str(), x[0].dir.clone()), ("Forza", d.join("XboxGames/Forza/Content")));
    }

    #[test]
    fn vdf_unescapes_windows_paths_and_ignores_sections() {
        let kv = vdf_pairs("\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t\t\"label\"\t\t\"\"\n\t}\n}\n");
        assert_eq!(kv, [("path".to_string(), r"D:\SteamLibrary".to_string()), ("label".to_string(), String::new())]);
    }

    #[test]
    fn finish_drops_missing_and_duplicate_folders_and_sorts() {
        let d = std::env::temp_dir().join(format!("gpumods-finish-{}", std::process::id()));
        fs::create_dir_all(d.join("a")).unwrap();
        fs::create_dir_all(d.join("b")).unwrap();
        let f = |name: &str, store, dir: &str| Found { name: name.into(), store, dir: d.join(dir), steam_appid: None };
        let out = finish(vec![f("zeta", "Steam", "a"), f("Alpha", "Epic", "b"), f("Zeta again", "GOG", "a"), f("gone", "EA", "missing")]);
        let names: Vec<&str> = out.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta"]);
    }

    #[test]
    fn epic_skips_broken_manifests_and_xbox_needs_content() {
        let d = std::env::temp_dir().join(format!("gpumods-broken-{}", std::process::id()));
        fs::create_dir_all(d.join("XboxGames/NoContent")).unwrap();
        fs::create_dir_all(d.join("Games2/Halo/Content")).unwrap();
        fs::write(d.join("bad.item"), "{ not json").unwrap();
        fs::write(d.join("nofields.item"), "{}").unwrap();
        assert!(epic(&d).is_empty());
        let mut root = b"RGBX\x02\x00\x00\x00".to_vec();
        root.extend("XboxGames\0Games2\0".encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(d.join(".GamingRoot"), root).unwrap();
        let x = xbox(&d);
        assert_eq!(x.len(), 1);
        assert_eq!(x[0].name, "Halo");
        assert!(xbox(&d.join("no-drive")).is_empty());
    }

    #[test]
    fn display_name_skips_generic_folders() {
        assert_eq!(display_name(Path::new("/g/Cyberpunk 2077/bin/x64")), "Cyberpunk 2077");
        assert_eq!(display_name(Path::new("/x/XboxGames/Forza Horizon 5/Content")), "Forza Horizon 5");
        assert_eq!(display_name(Path::new("/g/Stalker2/Binaries/Win64")), "Stalker2");
        assert_eq!(display_name(Path::new("/g/Elden Ring/Game")), "Elden Ring");
    }

    #[test]
    fn manual_games_round_trip() {
        let d = std::env::temp_dir().join(format!("gpumods-manual-{}", std::process::id()));
        let game = d.join("Alan Wake 2");
        fs::create_dir_all(&game).unwrap();
        let file = d.join("manual.json");
        assert!(load_manual(&file).is_empty());
        let a = add_manual_to(&file, &game).unwrap();
        assert_eq!((a.name.as_str(), a.store), ("Alan Wake 2", "Manual"));
        add_manual_to(&file, &game).unwrap();
        assert_eq!(load_manual(&file).len(), 1, "adding twice keeps one");
        fs::write(&file, "garbage").unwrap();
        assert!(load_manual(&file).is_empty(), "a broken file is ignored, not fatal");
    }
}
