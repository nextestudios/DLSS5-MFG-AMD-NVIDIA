//! `DLSS5-FrameGen.exe --smoke-test <report.txt>`: checks the real exe on a real Windows (CI) without
//! a window: GPU detection, store scan, profiles, every plan, and a full apply/remove on a fake game.
use std::fs;
use std::path::Path;

use crate::gpu::{self, Gpu, Tier};
use crate::{games, install, plan, profiles};

pub fn run(report: &Path) -> bool {
    let mut out = String::new();
    let mut ok = true;
    let mut check = |name: &str, r: Result<String, String>| {
        match &r {
            Ok(m) => out.push_str(&format!("OK    {name}: {m}\n")),
            Err(e) => {
                ok = false;
                out.push_str(&format!("FALHA {name}: {e}\n"));
            }
        }
    };
    check("versão", Ok(env!("CARGO_PKG_VERSION").into()));
    check("placa", Ok(gpu::detect().map_or("nenhuma placa dedicada (normal em VM)".into(), |g| format!("{} → {}", g.name, g.tier.label()))));
    check("lojas", Ok(format!("{} jogos encontrados", games::scan().len())));
    let p = profiles::load();
    check("perfis", if p.is_empty() { Err("nenhum perfil carregado".into()) } else { Ok(format!("{} perfis", p.len())) });
    let plans = Tier::ALL.iter().flat_map(|t| [0u8, 2, 3, 4, 6].map(|m| plan::build(*t, true, m))).count();
    check("planos", Ok(format!("{plans} combinações")));
    check("aplicar e remover", apply_and_remove());
    let _ = fs::write(report, &out);
    ok
}

fn apply_and_remove() -> Result<String, String> {
    let game = std::env::temp_dir().join(format!("gpumods-smoke-{}", std::process::id()));
    let _ = fs::remove_dir_all(&game);
    fs::create_dir_all(&game).map_err(|e| e.to_string())?;
    fs::write(game.join("Game.exe"), "x").map_err(|e| e.to_string())?;
    fs::write(game.join("dxgi.dll"), "original").map_err(|e| e.to_string())?;
    let gpu = Gpu { name: "smoke".into(), tier: Tier::Rtx40 };
    let log = |_: String| {};
    install::apply(&gpu, false, 3, None, &game, &log)?;
    let size = fs::metadata(game.join("dxgi.dll")).map_err(|e| e.to_string())?.len();
    if size < 1_000_000 {
        return Err(format!("dxgi.dll instalado tem só {size} bytes"));
    }
    install::uninstall(&game, &log)?;
    let back = fs::read_to_string(game.join("dxgi.dll")).map_err(|e| e.to_string())?;
    if back != "original" {
        return Err("o dxgi.dll original não voltou".into());
    }
    let _ = fs::remove_dir_all(&game);
    Ok(format!("MFG Unlocker instalado ({size} bytes) e removido, original restaurado"))
}
