#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Rtx50,
    Rtx40,
    Rtx2030,
    NvidiaOld,
    Rdna4,
    Rdna3,
    Rdna2,
    AmdOld,
    Intel,
}

impl Tier {
    pub const ALL: [Tier; 9] = [
        Tier::Rtx50, Tier::Rtx40, Tier::Rtx2030, Tier::NvidiaOld,
        Tier::Rdna4, Tier::Rdna3, Tier::Rdna2, Tier::AmdOld, Tier::Intel,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tier::Rtx50 => "NVIDIA RTX 50",
            Tier::Rtx40 => "NVIDIA RTX 40",
            Tier::Rtx2030 => "NVIDIA RTX 20/30",
            Tier::NvidiaOld => "NVIDIA GTX",
            Tier::Rdna4 => "AMD RX 9000",
            Tier::Rdna3 => "AMD RX 7000",
            Tier::Rdna2 => "AMD RX 6000",
            Tier::AmdOld => "AMD (antiga)",
            Tier::Intel => "Intel Arc",
        }
    }

    pub fn is_rtx(self) -> bool {
        matches!(self, Tier::Rtx50 | Tier::Rtx40 | Tier::Rtx2030)
    }

    pub fn is_nvidia(self) -> bool {
        self.is_rtx() || self == Tier::NvidiaOld
    }
}

#[derive(Clone, Debug)]
pub struct Gpu {
    pub name: String,
    pub tier: Tier,
}

/// First number that follows `token` (e.g. "RTX 4070 Ti" -> 4070).
fn model_after(name: &str, token: &str) -> Option<u32> {
    let up = name.to_uppercase();
    let rest = &up[up.find(token)? + token.len()..];
    let digits: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub fn classify(name: &str, vendor_id: u32) -> Gpu {
    let up = name.to_uppercase();
    let tier = match vendor_id {
        // Workstation cards name the architecture: "RTX 2000 Ada", "RTX PRO 6000 Blackwell".
        0x10DE if up.contains("BLACKWELL") => Tier::Rtx50,
        0x10DE if up.contains(" ADA") => Tier::Rtx40,
        0x10DE => match model_after(name, "RTX") {
            Some(n) if (5000..6000).contains(&n) => Tier::Rtx50,
            Some(n) if (4000..5000).contains(&n) => Tier::Rtx40,
            Some(n) if (2000..4000).contains(&n) => Tier::Rtx2030,
            _ if up.contains("RTX A") => Tier::Rtx2030,
            _ => Tier::NvidiaOld,
        },
        0x1002 => match model_after(name, "RX") {
            Some(n) if (9000..10000).contains(&n) => Tier::Rdna4,
            Some(n) if (7000..8000).contains(&n) => Tier::Rdna3,
            Some(n) if (6000..7000).contains(&n) => Tier::Rdna2,
            _ => Tier::AmdOld,
        },
        _ => Tier::Intel,
    };
    Gpu { name: name.trim().to_string(), tier }
}

/// Picks the adapter with the most dedicated VRAM (the discrete card on laptops with an iGPU).
#[cfg(windows)]
pub fn detect() -> Option<Gpu> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut best: Option<(usize, Gpu)> = None;
    let mut i = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(i) } {
        i += 1;
        let Ok(d) = (unsafe { adapter.GetDesc1() }) else { continue };
        if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
        let gpu = classify(&String::from_utf16_lossy(&d.Description[..len]), d.VendorId);
        if best.as_ref().is_none_or(|(vram, _)| d.DedicatedVideoMemory > *vram) {
            best = Some((d.DedicatedVideoMemory, gpu));
        }
    }
    best.map(|(_, g)| g)
}

#[cfg(not(windows))]
pub fn detect() -> Option<Gpu> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_real_names() {
        let cases = [
            ("NVIDIA GeForce RTX 5090", 0x10DE, Tier::Rtx50),
            ("NVIDIA GeForce RTX 4060 Laptop GPU", 0x10DE, Tier::Rtx40),
            ("NVIDIA GeForce RTX 3070 Ti", 0x10DE, Tier::Rtx2030),
            ("NVIDIA GeForce RTX 2060", 0x10DE, Tier::Rtx2030),
            ("NVIDIA RTX A4000", 0x10DE, Tier::Rtx2030),
            ("NVIDIA GeForce GTX 1660 SUPER", 0x10DE, Tier::NvidiaOld),
            ("AMD Radeon RX 9070 XT", 0x1002, Tier::Rdna4),
            ("AMD Radeon RX 7900 XTX", 0x1002, Tier::Rdna3),
            ("AMD Radeon RX 6700 XT", 0x1002, Tier::Rdna2),
            ("Radeon RX 580 Series", 0x1002, Tier::AmdOld),
            ("AMD Radeon(TM) Graphics", 0x1002, Tier::AmdOld),
            ("Intel(R) Arc(TM) A770 Graphics", 0x8086, Tier::Intel),
        ];
        for (name, vendor, tier) in cases {
            assert_eq!(classify(name, vendor).tier, tier, "{name}");
        }
    }

    #[test]
    fn classifies_laptops_igpus_and_workstation_cards() {
        let cases = [
            ("NVIDIA GeForce RTX 5060 Laptop GPU", 0x10DE, Tier::Rtx50),
            ("NVIDIA GeForce RTX 4090 D", 0x10DE, Tier::Rtx40),
            ("NVIDIA GeForce RTX 3050 6GB Laptop GPU", 0x10DE, Tier::Rtx2030),
            // Workstation cards name the architecture instead of the series.
            ("NVIDIA RTX 2000 Ada Generation", 0x10DE, Tier::Rtx40),
            ("NVIDIA RTX 6000 Ada Generation", 0x10DE, Tier::Rtx40),
            ("NVIDIA RTX PRO 6000 Blackwell Workstation Edition", 0x10DE, Tier::Rtx50),
            ("NVIDIA GeForce GTX 1080 Ti", 0x10DE, Tier::NvidiaOld),
            ("AMD Radeon RX 9060 XT", 0x1002, Tier::Rdna4),
            ("AMD Radeon RX 7900 GRE", 0x1002, Tier::Rdna3),
            ("AMD Radeon RX 6600M", 0x1002, Tier::Rdna2),
            ("AMD Radeon 780M Graphics", 0x1002, Tier::AmdOld),
            ("Intel(R) Arc(TM) B580 Graphics", 0x8086, Tier::Intel),
            ("  NVIDIA GeForce RTX 4070  ", 0x10DE, Tier::Rtx40),
        ];
        for (name, vendor, tier) in cases {
            assert_eq!(classify(name, vendor).tier, tier, "{name}");
        }
        assert_eq!(classify("  NVIDIA GeForce RTX 4070  ", 0x10DE).name, "NVIDIA GeForce RTX 4070");
    }

    #[test]
    fn tier_flags_are_consistent() {
        for t in Tier::ALL {
            assert!(!t.is_rtx() || t.is_nvidia(), "{t:?}");
            assert!(!t.label().is_empty());
        }
        assert_eq!(Tier::ALL.iter().filter(|t| t.is_nvidia()).count(), 4);
    }
}
