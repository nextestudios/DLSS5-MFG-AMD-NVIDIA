//! Cover art (poster, background) and store details from Steam's official store APIs, cached on disk.
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
    pub appid: Option<u32>,
    pub details: Option<Details>,
}

/// "Sobre" data from the Steam store, in Portuguese.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Details {
    pub description: String,
    pub developers: String,
    pub publishers: String,
    pub release: String,
    pub genres: Vec<String>,
}

/// Steam's official `appdetails` answer (the `data` object) to Details. Cached per app.
fn parse_details(d: &serde_json::Value) -> Option<Details> {
    let list = |k: &str| d[k].as_array().map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let text = d["short_description"].as_str()?;
    // Store text comes with HTML entities.
    let description = [("&quot;", "\""), ("&#39;", "'"), ("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">")]
        .iter().fold(text.to_string(), |s, (e, c)| s.replace(e, c));
    Some(Details {
        description,
        developers: list("developers"),
        publishers: list("publishers"),
        release: d["release_date"]["date"].as_str().unwrap_or_default().to_string(),
        genres: d["genres"].as_array().into_iter().flatten().filter_map(|g| g["description"].as_str().map(str::to_string)).collect(),
    })
}

fn details(appid: u32) -> Option<Details> {
    let cached = dir().join(format!("{appid}.details.json"));
    let data = match fs::read_to_string(&cached).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        Some(v) => v,
        None => {
            let j = net::get_json(&format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=brazilian")).ok()?;
            let v = j[appid.to_string()]["data"].clone();
            let _ = fs::write(&cached, v.to_string());
            v
        }
    };
    parse_details(&data)
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

/// Library art file names from Steam's official store API. Newer games only have hashed paths
/// ("<hash>/library_capsule.jpg"), so the names have to come from here. Cached per app.
fn assets(appid: u32) -> serde_json::Value {
    let cached = dir().join(format!("{appid}.assets.json"));
    if let Some(v) = fs::read_to_string(&cached).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        return v;
    }
    let q = encode(&format!(r#"{{"ids":[{{"appid":{appid}}}],"context":{{"language":"english","country_code":"US"}},"data_request":{{"include_assets":true}}}}"#));
    let Ok(j) = net::get_json(&format!("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={q}")) else {
        return serde_json::Value::Null;
    };
    let a = j["response"]["store_items"][0]["assets"].clone();
    if a.is_object() {
        let _ = fs::write(&cached, a.to_string());
    }
    a
}

/// One CDN file, downloaded once. A miss is remembered so it is not asked again.
fn steam_file(appid: u32, file: &str) -> Option<PathBuf> {
    let path = dir().join(format!("{appid}_{}", file.replace('/', "_")));
    let miss = path.with_extension("none");
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
    let a = assets(appid);
    let named = |key: &str, old: &str| steam_file(appid, a[key].as_str().unwrap_or(old)).or_else(|| steam_file(appid, old));
    Art {
        poster: named("library_capsule_2x", "library_600x900.jpg"),
        hero: named("library_hero", "library_hero.jpg"),
        appid: Some(appid),
        details: details(appid),
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
    fn store_details_in_portuguese() {
        let d = serde_json::json!({
            "short_description": "O clássico &quot;Dead Space&quot; volta.",
            "developers": ["Motive"], "publishers": ["Electronic Arts", "EA"],
            "release_date": {"date": "27/jan./2023"},
            "genres": [{"description": "Ação"}, {"description": "Aventura"}]
        });
        assert_eq!(parse_details(&d), Some(Details {
            description: "O clássico \"Dead Space\" volta.".into(),
            developers: "Motive".into(),
            publishers: "Electronic Arts, EA".into(),
            release: "27/jan./2023".into(),
            genres: vec!["Ação".into(), "Aventura".into()],
        }));
        assert_eq!(parse_details(&serde_json::Value::Null), None, "no data, no Sobre card");
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
        assert!(a.poster.is_some() && a.hero.is_some());
        let d = a.details.unwrap();
        assert!(d.developers.contains("CD PROJEKT") && !d.description.is_empty() && !d.genres.is_empty(), "{d:?}");
    }
}
