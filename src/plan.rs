//! What gets installed for each GPU. Pure logic, no I/O.
use crate::gpu::Tier;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opti {
    /// optiscaler/OptiScaler
    Official,
    /// ShyVortex/dlss-unlocked: OptiScaler fork with DLSS 5 (NR) and the RTX 20/30 DLSS-G unlocker.
    Unlocked,
    /// optiscaler/OptiScaler-nightly (v10) with dashdogy's OptiScaler.dll: the build MFG Unlocker runs in as a plugin.
    Nightly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniEdit {
    pub file: &'static str,
    pub section: &'static str,
    pub key: &'static str,
    pub value: String,
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub opti: Option<Opti>,
    pub reshade: bool,
    pub mfg_addon: bool,
    /// dashdogy/MFG-Unlocker: OptiScaler plugin when there is an OptiScaler, else its own dxgi.dll proxy.
    pub mfg_unlocker: bool,
    /// Text files written as-is (MFG Unlocker's settings).
    pub files: Vec<(&'static str, String)>,
    /// DLSS-NR on AMD: separate app (NeuralHub) with its own loader; nothing else goes in the game.
    pub neuralhub: bool,
    pub ini: Vec<IniEdit>,
    pub notes: Vec<String>,
}

pub const OFF: u8 = 0;

/// Frame generation multipliers this GPU can reach (0 = off).
pub fn multipliers(tier: Tier) -> &'static [u8] {
    if tier == Tier::NvidiaOld { &[0, 2] } else { &[0, 2, 3, 4, 6] }
}

pub fn dlss5_supported(tier: Tier) -> bool {
    tier.is_rtx() || matches!(tier, Tier::Rdna4 | Tier::Rdna3 | Tier::Rdna2)
}

impl Plan {
    fn set(&mut self, file: &'static str, section: &'static str, key: &'static str, value: impl ToString) {
        self.ini.push(IniEdit { file, section, key, value: value.to_string() });
    }

    fn opti(&mut self, section: &'static str, key: &'static str, value: impl ToString) {
        self.set("OptiScaler.ini", section, key, value);
    }

    fn note(&mut self, n: &str) {
        self.notes.push(n.to_string());
    }

    /// MFG Unlock (RenoDX) as a ReShade add-on. `xemfg` picks Intel XeSS-FG (2x-6x on RTX 20-50)
    /// instead of the unlocked DLSS-G backend (RTX 40 only).
    fn mfg_unlock(&mut self, mult: u8, xemfg: bool) {
        self.reshade = true;
        self.mfg_addon = true;
        if xemfg {
            self.set("MFGAdaUnlock.ini", "Backend", "Selected", "XeMFG");
            self.set("MFGAdaUnlock.ini", "XeFG", "Multiplier", mult);
            self.note("1ª vez no jogo: Home > MFG Unlock > Diagnostics > Intel runtime files > Install / update files, e reinicie o jogo.");
            self.note("Use o jogo em Janela sem bordas (XeMFG não funciona em tela cheia exclusiva).");
        } else {
            self.set("ReShade.ini", "RenoDX.MFGUnlock", "ForceMultiplier", mult);
        }
        self.note("O jogo precisa ter Frame Generation (DLSS-G, FSR 3.1 ou XeSS-FG): ligue-o no menu do jogo.");
    }

    /// MFG Unlocker on its own: DLSS MFG at a fixed multiplier (RTX 20-50, needs the game's DLSS-G).
    fn mfg_unlocker_dlssg(&mut self, mult: u8) {
        self.mfg_unlocker = true;
        self.files.push(("RTXMFG-Universal.json", format!("{{\"followGame\": false, \"mode\": \"fixed\", \"multiplier\": {mult}}}\r\n")));
        self.note("Ligue o Frame Generation (DLSS-G) no menu do jogo. Backspace abre o menu do MFG Unlocker.");
    }
}

pub fn build(tier: Tier, dlss5: bool, mult: u8) -> Plan {
    let mut p = Plan::default();
    let dlss5 = dlss5 && dlss5_supported(tier);
    let mult = if multipliers(tier).contains(&mult) { mult } else { OFF };

    if dlss5 && !tier.is_nvidia() {
        p.neuralhub = true;
        p.note("O app instala o NeuralHub e baixa o nvngx_dlssnr.dll (e dlss, dlssd, dlssg) mais recentes; a pasta com eles abre sozinha.");
        p.note("No NeuralHub: importe os DLLs dessa pasta (uma vez só), deixe ele achar os jogos e instale o mod loader no jogo.");
        p.note("No jogo: ative DLSS, FSR ou XeSS e aperte End. O Frame Generation (DLSS-FG até 4x) é configurado no próprio NeuralHub.");
        return p;
    }

    if tier.is_nvidia() {
        if dlss5 {
            p.opti = Some(Opti::Unlocked);
            p.opti("Upscalers", "Dx12Upscaler", "dlss");
            // Only one MFG unlocker at a time: the RenoDX add-on handles it on RTX 40.
            p.opti("DLSSG", "AdaMfgUnlock", "false");
        } else if tier == Tier::NvidiaOld {
            p.opti = Some(Opti::Official);
            p.opti("Upscalers", "Dx12Upscaler", "fsr31");
        }
        if dlss5 {
            // Validated with the MFG-DLSS5 script: Pre-SR, keep the game's colours.
            p.opti("DlssNr", "Enabled", "true");
            p.opti("DlssNr", "RunBeforeSR", "true");
            p.opti("DlssNr", "FinishedPicture", "false");
            p.opti("DlssNr", "ColourStrength", "0.000000");
            p.note("DLSS 5 ligado. Para desligar no jogo: Insert > DLSS Neural Rendering.");
        }
        match (tier, mult) {
            (_, OFF) => {}
            (Tier::NvidiaOld, _) => fsr_fg(&mut p),
            (Tier::Rtx50, m) if m <= 4 => p.note("RTX 50 já tem MFG nativo até 4x: escolha o multiplicador no menu do jogo."),
            // With DLSS 5 the fork owns the proxy: MFG goes through ReShade + RenoDX (the fork's OptiScaler 0.9 can't host MFG Unlocker).
            (Tier::Rtx40, m) if dlss5 => p.mfg_unlock(m, false),
            (Tier::Rtx50, 6) if dlss5 => p.mfg_unlock(6, true),
            (Tier::Rtx2030, 6) if dlss5 => {
                p.note("6x em RTX 20/30 é experimental.");
                p.mfg_unlock(6, true);
            }
            (Tier::Rtx2030, _) if dlss5 => p.note("Escolha o Frame Generation (2x/3x/4x) no menu do jogo; o dlss-unlocked libera o DLSS-G na RTX 20/30."),
            (t, m) => {
                if t == Tier::Rtx2030 {
                    p.note("RTX 30 é suportada pelo MFG Unlocker; RTX 20 é experimental.");
                }
                p.mfg_unlocker_dlssg(m);
            }
        }
    } else {
        // 3x+ (and Intel at any multiplier) runs XeSS-FG with MFG Unlocker inside OptiScaler 10.
        let xefg = mult != OFF && (mult > 2 || tier == Tier::Intel);
        p.opti = Some(if xefg { Opti::Nightly } else { Opti::Official });
        p.opti("Upscalers", "Dx12Upscaler", if tier == Tier::Intel { "xess" } else { "fsr31" });
        if tier == Tier::Rdna4 {
            p.note("FSR 4 é ativado automaticamente pelo OptiScaler na RX 9000.");
        }
        match mult {
            OFF => {}
            2 if tier != Tier::Intel => fsr_fg(&mut p),
            m => {
                p.mfg_unlocker = true;
                p.opti("Plugins", "LoadAsiPlugins", "true");
                p.opti("FrameGen", "Enabled", "true");
                p.opti("FrameGen", "FGInput", "upscaler");
                p.opti("FrameGen", "FGOutput", "xefg");
                p.opti("XeFG", "InterpolationCount", m - 1);
                p.note("Frame Generation pelo XeSS-FG com o MFG Unlocker (OptiScaler 10 nightly). O multiplicador também muda no menu Insert.");
                if tier != Tier::Intel || m == 6 {
                    p.note("XeSS-FG em AMD e 6x são experimentais: se der problema, use um multiplicador menor.");
                }
            }
        }
    }
    if p.opti.is_some() {
        p.note("No jogo: ative DLSS/FSR/XeSS. Insert abre o menu do OptiScaler.");
    }
    if p.reshade {
        if p.opti.is_some() {
            p.opti("Plugins", "LoadReshade", "true");
        }
        p.note("Home abre o menu do ReShade.");
    }
    p
}

impl Plan {
    fn ini_val(&self, section: &str, key: &str) -> Option<&str> {
        self.ini.iter().rev().find(|e| e.section == section && e.key == key).map(|e| e.value.as_str())
    }

    /// What happens to the picture, in order: [upscaler, DLSS 5, frame generation]. Shown in the UI.
    pub fn pipeline(&self, tier: Tier, mult: u8) -> [String; 3] {
        let up = if self.neuralhub {
            "DLSS / FSR (NeuralHub)"
        } else {
            match self.ini_val("Upscalers", "Dx12Upscaler") {
                Some("dlss") => "DLSS",
                Some("fsr31") if tier == Tier::Rdna4 => "FSR 4",
                Some("fsr31") => "FSR 3.1",
                Some("xess") => "XeSS",
                _ => "Upscaler do jogo",
            }
        };
        let nr = if self.neuralhub || self.ini_val("DlssNr", "Enabled") == Some("true") { "DLSS 5 Neural Rendering" } else { "Desligado" };
        let fg = match mult {
            OFF => "Desligado".to_string(),
            m if self.neuralhub => format!("DLSS-FG {m}x (NeuralHub)"),
            m if self.mfg_unlocker && self.opti.is_some() => format!("XeSS-FG {m}x + MFG Unlocker"),
            m if self.mfg_unlocker => format!("DLSS MFG {m}x (MFG Unlocker)"),
            m if self.mfg_addon => format!("MFG Unlock {m}x (RenoDX)"),
            m if self.ini_val("FrameGen", "FGOutput") == Some("fsrfg") => format!("FSR FG {m}x"),
            m => format!("DLSS-G {m}x do jogo"),
        };
        [up.to_string(), nr.to_string(), fg]
    }
}

fn fsr_fg(p: &mut Plan) {
    p.opti("FrameGen", "Enabled", "true");
    p.opti("FrameGen", "FGInput", "upscaler");
    p.opti("FrameGen", "FGOutput", "fsrfg");
    p.note("Frame Generation 2x via FSR FG. Se a interface tremer, ajuste o HUDfix no menu Insert.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(p: &Plan, section: &str, key: &str) -> Option<String> {
        p.ini.iter().rev().find(|e| e.section == section && e.key == key).map(|e| e.value.clone())
    }

    #[test]
    fn rtx40_dlss5_4x() {
        let p = build(Tier::Rtx40, true, 4);
        assert_eq!(p.opti, Some(Opti::Unlocked));
        assert!(p.reshade && p.mfg_addon);
        assert_eq!(val(&p, "RenoDX.MFGUnlock", "ForceMultiplier").as_deref(), Some("4"));
        assert_eq!(val(&p, "Plugins", "LoadReshade").as_deref(), Some("true"));
        assert_eq!(val(&p, "DlssNr", "Enabled").as_deref(), Some("true"));
    }

    #[test]
    fn mfg_only_uses_mfg_unlocker_alone() {
        for t in [Tier::Rtx40, Tier::Rtx2030] {
            let p = build(t, false, 3);
            assert_eq!(p.opti, None);
            assert!(p.mfg_unlocker && !p.reshade);
            assert!(p.files[0].1.contains("\"multiplier\": 3"));
        }
    }

    #[test]
    fn rtx50_native_up_to_4x_and_xemfg_for_6x() {
        let p = build(Tier::Rtx50, false, 4);
        assert!(p.opti.is_none() && !p.reshade);
        assert!(build(Tier::Rtx50, false, 6).mfg_unlocker);
        let p = build(Tier::Rtx50, true, 6);
        assert_eq!(val(&p, "Backend", "Selected").as_deref(), Some("XeMFG"));
        assert_eq!(val(&p, "XeFG", "Multiplier").as_deref(), Some("6"));
    }

    #[test]
    fn rtx30_dlss5_uses_unlocked() {
        assert_eq!(build(Tier::Rtx2030, true, 2).opti, Some(Opti::Unlocked));
        assert_eq!(build(Tier::Rtx2030, false, 0).opti, None);
    }

    #[test]
    fn amd_dlss5_is_neuralhub_only() {
        let p = build(Tier::Rdna3, true, 4);
        assert!(p.neuralhub && p.opti.is_none() && p.ini.is_empty());
    }

    #[test]
    fn amd_multipliers() {
        let p = build(Tier::Rdna4, false, 2);
        assert_eq!(val(&p, "FrameGen", "FGOutput").as_deref(), Some("fsrfg"));
        assert_eq!(p.opti, Some(Opti::Official));
        let p = build(Tier::Rdna2, false, 6);
        assert_eq!((p.opti, p.mfg_unlocker), (Some(Opti::Nightly), true));
        assert_eq!(val(&p, "FrameGen", "FGOutput").as_deref(), Some("xefg"));
        assert_eq!(val(&p, "XeFG", "InterpolationCount").as_deref(), Some("5"));
        assert_eq!(val(&p, "Plugins", "LoadAsiPlugins").as_deref(), Some("true"));
    }

    #[test]
    fn pipeline_names_each_stage() {
        assert_eq!(build(Tier::Rdna4, false, 6).pipeline(Tier::Rdna4, 6), ["FSR 4", "Desligado", "XeSS-FG 6x + MFG Unlocker"].map(String::from));
        assert_eq!(build(Tier::Rtx40, true, 4).pipeline(Tier::Rtx40, 4), ["DLSS", "DLSS 5 Neural Rendering", "MFG Unlock 4x (RenoDX)"].map(String::from));
        assert_eq!(build(Tier::Rtx50, false, 4).pipeline(Tier::Rtx50, 4)[2], "DLSS-G 4x do jogo");
    }

    #[test]
    fn intel_uses_xess_and_xefg() {
        let p = build(Tier::Intel, true, 2);
        assert!(!p.neuralhub);
        assert_eq!(val(&p, "Upscalers", "Dx12Upscaler").as_deref(), Some("xess"));
        assert_eq!(val(&p, "XeFG", "InterpolationCount").as_deref(), Some("1"));
    }

    /// Rules that must hold for every GPU x DLSS 5 x multiplier, including invalid multipliers.
    #[test]
    fn invariants_for_every_combination() {
        for tier in Tier::ALL {
            for dlss5 in [false, true] {
                for mult in [0u8, 1, 2, 3, 4, 5, 6, 8] {
                    let p = build(tier, dlss5, mult);
                    let ctx = format!("{tier:?} dlss5={dlss5} {mult}x");
                    let effective = if multipliers(tier).contains(&mult) { mult } else { OFF };

                    // Only one thing may be the game's dxgi.dll proxy.
                    let proxies = usize::from(p.opti.is_some())
                        + usize::from(p.reshade && p.opti.is_none())
                        + usize::from(p.mfg_unlocker && p.opti.is_none());
                    assert!(proxies <= 1, "{ctx}: {proxies} proxies");

                    if p.neuralhub {
                        assert!(p.opti.is_none() && p.ini.is_empty() && p.files.is_empty() && !p.reshade && !p.mfg_unlocker, "{ctx}");
                        assert!(!tier.is_nvidia() && dlss5, "{ctx}");
                    }
                    if effective == OFF {
                        assert!(!p.mfg_unlocker && !p.mfg_addon, "{ctx}: FG off but MFG installed");
                        assert_ne!(val(&p, "FrameGen", "Enabled").as_deref(), Some("true"), "{ctx}");
                    }
                    if !dlss5_supported(tier) {
                        assert!(!p.neuralhub && val(&p, "DlssNr", "Enabled").is_none(), "{ctx}");
                    }
                    if p.mfg_unlocker && p.opti.is_some() {
                        assert_eq!(p.opti, Some(Opti::Nightly), "{ctx}: MFG Unlocker plugin needs OptiScaler 10");
                        assert_eq!(val(&p, "Plugins", "LoadAsiPlugins").as_deref(), Some("true"), "{ctx}");
                    }
                    assert_eq!(val(&p, "Plugins", "LoadReshade").is_some(), p.reshade && p.opti.is_some(), "{ctx}");
                    assert_eq!(p.mfg_addon, p.reshade, "{ctx}: ReShade only comes for the RenoDX add-on");
                    if let Some(c) = val(&p, "XeFG", "InterpolationCount") {
                        assert_eq!(c.parse::<u8>().unwrap() + 1, effective, "{ctx}");
                    }
                    for (name, text) in &p.files {
                        let j: serde_json::Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{ctx}: {name}: {e}"));
                        assert_eq!(j["multiplier"], u64::from(effective), "{ctx}");
                    }
                    assert!(p.notes.iter().all(|n| !n.trim().is_empty()), "{ctx}");
                    let [up, nr, fg] = p.pipeline(tier, effective);
                    assert!(!up.is_empty() && !nr.is_empty(), "{ctx}");
                    assert_eq!(fg == "Desligado", effective == OFF, "{ctx}: {fg}");
                }
            }
        }
    }

    #[test]
    fn gtx_gets_fsr_fg_and_no_dlss5() {
        let p = build(Tier::NvidiaOld, true, 2);
        assert_eq!(p.opti, Some(Opti::Official));
        assert_eq!(val(&p, "Upscalers", "Dx12Upscaler").as_deref(), Some("fsr31"));
        assert_eq!(val(&p, "FrameGen", "FGOutput").as_deref(), Some("fsrfg"));
        assert!(val(&p, "DlssNr", "Enabled").is_none());
        assert_eq!(multipliers(Tier::NvidiaOld), &[0, 2]);
    }
}
