//! Puts files in the game folder with backups, edits INIs and undoes everything.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::gpu::{Gpu, Tier};
use crate::net::{self, Log};
use crate::plan::{self, Opti};

const MANIFEST: &str = ".gpu-mods-files.txt";
const STATE: &str = ".gpu-mods-state.json";

/// What was installed in a game, so the app can show it and redo it with newer versions.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct State {
    pub gpu: String,
    pub dlss5: bool,
    pub mult: u8,
    pub versions: std::collections::BTreeMap<String, String>,
    /// Unix seconds.
    pub when: u64,
    /// Proxy name chosen for this game (None = automatic).
    #[serde(default)]
    pub proxy: Option<String>,
}

/// Names the game can load our DLL as. OptiScaler and MFG Unlocker both accept all of them.
/// Also the allow-list for proxies coming from (remote) game profiles.
pub const PROXY_CHOICES: [&str; 4] = ["dxgi.dll", "version.dll", "winmm.dll", "winhttp.dll"];

fn valid_proxy(p: Option<&str>) -> Result<Option<&str>, String> {
    match p {
        Some(p) if !PROXY_CHOICES.contains(&p) => Err(format!("nome de proxy não permitido: {p}")),
        p => Ok(p),
    }
}

pub fn read_state(game_dir: &Path) -> Option<State> {
    serde_json::from_str(&fs::read_to_string(game_dir.join(STATE)).ok()?).ok()
}

const BACKUP: &str = ".gpumods-backup";
const PROXIES: [&str; 7] = ["dxgi.dll", "winmm.dll", "version.dll", "dbghelp.dll", "d3d12.dll", "wininet.dll", "winhttp.dll"];
const SKIP_EXE: [&str; 19] = [
    "unins", "crash", "redist", "setup", "dxsetup", "dotnet", "easyanticheat", "eac_", "battleye", "be_service",
    "launcher", "helper", "report", "prereq", "cefprocess", "webhelper", "vc_", "epicgames", "eosbootstrapper",
];
const SKIP_PACKAGE: [&str; 4] = ["setup_windows.bat", "setup_linux.sh", "install.sh", ".ok"];

fn game_exes(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .filter(|p| {
            let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            !SKIP_EXE.iter().any(|s| n.contains(s))
        })
        .collect()
}

/// The folder with the game's main .exe (where proxies must go), from a folder or an .exe.
pub fn resolve_game_dir(p: &Path) -> Result<PathBuf, String> {
    if p.is_file() {
        return p.parent().map(Path::to_path_buf).ok_or_else(|| "Caminho inválido".into());
    }
    if !p.is_dir() {
        return Err(format!("Pasta não encontrada: {}", p.display()));
    }
    for sub in ["", "bin64", "Binaries/Win64", "bin/x64", "bin/win64", "Binaries/x64", "bin", "Game/Binaries/Win64"] {
        let d = p.join(sub);
        if !game_exes(&d).is_empty() {
            return Ok(d);
        }
    }
    // Unreal: <Game>/<Project>/Binaries/Win64/<Project>-Win64-Shipping.exe
    let mut stack = vec![(p.to_path_buf(), 0)];
    let mut biggest: Option<(u64, PathBuf)> = None;
    while let Some((dir, depth)) = stack.pop() {
        for exe in game_exes(&dir) {
            let name = exe.file_name().unwrap_or_default().to_string_lossy().to_string();
            if name.ends_with("-Win64-Shipping.exe") || name.ends_with("-WinGDK-Shipping.exe") {
                return Ok(dir);
            }
            let len = exe.metadata().map(|m| m.len()).unwrap_or(0);
            if biggest.as_ref().is_none_or(|(l, _)| len > *l) {
                biggest = Some((len, dir.clone()));
            }
        }
        if depth < 5 {
            stack.extend(fs::read_dir(&dir).into_iter().flatten().flatten()
                .filter(|e| e.path().is_dir()).map(|e| (e.path(), depth + 1)));
        }
    }
    biggest.map(|(_, d)| d).ok_or_else(|| format!("Nenhum .exe de jogo encontrado em {}", p.display()))
}

pub fn can_write(dir: &Path) -> bool {
    let t = dir.join(".gpu-mods-write-test");
    let ok = fs::write(&t, b"x").is_ok();
    let _ = fs::remove_file(t);
    ok
}

/// Everything we put in the game folder, so uninstall removes only that.
pub struct Game {
    pub dir: PathBuf,
    ours: BTreeSet<String>,
}

impl Game {
    pub fn open(dir: &Path) -> Game {
        let ours = fs::read_to_string(dir.join(MANIFEST)).unwrap_or_default()
            .lines().filter(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()).collect();
        Game { dir: dir.to_path_buf(), ours }
    }

    fn is_ours(&self, name: &str) -> bool {
        self.ours.iter().any(|o| o.eq_ignore_ascii_case(name))
    }

    pub fn save(&self) -> Result<(), String> {
        let list: Vec<&str> = self.ours.iter().map(String::as_str).collect();
        fs::write(self.dir.join(MANIFEST), list.join("\r\n")).map_err(|e| e.to_string())
    }

    /// Copies `src` as `name`. A game file that would be overwritten is kept as `name.gpumods-backup`.
    /// Folders the game already had are merged and never deleted on uninstall.
    pub fn place(&mut self, src: &Path, name: &str, log: Log) -> Result<(), String> {
        let dst = self.dir.join(name);
        if src.is_dir() {
            let preexisting = dst.exists() && !self.is_ours(name);
            copy_dir(src, &dst)?;
            if preexisting {
                log(format!("A pasta {name} já existia no jogo: arquivos mesclados"));
            } else {
                self.ours.insert(name.to_string());
            }
            return Ok(());
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let backup = self.dir.join(format!("{name}{BACKUP}"));
        if dst.exists() && !self.is_ours(name) && !backup.exists() {
            fs::rename(&dst, &backup).map_err(|e| format!("Backup de {name}: {e}"))?;
            log(format!("{name} original do jogo guardado como {name}{BACKUP}"));
        }
        fs::copy(src, &dst).map_err(|e| format!("Copiar {name}: {e} (feche o jogo)"))?;
        self.ours.insert(name.to_string());
        Ok(())
    }

    pub fn write_file(&mut self, name: &str, text: &str) -> Result<(), String> {
        let path = self.dir.join(name);
        if !path.exists() {
            self.ours.insert(name.to_string());
        }
        fs::write(&path, text).map_err(|e| format!("{name}: {e}"))
    }

    pub fn set_ini(&mut self, file: &str, section: &str, key: &str, value: &str) -> Result<(), String> {
        let path = self.dir.join(file);
        if !path.exists() {
            self.ours.insert(file.to_string());
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        fs::write(&path, set_ini_value(&text, section, key, value)).map_err(|e| format!("{file}: {e}"))
    }

    pub fn uninstall(mut self, log: Log) -> Result<(), String> {
        for name in std::mem::take(&mut self.ours) {
            let p = self.dir.join(&name);
            let r = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
            if r.is_ok() {
                log(format!("Removido {name}"));
            }
        }
        for e in fs::read_dir(&self.dir).map_err(|e| e.to_string())?.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(orig) = n.strip_suffix(BACKUP) {
                fs::rename(e.path(), self.dir.join(orig)).map_err(|e| format!("Restaurar {orig}: {e}"))?;
                log(format!("Restaurado {orig}"));
            }
        }
        let _ = fs::remove_file(self.dir.join(MANIFEST));
        Ok(())
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for e in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            fs::copy(e.path(), &to).map_err(|err| format!("{}: {err}", to.display()))?;
        }
    }
    Ok(())
}

/// Sets `key=value` in `[section]`, replacing the existing line (commented or not) or adding it.
pub fn set_ini_value(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let (mut cur, mut sec_idx, mut key_idx) = (String::new(), None, None);
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with('[') && t.contains(']') {
            cur = t[1..t.find(']').unwrap()].trim().to_string();
            if cur.eq_ignore_ascii_case(section) {
                sec_idx = Some(i);
            }
            continue;
        }
        let k = t.trim_start_matches([';', '#']).split('=').next().unwrap_or("").trim();
        if cur.eq_ignore_ascii_case(section) && t.contains('=') && k.eq_ignore_ascii_case(key) {
            key_idx = Some(i);
            break;
        }
    }
    let entry = format!("{key}={value}");
    match (key_idx, sec_idx) {
        (Some(i), _) => lines[i] = entry,
        (None, Some(i)) => lines.insert(i + 1, entry),
        (None, None) => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(entry);
        }
    }
    lines.join("\r\n") + "\r\n"
}

fn original_filename(path: &Path) -> Option<String> {
    // Cheap check without the version API: OptiScaler's binary embeds its own name.
    let bytes = fs::read(path).ok()?;
    let needle: Vec<u8> = "OptiScaler.dll".encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes.windows(needle.len()).any(|w| w == needle).then(|| "OptiScaler.dll".to_string())
}

/// Runs the whole plan on one game folder.
pub fn apply(gpu: &Gpu, dlss5: bool, mult: u8, proxy: Option<&str>, game_dir: &Path, log: Log) -> Result<(), String> {
    let plan = plan::build(gpu.tier, dlss5, mult);
    let chosen = valid_proxy(proxy)?;
    let mut state = State { gpu: gpu.tier.label().into(), dlss5, mult, proxy: chosen.map(str::to_string), ..State::default() };

    if plan.neuralhub {
        return neuralhub(&plan.notes, log);
    }

    // Reapplying starts clean (another choice may need different files), but keeps the user's OptiScaler.ini.
    let previous = Game::open(game_dir);
    let saved_ini = previous.is_ours("OptiScaler.ini").then(|| fs::read(game_dir.join("OptiScaler.ini")).ok()).flatten();
    if !previous.ours.is_empty() {
        log("Removendo a instalação anterior antes de aplicar".into());
        previous.uninstall(log)?;
    }
    let mut game = Game::open(game_dir);
    log(format!("Jogo: {}", game.dir.display()));

    if let Some(opti) = plan.opti {
        let rel = match opti {
            Opti::Unlocked => net::latest_release("ShyVortex/dlss-unlocked", |n| n.contains("standalone") && n.ends_with(".zip"), log)?,
            Opti::Official => net::latest_release("optiscaler/OptiScaler", |n| n.ends_with(".7z") || n.ends_with(".zip"), log)?,
            Opti::Nightly => net::latest_release("optiscaler/OptiScaler-nightly", |n| n.ends_with(".7z") || n.ends_with(".zip"), log)?,
        };
        // MFG Unlocker's OptiScaler build (same files, patched OptiScaler.dll) until its changes are upstream.
        let dll_override = match opti {
            Opti::Nightly => Some(net::latest_release("dashdogy/OptiScaler", |n| n == "OptiScaler.dll", log)?),
            _ => None,
        };
        let root = net::extract(&rel.file)?;
        let ini = net::find_file(&root, "OptiScaler.ini").ok_or("OptiScaler.ini não está no pacote baixado")?;
        let src = ini.parent().unwrap().to_path_buf();
        let dll = if src.join("OptiScaler.dll").exists() { "OptiScaler.dll" } else { "dxgi.dll" };

        // Keep the proxy name of a previous OptiScaler install, else dxgi.dll.
        let old: Vec<&str> = PROXIES.iter().copied()
            .filter(|p| original_filename(&game.dir.join(p)).is_some()).collect();
        let proxy = chosen.or(old.first().copied()).unwrap_or("dxgi.dll");
        for o in old.iter().filter(|o| **o != proxy) {
            let _ = fs::remove_file(game.dir.join(o));
            log(format!("Removido OptiScaler antigo: {o}"));
        }

        for e in fs::read_dir(&src).map_err(|e| e.to_string())?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if SKIP_PACKAGE.contains(&name.as_str()) || name.starts_with("!!") {
                continue;
            }
            if name == dll {
                let src = dll_override.as_ref().map_or(e.path(), |o| o.file.clone());
                game.place(&src, proxy, log)?;
            } else {
                game.place(&e.path(), &name, log)?;
            }
        }
        if let Some(ini) = &saved_ini {
            fs::write(game.dir.join("OptiScaler.ini"), ini).map_err(|e| e.to_string())?;
            log("Mantido o seu OptiScaler.ini anterior".into());
        }
        log(format!("OptiScaler {} instalado como {proxy}", rel.tag));
        let label = match opti {
            Opti::Official => "OptiScaler",
            Opti::Unlocked => "DLSS Unlocked",
            Opti::Nightly => "OptiScaler nightly",
        };
        state.versions.insert(label.into(), rel.tag.clone());

        if gpu.tier.is_nvidia() {
            // The fork's RTX 20/30 frame-gen sideload breaks native DLSS-G on RTX 40/50.
            if gpu.tier != Tier::Rtx2030 {
                let sm86 = game.dir.join("OptiScaler").join("dlssg_sm86");
                if sm86.exists() && fs::remove_dir_all(&sm86).is_ok() {
                    log("Removido OptiScaler/dlssg_sm86 (só serve para RTX 20/30)".into());
                }
            }
            copy_dlss_dlls(&mut game, log)?;
        }
    }

    if plan.reshade {
        let rs = net::reshade(log)?;
        let dll = net::find_file(&net::extract(&rs.file)?, "ReShade64.dll").ok_or("ReShade64.dll não está no instalador do ReShade")?;
        // With OptiScaler, it loads ReShade64.dll itself; without it, ReShade is the dxgi.dll proxy.
        let name = if plan.opti.is_some() { "ReShade64.dll" } else { "dxgi.dll" };
        game.place(&dll, name, log)?;
        log(format!("ReShade {} instalado como {name}", rs.tag));
        state.versions.insert("ReShade".into(), rs.tag.clone());
    }

    if plan.mfg_addon {
        let rel = net::latest_release("mavismmg/MFGAdaUnlock-RenoDx", |n| n.ends_with(".addon64") || n.ends_with(".zip"), log)?;
        let addon = if rel.file.extension().is_some_and(|e| e == "addon64") {
            rel.file.clone()
        } else {
            let dir = net::extract(&rel.file)?;
            fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "addon64")).ok_or("Arquivo .addon64 não encontrado")?
        };
        game.place(&addon, "renodx-mfgunlock.addon64", log)?;
        log(format!("MFG Unlock {} instalado", rel.tag));
        state.versions.insert("RenoDX MFG Unlock".into(), rel.tag.clone());
    }

    if plan.mfg_unlocker {
        let rel = net::latest_release("dashdogy/MFG-Unlocker", |n| n.starts_with("MFGUnlocker") && n.ends_with(".zip"), log)?;
        let dll = net::find_file(&net::extract(&rel.file)?, "MFGUnlocker.dll").ok_or("MFGUnlocker.dll não está no pacote")?;
        // Inside OptiScaler it must be a plugin; on its own it is the dxgi.dll proxy.
        let name = if plan.opti.is_some() { "OptiScaler/plugins/MFGUnlocker.asi" } else { chosen.unwrap_or("dxgi.dll") };
        game.place(&dll, name, log)?;
        log(format!("MFG Unlocker {} instalado como {name}", rel.tag));
        state.versions.insert("MFG Unlocker".into(), rel.tag.clone());
    }
    for (name, text) in &plan.files {
        game.write_file(name, text)?;
        log(format!("{name}: {}", text.trim()));
    }

    for e in &plan.ini {
        game.set_ini(e.file, e.section, e.key, &e.value)?;
        log(format!("{}: [{}] {}={}", e.file, e.section, e.key, e.value));
    }
    state.when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    game.write_file(STATE, &serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?)?;
    game.save()?;
    log("Pronto!".into());
    for n in &plan.notes {
        log(format!("• {n}"));
    }
    log("• Não use em jogos online com anti-cheat (risco de banimento).".into());
    Ok(())
}

/// OptiScaler only finds nvngx_dlss*.dll next to the exe; Unreal keeps them in Engine/Plugins.
/// Looks inside the game's own install folder only (stops at library folders like steamapps/common).
fn copy_dlss_dlls(game: &mut Game, log: Log) -> Result<(), String> {
    let mut root = game.dir.clone();
    for _ in 0..4 {
        let Some(parent) = root.parent() else { break };
        let leaf = parent.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        if parent.parent().is_none()
            || ["common", "steamapps", "games", "game", "jogos", "epic games", "xboxgames", "gog games", "gog galaxy"].contains(&leaf.as_str())
            || leaf.starts_with("program files")
        {
            break;
        }
        root = parent.to_path_buf();
    }
    for dll in ["nvngx_dlss.dll", "nvngx_dlssd.dll"] {
        if game.dir.join(dll).exists() {
            continue;
        }
        if let Some(found) = net::find_file(&root, dll).filter(|f| !f.starts_with(game.dir.join("OptiScaler"))) {
            game.place(&found, dll, log)?;
            log(format!("{dll} do jogo copiado para junto do exe"));
        }
    }
    Ok(())
}

const NR_DLLS: [&str; 4] = [
    "OptiScaler/streamline/nvngx_dlssnr.dll",
    "OptiScaler/streamline/nvngx_dlss.dll",
    "OptiScaler/streamline/nvngx_dlssd.dll",
    "OptiScaler/streamline/nvngx_dlssg.dll",
];

/// AMD + DLSS 5: NeuralHub has its own loader and game list. We install/update it silently,
/// fetch the newest NVIDIA DLLs it needs (the official copies shipped in dlss-unlocked) and open both.
fn neuralhub(notes: &[String], log: Log) -> Result<(), String> {
    let dlls = net::zip_files("ShyVortex/dlss-unlocked", |n| n.contains("standalone") && n.ends_with(".zip"), &NR_DLLS, log)?;
    log(format!("DLLs do DLSS ({}) em {}", dlls.tag, dlls.file.display()));

    let setup = net::latest_release("danielblnc/DLSS-NR-on-AMD", |n| n.to_lowercase().ends_with(".exe"), log)?;
    let marker = net::cache_dir().join("neuralhub-installed.txt");
    if neuralhub_exe().is_none() || fs::read_to_string(&marker).ok().as_deref() != Some(setup.tag.as_str()) {
        log(format!("Instalando NeuralHub {}...", setup.tag));
        // Tauri's NSIS installer: /S = silent, per-user (no admin).
        let ok = std::process::Command::new(&setup.file).arg("/S").status().is_ok_and(|s| s.success());
        if ok {
            let _ = fs::write(&marker, &setup.tag);
        } else {
            log("A instalação silenciosa falhou: abrindo o instalador normal.".into());
            std::process::Command::new(&setup.file).status().map_err(|e| format!("Instalador do NeuralHub: {e}"))?;
        }
    }
    let exe = neuralhub_exe().ok_or("NeuralHub não encontrado depois da instalação")?;
    std::process::Command::new(&exe).spawn().map_err(|e| format!("Abrir NeuralHub: {e}"))?;
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", dlls.file.join("nvngx_dlssnr.dll").display())).spawn();
    log(format!("NeuralHub aberto ({})", exe.display()));
    for n in notes {
        log(format!("• {n}"));
    }
    Ok(())
}

fn neuralhub_exe() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    [env("LOCALAPPDATA").map(|d| d.join("NeuralHub")), env("LOCALAPPDATA").map(|d| d.join("Programs/NeuralHub")), env("ProgramFiles").map(|d| d.join("NeuralHub"))]
        .into_iter().flatten().map(|d| d.join("neuralhub.exe")).find(|p| p.exists())
}

/// Redoes every modded game with its saved choices and the newest versions. One failure doesn't stop the rest.
pub fn update_all(gpu: &Gpu, dirs: &[PathBuf], log: Log) -> Result<(), String> {
    let mut failed = Vec::new();
    for dir in dirs {
        let Some(st) = read_state(dir) else { continue };
        log(format!("━━ {}", dir.display()));
        if let Err(e) = apply(gpu, st.dlss5, st.mult, st.proxy.as_deref(), dir, log) {
            log(format!("ERRO: {e}"));
            failed.push(dir.display().to_string());
        }
    }
    if failed.is_empty() { Ok(()) } else { Err(format!("{} jogo(s) falharam: {}", failed.len(), failed.join(", "))) }
}

pub fn uninstall(game_dir: &Path, log: Log) -> Result<(), String> {
    Game::open(game_dir).uninstall(log)?;
    log("Mods removidos e arquivos originais restaurados.".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gpumods-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn ini_replace_commented_add_and_new_section() {
        let t = "[Upscalers]\r\n; comment\r\nDx12Upscaler=auto\r\n[Plugins]\r\n;LoadReshade=auto\r\n";
        let t = set_ini_value(t, "Upscalers", "Dx12Upscaler", "dlss");
        let t = set_ini_value(&t, "Plugins", "LoadReshade", "true");
        let t = set_ini_value(&t, "Plugins", "Path", "auto");
        let t = set_ini_value(&t, "XeFG", "InterpolationCount", "3");
        assert_eq!(t, "[Upscalers]\r\n; comment\r\nDx12Upscaler=dlss\r\n[Plugins]\r\nPath=auto\r\nLoadReshade=true\r\n\r\n[XeFG]\r\nInterpolationCount=3\r\n");
        // Same key name in another section is left alone.
        let t = set_ini_value("[A]\nEnabled=false\n[B]\nEnabled=false\n", "B", "Enabled", "true");
        assert_eq!(t, "[A]\r\nEnabled=false\r\n[B]\r\nEnabled=true\r\n");
    }

    #[test]
    fn place_backs_up_and_uninstall_restores() {
        let game = tmp("game");
        let pkg = tmp("pkg");
        fs::write(game.join("dxgi.dll"), "game-original").unwrap();
        fs::create_dir_all(game.join("Licenses")).unwrap();
        fs::write(game.join("Licenses/game.txt"), "keep").unwrap();
        fs::write(pkg.join("OptiScaler.dll"), "opti").unwrap();
        fs::create_dir_all(pkg.join("Licenses")).unwrap();
        fs::write(pkg.join("Licenses/opti.txt"), "x").unwrap();
        fs::create_dir_all(pkg.join("D3D12_Optiscaler")).unwrap();
        fs::write(pkg.join("D3D12_Optiscaler/D3D12Core.dll"), "x").unwrap();

        let log = |_: String| {};
        let mut g = Game::open(&game);
        g.place(&pkg.join("OptiScaler.dll"), "dxgi.dll", &log).unwrap();
        g.place(&pkg.join("Licenses"), "Licenses", &log).unwrap();
        g.place(&pkg.join("D3D12_Optiscaler"), "D3D12_Optiscaler", &log).unwrap();
        g.set_ini("ReShade.ini", "RenoDX.MFGUnlock", "ForceMultiplier", "4").unwrap();
        g.save().unwrap();
        assert_eq!(fs::read_to_string(game.join("dxgi.dll")).unwrap(), "opti");

        // Reinstall must not back up our own dll over the game's original.
        let mut g = Game::open(&game);
        g.place(&pkg.join("OptiScaler.dll"), "dxgi.dll", &log).unwrap();
        g.save().unwrap();

        g.write_file(STATE, r#"{"gpu":"NVIDIA RTX 40","dlss5":true,"mult":4,"versions":{"OptiScaler":"v1"},"when":1}"#).unwrap();
        g.save().unwrap();
        assert_eq!(read_state(&game).unwrap().mult, 4);
        assert!(game.join(MANIFEST).exists());

        uninstall(&game, &log).unwrap();
        assert!(read_state(&game).is_none());
        assert_eq!(fs::read_to_string(game.join("dxgi.dll")).unwrap(), "game-original");
        assert!(game.join("Licenses/game.txt").exists(), "pre-existing folder kept");
        assert!(!game.join("D3D12_Optiscaler").exists());
        assert!(!game.join("ReShade.ini").exists());
        assert!(!game.join(MANIFEST).exists());
    }

    #[test]
    fn resolves_unreal_and_bin_folders() {
        let root = tmp("resolve");
        let bin = root.join("MyGame/Binaries/Win64");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("MyGame-Win64-Shipping.exe"), "x").unwrap();
        fs::write(root.join("UnityCrashHandler64.exe"), "x").unwrap();
        assert_eq!(resolve_game_dir(&root).unwrap(), bin);
        assert_eq!(resolve_game_dir(&bin.join("MyGame-Win64-Shipping.exe")).unwrap(), bin);
    }

    #[test]
    fn nested_place_and_new_files_are_tracked_and_removed() {
        let game = tmp("nested");
        let src = game.join("src.bin");
        fs::write(&src, "mfg").unwrap();
        fs::write(game.join("ReShade.ini"), "[GENERAL]\r\n").unwrap();
        let log = |_: String| {};
        let mut g = Game::open(&game);
        g.place(&src, "OptiScaler/plugins/MFGUnlocker.asi", &log).unwrap();
        g.write_file("RTXMFG-Universal.json", "{}").unwrap();
        g.set_ini("OptiScaler.ini", "Plugins", "LoadAsiPlugins", "true").unwrap();
        g.set_ini("ReShade.ini", "RenoDX.MFGUnlock", "ForceMultiplier", "4").unwrap();
        g.save().unwrap();
        assert!(game.join("OptiScaler/plugins/MFGUnlocker.asi").exists());
        assert!(fs::read_to_string(game.join("ReShade.ini")).unwrap().contains("ForceMultiplier=4"));

        uninstall(&game, &log).unwrap();
        assert!(!game.join("OptiScaler/plugins/MFGUnlocker.asi").exists());
        assert!(!game.join("RTXMFG-Universal.json").exists() && !game.join("OptiScaler.ini").exists());
        assert!(game.join("ReShade.ini").exists(), "the user's own ReShade.ini stays");
    }

    #[test]
    fn uninstall_tolerates_files_already_gone() {
        let game = tmp("gone");
        fs::write(game.join(MANIFEST), "dxgi.dll\r\nmissing.dll").unwrap();
        fs::write(game.join("dxgi.dll"), "x").unwrap();
        uninstall(&game, &|_| {}).unwrap();
        assert!(!game.join("dxgi.dll").exists() && !game.join(MANIFEST).exists());
    }

    #[test]
    fn detects_optiscaler_by_embedded_name() {
        let d = tmp("orig");
        let mut bytes = b"MZ....".to_vec();
        bytes.extend("OptiScaler.dll".encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(d.join("dxgi.dll"), bytes).unwrap();
        fs::write(d.join("winmm.dll"), "MZ other").unwrap();
        assert_eq!(original_filename(&d.join("dxgi.dll")).as_deref(), Some("OptiScaler.dll"));
        assert_eq!(original_filename(&d.join("winmm.dll")), None);
        assert_eq!(original_filename(&d.join("nope.dll")), None);
    }

    #[test]
    fn dlss_dlls_come_from_this_game_only() {
        let lib = tmp("lib").join("steamapps/common");
        let exe_dir = lib.join("MyGame/MyGame/Binaries/Win64");
        let plugin = lib.join("MyGame/Engine/Plugins/DLSS/Binaries/ThirdParty/Win64");
        let other = lib.join("OtherGame");
        for d in [&exe_dir, &plugin, &other] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(plugin.join("nvngx_dlss.dll"), "mine").unwrap();
        fs::write(other.join("nvngx_dlssd.dll"), "not mine").unwrap();
        let mut g = Game::open(&exe_dir);
        copy_dlss_dlls(&mut g, &|_| {}).unwrap();
        assert_eq!(fs::read_to_string(exe_dir.join("nvngx_dlss.dll")).unwrap(), "mine");
        assert!(!exe_dir.join("nvngx_dlssd.dll").exists(), "must not cross into another game");
    }

    #[test]
    fn state_reading_and_update_all_skip_unmodded() {
        let d = tmp("state");
        assert!(read_state(&d).is_none());
        fs::write(d.join(STATE), "not json").unwrap();
        assert!(read_state(&d).is_none());
        let plain = tmp("plain");
        let gpu = Gpu { name: "x".into(), tier: Tier::Rtx40 };
        let logs = std::cell::RefCell::new(Vec::new());
        update_all(&gpu, &[plain], &|s| logs.borrow_mut().push(s)).unwrap();
        assert!(logs.borrow().is_empty(), "nothing to update, no network");
    }

    #[test]
    fn resolve_errors_and_write_check() {
        let d = tmp("empty");
        assert!(resolve_game_dir(&d).unwrap_err().contains("Nenhum .exe"));
        assert!(resolve_game_dir(&d.join("nope")).unwrap_err().contains("não encontrada"));
        fs::write(d.join("unins000.exe"), "x").unwrap();
        assert!(resolve_game_dir(&d).is_err(), "uninstallers are not games");
        assert!(can_write(&d));
        assert!(!d.join(".gpu-mods-write-test").exists());
    }

    /// End to end with the real packages: `cargo test -- --ignored`. Applies to a fake game and removes it.
    #[test]
    #[ignore]
    fn real_apply_and_uninstall() {
        let log = |s: String| println!("{s}");
        for (tier, dlss5, mult) in [(Tier::Rtx40, false, 3), (Tier::Rdna3, false, 6), (Tier::NvidiaOld, false, 2), (Tier::Intel, false, 4)] {
            let game = tmp(&format!("e2e-{tier:?}"));
            fs::write(game.join("Game.exe"), "x").unwrap();
            fs::write(game.join("dxgi.dll"), "original").unwrap();
            let gpu = Gpu { name: "test".into(), tier };
            apply(&gpu, dlss5, mult, None, &game, &log).unwrap();
            let st = read_state(&game).unwrap();
            assert_eq!((st.mult, st.dlss5), (mult, dlss5));
            assert!(fs::metadata(game.join("dxgi.dll")).unwrap().len() > 1_000_000, "{tier:?}: proxy installed");
            assert_eq!(fs::read_to_string(game.join("dxgi.dll.gpumods-backup")).unwrap(), "original");
            let ini = fs::read_to_string(game.join("OptiScaler.ini")).unwrap_or_default();
            match tier {
                Tier::Rtx40 => assert!(fs::read_to_string(game.join("RTXMFG-Universal.json")).unwrap().contains("\"multiplier\": 3")),
                Tier::Rdna3 => {
                    assert!(game.join("OptiScaler/plugins/MFGUnlocker.asi").exists());
                    assert!(ini.contains("FGOutput=xefg") && ini.contains("InterpolationCount=5") && ini.contains("LoadAsiPlugins=true"));
                }
                Tier::NvidiaOld => assert!(ini.contains("FGOutput=fsrfg")),
                _ => assert!(ini.contains("Dx12Upscaler=xess") && ini.contains("InterpolationCount=3")),
            }
            // Reapplying must not stack backups or lose the original.
            apply(&gpu, dlss5, mult, None, &game, &log).unwrap();
            uninstall(&game, &log).unwrap();
            let left: Vec<String> = fs::read_dir(&game).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            assert_eq!(fs::read_to_string(game.join("dxgi.dll")).unwrap(), "original");
            let mut left = left;
            left.sort();
            assert_eq!(left, ["Game.exe", "dxgi.dll"], "{tier:?}: everything removed");
        }
    }

    #[test]
    fn proxy_names_outside_the_allow_list_are_refused() {
        let game = tmp("badproxy");
        let gpu = Gpu { name: "x".into(), tier: Tier::Rtx40 };
        for bad in ["../../evil.dll", "kernel32.dll", "dxgi.dll\\..\\x.dll"] {
            let err = apply(&gpu, false, 3, Some(bad), &game, &|_| {}).unwrap_err();
            assert!(err.contains("não permitido"), "{bad}: {err}");
        }
        assert!(fs::read_dir(&game).unwrap().next().is_none(), "nothing was written");
        assert_eq!(valid_proxy(Some("version.dll")), Ok(Some("version.dll")));
        assert_eq!(valid_proxy(None), Ok(None));
    }

    /// Chosen proxy name, end to end (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn real_apply_with_chosen_proxy() {
        let log = |s: String| println!("{s}");
        for tier in [Tier::Rtx40, Tier::Intel] {
            let game = tmp(&format!("e2e-proxy-{tier:?}"));
            fs::write(game.join("Game.exe"), "x").unwrap();
            let gpu = Gpu { name: "test".into(), tier };
            apply(&gpu, false, 4, Some("version.dll"), &game, &log).unwrap();
            assert!(fs::metadata(game.join("version.dll")).unwrap().len() > 1_000_000, "{tier:?}");
            assert!(!game.join("dxgi.dll").exists(), "{tier:?}");
            assert_eq!(read_state(&game).unwrap().proxy.as_deref(), Some("version.dll"));
            update_all(&gpu, std::slice::from_ref(&game), &log).unwrap();
            assert!(game.join("version.dll").exists(), "update keeps the chosen proxy");
            uninstall(&game, &log).unwrap();
            assert!(!game.join("version.dll").exists());
        }
    }
}
