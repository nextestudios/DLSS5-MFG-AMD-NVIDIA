//! Cover art from Steam's CDN (poster, background, logo), cached on disk.
//! Games from other stores get Steam's art when a Steam game has exactly the same name.
use std::fs;
use std::path::PathBuf;

use crate::games::Found;
use crate::net;

#[derive(Clone, Debug, Default)]
pub struct Art {
    /// 600x900 cover.
    pub poster: Option<PathBuf>,
    /// 1920x620 background.
    pub hero: Option<PathBuf>,
    /// Transparent title logo.
    pub logo: Option<PathBuf>,
}

/// "Forza Horizon 5™" and "ForzaHorizon5" both become "forzahorizon5".
pub fn normalize(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The search result whose name is the same game (DLCs and soundtracks have longer names).
fn pick(items: &serde_json::Value, name: &str) -> Option<u32> {
    let want = normalize(name);
    items["items"].as_array()?.iter()
        .find(|i| i["name"].as_str().is_some_and(|n| normalize(n) == want))
        .and_then(|i| i["id"].as_u64()).and_then(|id| u32::try_from(id).ok())
}

/// Percent-encodes a URL query value ("Baldur's Gate 3" -> "Baldur%27s%20Gate%203").
fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn dir() -> PathBuf {
    let d = net::cache_dir().join("covers");
    let _ = fs::create_dir_all(&d);
    d
}

fn search_appid(name: &str) -> Option<u32> {
    let key = normalize(name);
    if key.is_empty() {
        return None;
    }
    let cached = dir().join(format!("name-{key}.id"));
    if let Ok(t) = fs::read_to_string(&cached) {
        return t.trim().parse().ok();
    }
    let q = encode(name);
    let j = net::get_json(&format!("https://store.steampowered.com/api/storesearch/?term={q}&cc=us&l=en")).ok()?;
    let id = pick(&j, name);
    let _ = fs::write(&cached, id.map(|i| i.to_string()).unwrap_or_default());
    id
}

/// One CDN file, downloaded once. A miss is remembered so it is not asked again.
fn steam_file(appid: u32, file: &str) -> Option<PathBuf> {
    let path = dir().join(format!("{appid}_{file}"));
    let miss = dir().join(format!("{appid}_{file}.none"));
    if path.exists() {
        return Some(path);
    }
    if miss.exists() {
        return None;
    }
    let url = format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{appid}/{file}");
    match net::download(&url, &path, None, &|_| {}) {
        Ok(()) => Some(path),
        Err(_) => {
            let _ = fs::write(miss, b"");
            None
        }
    }
}

pub fn fetch(game: &Found) -> Art {
    let Some(appid) = game.steam_appid.or_else(|| search_appid(&game.name)) else { return Art::default() };
    Art {
        poster: steam_file(appid, "library_600x900.jpg"),
        hero: steam_file(appid, "library_hero.jpg"),
        logo: steam_file(appid, "logo.png"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_across_stores() {
        assert_eq!(normalize("Forza Horizon 5™"), "forzahorizon5");
        assert_eq!(normalize("ForzaHorizon5"), normalize("Forza Horizon 5"));
        assert_eq!(normalize("Marvel's Spider-Man 2"), "marvelsspiderman2");
    }

    #[test]
    fn query_is_percent_encoded() {
        assert_eq!(encode("Baldur's Gate 3"), "Baldur%27s%20Gate%203");
        assert_eq!(encode("Pokémon"), "Pok%C3%A9mon");
    }

    #[test]
    fn picks_the_game_not_its_dlc() {
        let r = serde_json::json!({"items": [
            {"id": 2138330, "name": "Cyberpunk 2077: Phantom Liberty"},
            {"id": 1091500, "name": "Cyberpunk 2077"}
        ]});
        assert_eq!(pick(&r, "Cyberpunk 2077"), Some(1091500));
        let r = serde_json::json!({"items": [{"id": 3274290, "name": "Beat Saber - Alan Walker - \"Wake Up\""}]});
        assert_eq!(pick(&r, "Alan Wake 2"), None, "no cover is better than a wrong one");
        assert_eq!(pick(&serde_json::json!({}), "x"), None);
    }

    /// `cargo test -- --ignored`: real CDN and store search.
    #[test]
    #[ignore]
    fn real_art() {
        let a = fetch(&Found { name: "Cyberpunk 2077".into(), store: "Epic", ..Found::default() });
        assert!(a.poster.is_some() && a.hero.is_some() && a.logo.is_some());
    }
}
