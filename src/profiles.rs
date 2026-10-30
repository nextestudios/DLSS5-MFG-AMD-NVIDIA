//! Per-game knowledge (proxy name, anti-cheat, tips) from `profiles.json` in the repository,
//! plus anti-cheat detection from the game's own files.
use std::fs;
use std::path::Path;

use crate::art::normalize;
use crate::games::Found;
use crate::install::PROXY_CHOICES;
use crate::net;

const URL: &str = "https://raw.githubusercontent.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/main/profiles.json";
/// Copy built into the exe, used offline and before the first download.
const EMBEDDED: &str = include_str!("../profiles.json");

#[derive(serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Profile {
    #[serde(default)]
    pub names: Vec<String>,
    pub steam_appid: Option<u32>,
    #[serde(default)]
    pub exes: Vec<String>,
    pub proxy: Option<String>,
    pub anticheat: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(serde::Deserialize)]
struct File {
    games: Vec<Profile>,
}

/// Parses a profiles file. Proxies outside the allow-list are dropped: the file comes from the internet.
fn parse(text: &str) -> Option<Vec<Profile>> {
    let mut games = serde_json::from_str::<File>(text).ok()?.games;
    for g in &mut games {
        if g.proxy.as_deref().is_some_and(|p| !PROXY_CHOICES.contains(&p)) {
            g.proxy = None;
        }
    }
    Some(games)
}

/// The repository's current profiles (cached for a day), else the copy built into the app.
pub fn load() -> Vec<Profile> {
    let file = net::cache_dir().join("profiles.json");
    let fresh = fs::metadata(&file).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() < 86_400);
    if !fresh {
        let _ = fs::remove_file(file.with_extension("part"));
        if net::download(URL, &file.with_extension("new"), None, &|_| {}).is_ok() {
            let new = file.with_extension("new");
            if fs::read_to_string(&new).ok().as_deref().and_then(parse).is_some() {
                let _ = fs::rename(&new, &file);
            } else {
                let _ = fs::remove_file(new);
            }
        }
    }
    fs::read_to_string(&file).ok().as_deref().and_then(parse)
        .or_else(|| parse(EMBEDDED))
        .unwrap_or_default()
}

/// The profile for a game: by Steam id, then by name, then by an .exe in its folder.
pub fn find<'a>(profiles: &'a [Profile], game: &Found, exe_dir: Option<&Path>) -> Option<&'a Profile> {
    let name = normalize(&game.name);
    profiles.iter().find(|p| game.steam_appid.is_some() && p.steam_appid == game.steam_appid)
        .or_else(|| profiles.iter().find(|p| p.names.iter().any(|n| normalize(n) == name)))
        .or_else(|| {
            let dir = exe_dir?;
            profiles.iter().find(|p| p.exes.iter().any(|e| dir.join(e).is_file()))
        })
}

/// Anti-cheat shipped with the game, found by its files (EAC, BattlEye, EA Javelin).
pub fn detect_anticheat(root: &Path) -> Option<&'static str> {
    fn walk(dir: &Path, depth: u8) -> Option<&'static str> {
        for e in fs::read_dir(dir).ok()?.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            let hit = if n.starts_with("easyanticheat") || n == "start_protected_game.exe" || n == "eac_launcher.exe" {
                Some("Easy Anti-Cheat")
            } else if n == "battleye" || n.starts_with("beservice") || n.starts_with("battleye") {
                Some("BattlEye")
            } else if n == "eaanticheat" || n.starts_with("eaanticheat") {
                Some("EA Javelin")
            } else {
                None
            };
            if hit.is_some() {
                return hit;
            }
            if depth > 0 && e.file_type().is_ok_and(|t| t.is_dir()) && let Some(h) = walk(&e.path(), depth - 1) {
                return Some(h);
            }
        }
        None
    }
    walk(root, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_profiles_parse_and_are_safe() {
        let p = parse(EMBEDDED).expect("profiles.json must parse");
        assert!(p.len() >= 10);
        assert!(p.iter().all(|g| !g.names.is_empty() || g.steam_appid.is_some() || !g.exes.is_empty()));
        assert!(p.iter().all(|g| g.proxy.as_deref().is_none_or(|x| PROXY_CHOICES.contains(&x))));
    }

    #[test]
    fn remote_proxies_outside_the_allow_list_are_dropped() {
        let p = parse(r#"{"games":[{"names":["X"],"proxy":"..\\evil.dll"},{"names":["Y"],"proxy":"version.dll"}]}"#).unwrap();
        assert_eq!(p[0].proxy, None);
        assert_eq!(p[1].proxy.as_deref(), Some("version.dll"));
        assert!(parse("not json").is_none());
    }

    #[test]
    fn finds_by_appid_name_and_exe() {
        let profiles = parse(r#"{"games":[
            {"names":["Counter-Strike 2"],"steam_appid":730,"anticheat":"VAC"},
            {"names":["ELDEN RING"],"notes":["x"]},
            {"exes":["Special.exe"],"proxy":"winmm.dll"}]}"#).unwrap();
        let g = |name: &str, appid| Found { name: name.into(), steam_appid: appid, ..Found::default() };
        assert_eq!(find(&profiles, &g("cs2 renamed", Some(730)), None).unwrap().anticheat.as_deref(), Some("VAC"));
        assert!(find(&profiles, &g("Elden Ring", None), None).is_some(), "names match ignoring case and symbols");
        let dir = std::env::temp_dir().join(format!("gpumods-prof-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Special.exe"), "x").unwrap();
        assert_eq!(find(&profiles, &g("Whatever", None), Some(&dir)).unwrap().proxy.as_deref(), Some("winmm.dll"));
        assert!(find(&profiles, &g("Unknown", None), Some(&dir.join("nope"))).is_none());
    }

    #[test]
    fn detects_anticheat_files() {
        let base = std::env::temp_dir().join(format!("gpumods-ac-{}", std::process::id()));
        let cases = [
            ("eac/EasyAntiCheat/Settings.json", Some("Easy Anti-Cheat")),
            ("eac2/start_protected_game.exe", Some("Easy Anti-Cheat")),
            ("be/Game/BattlEye/BEClient_x64.dll", Some("BattlEye")),
            ("be2/BEService_x64.exe", Some("BattlEye")),
            ("ea/EAAntiCheat/x.dll", Some("EA Javelin")),
            ("clean/bin/x64/Game.exe", None),
        ];
        for (path, want) in cases {
            let f = base.join(path);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(&f, "x").unwrap();
            let root = base.join(path.split('/').next().unwrap());
            assert_eq!(detect_anticheat(&root), want, "{path}");
        }
        assert_eq!(detect_anticheat(&base.join("missing")), None);
    }
}
