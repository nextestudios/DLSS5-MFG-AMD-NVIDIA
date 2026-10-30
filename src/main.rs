#![cfg_attr(windows, windows_subsystem = "windows")]
mod games;
mod gpu;
mod install;
mod net;
mod plan;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use gpu::{Gpu, Tier};

const BG: Color32 = Color32::from_rgb(14, 15, 19);
const CARD: Color32 = Color32::from_rgb(24, 26, 32);
const CARD_HI: Color32 = Color32::from_rgb(34, 37, 46);
const MUTED: Color32 = Color32::from_rgb(140, 146, 160);

enum Msg {
    Log(String),
    Done(Result<(), String>),
    Library(Vec<Entry>),
}

/// A library game with the folder the mods go in (resolved once, in the background) and what is installed there.
struct Entry {
    found: games::Found,
    dir: Option<PathBuf>,
    state: Option<install::State>,
}

struct App {
    gpu: Gpu,
    detected: bool,
    dlss5: bool,
    mult: u8,
    game: Option<Result<PathBuf, String>>,
    library: Vec<Entry>,
    scanning: bool,
    search: String,
    store: Option<&'static str>,
    base_fps: u32,
    log: Vec<String>,
    show_log: bool,
    busy: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

fn accent(tier: Tier) -> Color32 {
    match tier {
        t if t.is_nvidia() => Color32::from_rgb(118, 185, 0),
        Tier::Intel => Color32::from_rgb(0, 140, 230),
        _ => Color32::from_rgb(237, 41, 57),
    }
}

fn store_color(store: &str) -> Color32 {
    match store {
        "Steam" => Color32::from_rgb(102, 192, 244),
        "Epic" => Color32::from_rgb(200, 200, 200),
        "GOG" => Color32::from_rgb(168, 85, 247),
        "Ubisoft" => Color32::from_rgb(0, 112, 255),
        "EA" => Color32::from_rgb(255, 71, 71),
        "Xbox" => Color32::from_rgb(16, 160, 16),
        _ => Color32::from_rgb(252, 175, 23),
    }
}

fn ago(when: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    match now.saturating_sub(when) / 86_400 {
        0 => "hoje".into(),
        1 => "ontem".into(),
        d => format!("há {d} dias"),
    }
}

fn scan_library(tx: Sender<Msg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let entries = games::scan().into_iter()
            .map(|found| {
                let dir = install::resolve_game_dir(&found.dir).ok();
                let state = dir.as_deref().and_then(install::read_state);
                Entry { found, dir, state }
            })
            .collect();
        let _ = tx.send(Msg::Library(entries));
        ctx.request_repaint();
    });
}

impl App {
    fn new(cc: &eframe::CreationContext, game_arg: Option<PathBuf>) -> App {
        let detected = gpu::detect();
        let gpu = detected.clone().unwrap_or(Gpu { name: "Placa não detectada".into(), tier: Tier::Rtx40 });
        let (tx, rx) = channel();
        style(&cc.egui_ctx, accent(gpu.tier));
        scan_library(tx.clone(), cc.egui_ctx.clone());
        let mut app = App {
            dlss5: gpu.tier.is_rtx(),
            mult: default_mult(gpu.tier),
            detected: detected.is_some(),
            gpu,
            game: None,
            library: Vec::new(),
            scanning: true,
            search: String::new(),
            store: None,
            base_fps: 60,
            log: Vec::new(),
            show_log: false,
            busy: false,
            tx,
            rx,
        };
        if let Some(p) = game_arg {
            app.set_game(&p);
        }
        app
    }

    /// Selecting a modded game brings back the choices it was installed with.
    fn set_game(&mut self, p: &Path) {
        let g = install::resolve_game_dir(p);
        if let Some(st) = g.as_ref().ok().and_then(|d| install::read_state(d)) {
            self.dlss5 = st.dlss5 && plan::dlss5_supported(self.gpu.tier);
            if plan::multipliers(self.gpu.tier).contains(&st.mult) {
                self.mult = st.mult;
            }
        }
        self.game = Some(g);
    }

    fn game_dir(&self) -> Option<&PathBuf> {
        self.game.as_ref().and_then(|g| g.as_ref().ok())
    }

    fn game_name(&self) -> String {
        let Some(d) = self.game_dir() else { return "Nenhum jogo escolhido".into() };
        self.library.iter().find(|e| d.starts_with(&e.found.dir)).map(|e| e.found.name.clone())
            .unwrap_or_else(|| {
                // bin/x64, Binaries/Win64, Content...: the game's name is the first folder above those.
                d.ancestors().filter_map(|a| a.file_name()).map(|f| f.to_string_lossy().into_owned())
                    .find(|n| !["bin", "bin64", "x64", "win64", "binaries", "content", "game", "retail"].contains(&n.to_lowercase().as_str()))
                    .unwrap_or_default()
            })
    }

    fn run(&mut self, ctx: &egui::Context, job: impl FnOnce(net::Log) -> Result<(), String> + Send + 'static) {
        self.busy = true;
        self.show_log = true;
        self.log.clear();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let log = |s: String| {
                let _ = tx.send(Msg::Log(s));
                ctx.request_repaint();
            };
            let r = job(&log);
            let _ = tx.send(Msg::Done(r));
            ctx.request_repaint();
        });
    }

    fn apply(&mut self, ctx: &egui::Context, neuralhub: bool) {
        let dir = self.game_dir().cloned().unwrap_or_default();
        if !neuralhub && !install::can_write(&dir) {
            if relaunch_as_admin(&dir) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.show_log = true;
                self.log = vec![format!("ERRO: sem permissão para gravar em {}. Abra o app como Administrador.", dir.display())];
            }
            return;
        }
        let (gpu, dlss5, mult) = (self.gpu.clone(), self.dlss5, self.mult);
        self.run(ctx, move |log| install::apply(&gpu, dlss5, mult, &dir, log));
    }

    fn modded_dirs(&self) -> Vec<PathBuf> {
        self.library.iter().filter(|e| e.state.is_some()).filter_map(|e| e.dir.clone()).collect()
    }

    fn refresh_states(&mut self) {
        for e in &mut self.library {
            e.state = e.dir.as_deref().and_then(install::read_state);
        }
    }
}

fn default_mult(tier: Tier) -> u8 {
    match tier {
        Tier::Rtx40 | Tier::Rtx50 => 4,
        _ => 2,
    }
}

fn style(ctx: &egui::Context, accent: Color32) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.extreme_bg_color = Color32::from_rgb(10, 11, 14);
        v.selection.bg_fill = accent.gamma_multiply(0.55);
        v.selection.stroke = Stroke::new(1.0, accent);
        v.hyperlink_color = accent;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(8);
        }
        v.widgets.inactive.weak_bg_fill = CARD_HI;
        v.widgets.inactive.bg_fill = CARD_HI;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 50, 62);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.7));
        v.widgets.active.weak_bg_fill = accent.gamma_multiply(0.6);
    });
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().fill(CARD).corner_radius(12).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    }).inner
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).size(11.0).color(MUTED).strong());
}

/// Jogo → Upscaler → DLSS 5 → Frame Gen → Tela, each stage lit when it does something.
fn pipeline(ui: &mut egui::Ui, stages: &[(String, String, bool)], accent: Color32) {
    let n = stages.len() as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
    let p = ui.painter_at(rect);
    let gap = 26.0;
    let w = (rect.width() - gap * (n - 1.0)) / n;
    for (i, (head, body, on)) in stages.iter().enumerate() {
        let x = rect.left() + i as f32 * (w + gap);
        let r = egui::Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height()));
        p.rect_filled(r, 10, if *on { accent.gamma_multiply(0.18) } else { CARD_HI });
        p.rect_stroke(r, 10, Stroke::new(1.0, if *on { accent } else { Color32::from_gray(60) }), StrokeKind::Inside);
        p.text(pos2(r.center().x, r.top() + 18.0), Align2::CENTER_CENTER, head, FontId::proportional(11.0), MUTED);
        let galley = p.layout(body.clone(), FontId::proportional(13.0), if *on { Color32::WHITE } else { MUTED }, w - 10.0);
        p.galley(pos2(r.center().x - galley.size().x / 2.0, r.top() + 30.0), galley, Color32::WHITE);
        if i + 1 < stages.len() {
            let y = r.center().y;
            let (a, b) = (r.right() + 5.0, r.right() + gap - 5.0);
            let c = if *on { accent } else { Color32::from_gray(80) };
            p.line_segment([pos2(a, y), pos2(b, y)], Stroke::new(2.0, c));
            p.line_segment([pos2(b - 5.0, y - 4.0), pos2(b, y)], Stroke::new(2.0, c));
            p.line_segment([pos2(b - 5.0, y + 4.0), pos2(b, y)], Stroke::new(2.0, c));
        }
    }
}

/// Real frames (solid) and generated ones (outlined), with a cursor running at the output rate.
fn frame_timeline(ui: &mut egui::Ui, mult: u8, accent: Color32) {
    let n = mult.max(1) as usize;
    let cycles = 4;
    let total = cycles * n;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
    let p = ui.painter_at(rect);
    let gap = 3.0;
    let w = (rect.width() - gap * (total as f32 - 1.0)) / total as f32;
    // Same wall-clock pace for every multiplier: more frames per second means a faster cursor.
    let t = ui.input(|i| i.time);
    let lit = (t * 1.5 * n as f64) as usize % total;
    for i in 0..total {
        let r = egui::Rect::from_min_size(pos2(rect.left() + i as f32 * (w + gap), rect.top()), vec2(w, rect.height()));
        let real = i % n == 0;
        let glow = if i == lit { 1.0 } else { 0.0 };
        if real {
            p.rect_filled(r, 5, accent.gamma_multiply(0.75 + 0.25 * glow));
        } else {
            p.rect_filled(r, 5, accent.gamma_multiply(0.12 + 0.45 * glow));
            p.rect_stroke(r, 5, Stroke::new(1.0, accent.gamma_multiply(0.7)), StrokeKind::Inside);
        }
        if w > 14.0 {
            p.text(r.center(), Align2::CENTER_CENTER, if real { "R" } else { "G" }, FontId::monospace(11.0), if real { BG } else { Color32::WHITE });
        }
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Log(s) => self.log.push(s),
                Msg::Done(r) => {
                    self.busy = false;
                    match r {
                        Ok(()) => self.log.push("✔ Concluído".into()),
                        Err(e) => self.log.push(format!("ERRO: {e}")),
                    }
                    self.refresh_states();
                }
                Msg::Library(l) => {
                    self.library = l;
                    self.scanning = false;
                }
            }
        }
        let ctx = ui.ctx().clone();
        let acc = accent(self.gpu.tier);
        let plan = plan::build(self.gpu.tier, self.dlss5, self.mult);

        egui::Panel::top("top").frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(16, 12))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("◆").size(22.0).color(acc));
                ui.vertical(|ui| {
                    ui.label(RichText::new("DLSS 5 + Frame Generation").size(18.0).strong().color(Color32::WHITE));
                    ui.label(RichText::new("Mods sempre na versão mais nova, em qualquer placa").size(11.0).color(MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let before = self.gpu.tier;
                    egui::ComboBox::from_id_salt("tier").selected_text(self.gpu.tier.label()).show_ui(ui, |ui| {
                        for t in Tier::ALL {
                            ui.selectable_value(&mut self.gpu.tier, t, t.label());
                        }
                    });
                    if self.gpu.tier != before {
                        self.dlss5 = self.gpu.tier.is_rtx();
                        self.mult = default_mult(self.gpu.tier);
                        style(&ctx, accent(self.gpu.tier));
                    }
                    egui::Frame::new().fill(acc.gamma_multiply(0.15)).stroke(Stroke::new(1.0, acc)).corner_radius(20)
                        .inner_margin(egui::Margin::symmetric(12, 5)).show(ui, |ui| {
                            let name = if self.detected { self.gpu.name.as_str() } else { "Não detectada: escolha o modelo →" };
                            ui.label(RichText::new(format!("● {name}")).color(acc).strong());
                        });
                });
            });
        });

        egui::Panel::left("library").resizable(true).default_size(300.0).min_size(240.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(18, 19, 24)).inner_margin(12)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    title(ui, &format!("Biblioteca · {}", self.library.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.scanning {
                            ui.spinner();
                        } else if ui.small_button("⟳").on_hover_text("Procurar jogos de novo").clicked() {
                            self.scanning = true;
                            scan_library(self.tx.clone(), ctx.clone());
                        }
                    });
                });
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("🔍 Buscar jogo").desired_width(f32::INFINITY));
                let mut stores: Vec<&'static str> = self.library.iter().map(|e| e.found.store).collect();
                stores.sort();
                stores.dedup();
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(self.store.is_none(), "Todas").clicked() {
                        self.store = None;
                    }
                    for s in stores {
                        if ui.selectable_label(self.store == Some(s), RichText::new(s).color(store_color(s))).clicked() {
                            self.store = Some(s);
                        }
                    }
                });
                ui.separator();
                let q = self.search.to_lowercase();
                let mut picked = None;
                let current = self.game_dir().cloned();
                egui::ScrollArea::vertical().auto_shrink(false).max_height(ui.available_height() - 44.0).show(ui, |ui| {
                    if self.library.is_empty() && !self.scanning {
                        ui.label(RichText::new("Nenhum jogo encontrado nas lojas. Use os botões abaixo.").color(MUTED));
                    }
                    for e in self.library.iter().filter(|e| e.found.name.to_lowercase().contains(&q) && self.store.is_none_or(|s| s == e.found.store)) {
                        let selected = current.as_ref().is_some_and(|d| d.starts_with(&e.found.dir));
                        let fill = if selected { acc.gamma_multiply(0.2) } else { Color32::TRANSPARENT };
                        let resp = egui::Frame::new().fill(fill).corner_radius(8).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("▍").color(store_color(e.found.store)));
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&e.found.name).color(Color32::WHITE));
                                    let sub = match &e.state {
                                        Some(st) => RichText::new(format!("✔ modado {} · {}", ago(st.when), if st.mult > 0 { format!("{}x", st.mult) } else { "sem FG".into() })).color(acc),
                                        None => RichText::new(e.found.store).color(MUTED),
                                    };
                                    ui.label(sub.size(11.0));
                                });
                            });
                        }).response.interact(Sense::click());
                        if resp.hovered() && !selected {
                            ui.painter().rect_stroke(resp.rect, 8, Stroke::new(1.0, Color32::from_gray(70)), StrokeKind::Inside);
                        }
                        if resp.clicked() {
                            picked = Some(e.found.dir.clone());
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("📁 Pasta…").clicked() && let Some(p) = rfd::FileDialog::new().set_title("Pasta do jogo").pick_folder() {
                        picked = Some(p);
                    }
                    if ui.button("🎮 .exe…").clicked() && let Some(p) = rfd::FileDialog::new().add_filter("Jogo", &["exe"]).pick_file() {
                        picked = Some(p);
                    }
                });
                if let Some(p) = picked {
                    self.set_game(&p);
                }
            });

        let modded = self.modded_dirs();
        egui::Panel::bottom("actions").frame(egui::Frame::new().fill(Color32::from_rgb(18, 19, 24)).inner_margin(12)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let ready = !self.busy && (plan.neuralhub || self.game_dir().is_some());
                let label = if plan.neuralhub { "⚡ Instalar DLSS 5 (NeuralHub)" } else { "⚡ Aplicar no jogo" };
                let apply = egui::Button::new(RichText::new(label).size(15.0).strong().color(BG)).fill(acc).min_size(vec2(220.0, 38.0));
                if ui.add_enabled(ready, apply).clicked() {
                    self.apply(&ctx, plan.neuralhub);
                }
                let has_mods = self.game_dir().is_some_and(|d| install::has_mods(d));
                if ui.add_enabled(!self.busy && has_mods, egui::Button::new("Remover do jogo").min_size(vec2(0.0, 38.0))).clicked() {
                    let dir = self.game_dir().cloned().unwrap_or_default();
                    self.run(&ctx, move |log| install::uninstall(&dir, log));
                }
                let update = egui::Button::new(format!("⟳ Atualizar todos ({})", modded.len())).min_size(vec2(0.0, 38.0));
                if ui.add_enabled(!self.busy && !modded.is_empty(), update)
                    .on_hover_text("Reaplica as escolhas salvas de cada jogo modado com as versões mais novas").clicked()
                {
                    let gpu = self.gpu.clone();
                    let dirs = modded.clone();
                    self.run(&ctx, move |log| install::update_all(&gpu, &dirs, log));
                }
                if self.busy {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let arrow = if self.show_log { "▼ Log" } else { "▲ Log" };
                    if ui.selectable_label(self.show_log, arrow).clicked() {
                        self.show_log = !self.show_log;
                    }
                    if let Some(last) = self.log.last().filter(|_| !self.show_log) {
                        ui.label(RichText::new(last).color(if last.starts_with("ERRO") { Color32::LIGHT_RED } else { MUTED }).size(11.0));
                    }
                });
            });
            if self.show_log {
                ui.add_space(6.0);
                egui::Frame::new().fill(Color32::from_rgb(10, 11, 14)).corner_radius(8).inner_margin(8).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(160.0).auto_shrink([false, true]).stick_to_bottom(true).show(ui, |ui| {
                        if self.log.is_empty() {
                            ui.label(RichText::new("Nada ainda.").color(MUTED));
                        }
                        for l in &self.log {
                            let c = if l.starts_with("ERRO") { Color32::LIGHT_RED } else if l.starts_with('✔') { acc } else { Color32::from_gray(190) };
                            ui.label(RichText::new(l).monospace().size(12.0).color(c));
                        }
                    });
                });
            }
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(16)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                // Game header
                card(ui, |ui| {
                    if plan.neuralhub {
                        title(ui, "DLSS 5 na AMD");
                        ui.label(RichText::new("NeuralHub cuida dos jogos").size(20.0).strong().color(Color32::WHITE));
                        ui.label(RichText::new("Ele tem a própria lista de jogos e o próprio loader: o app instala, baixa os DLLs e abre tudo.").color(MUTED));
                        return;
                    }
                    title(ui, "Jogo");
                    ui.label(RichText::new(self.game_name()).size(20.0).strong().color(Color32::WHITE));
                    match &self.game {
                        Some(Ok(d)) => {
                            ui.label(RichText::new(d.display().to_string()).size(11.0).color(MUTED));
                            if let Some(st) = install::read_state(d) {
                                let v: Vec<String> = st.versions.iter().map(|(k, v)| format!("{k} {v}")).collect();
                                ui.label(RichText::new(format!("✔ Instalado {} · {}", ago(st.when), v.join(" · "))).size(12.0).color(acc));
                            }
                        }
                        Some(Err(e)) => {
                            ui.colored_label(Color32::LIGHT_RED, e);
                        }
                        None => {
                            ui.label(RichText::new("Escolha um jogo na biblioteca à esquerda.").color(MUTED));
                        }
                    }
                });
                ui.add_space(4.0);

                // Features
                ui.columns(2, |cols| {
                    card(&mut cols[0], |ui| {
                        title(ui, "Neural Rendering");
                        let ok = plan::dlss5_supported(self.gpu.tier);
                        let route = if self.gpu.tier.is_nvidia() { "via DLSS Unlocked" } else { "via NeuralHub (DLSS-NR on AMD)" };
                        ui.add_enabled_ui(ok, |ui| {
                            let txt = RichText::new(if self.dlss5 { "DLSS 5  ●  LIGADO" } else { "DLSS 5  ○  desligado" }).size(16.0).strong();
                            let b = egui::Button::new(if self.dlss5 { txt.color(BG) } else { txt }).fill(if self.dlss5 { acc } else { CARD_HI })
                                .min_size(vec2(ui.available_width(), 44.0));
                            if ui.add(b).on_disabled_hover_text("Não disponível nesta placa").clicked() {
                                self.dlss5 = !self.dlss5;
                            }
                        });
                        ui.label(RichText::new(if ok { route } else { "Não disponível nesta placa" }).size(11.0).color(MUTED));
                    });
                    card(&mut cols[1], |ui| {
                        title(ui, "Frame Generation");
                        let ok = plan::multipliers(self.gpu.tier);
                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 4.0 * 6.0) / 5.0;
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for m in [0u8, 2, 3, 4, 6] {
                                let on = self.mult == m;
                                let t = RichText::new(if m == 0 { "Off".into() } else { format!("{m}x") }).size(16.0).strong();
                                let b = egui::Button::new(if on { t.color(BG) } else { t }).fill(if on { acc } else { CARD_HI }).min_size(vec2(w, 44.0));
                                if ui.add_enabled(ok.contains(&m), b).on_disabled_hover_text("Não disponível nesta placa").clicked() {
                                    self.mult = m;
                                }
                            }
                        });
                        ui.label(RichText::new("2x a 6x: quadros gerados entre os quadros reais").size(11.0).color(MUTED));
                    });
                });
                ui.add_space(4.0);

                // What happens to the image
                let [up, nr, fg] = plan.pipeline(self.gpu.tier, self.mult);
                let out_fps = self.base_fps * u32::from(self.mult.max(1));
                card(ui, |ui| {
                    title(ui, "Caminho da imagem");
                    let stages = [
                        ("JOGO".into(), format!("{} fps", self.base_fps), true),
                        ("UPSCALER".into(), up, true),
                        ("DLSS 5".into(), nr.clone(), nr != "Desligado"),
                        ("FRAME GEN".into(), fg.clone(), fg != "Desligado"),
                        ("TELA".into(), format!("{out_fps} fps"), true),
                    ];
                    pipeline(ui, &stages, acc);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        title(ui, "Linha do tempo de quadros");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(egui::Slider::new(&mut self.base_fps, 30..=144).suffix(" fps"));
                            ui.label(RichText::new("FPS do jogo:").size(12.0).color(MUTED));
                        });
                    });
                    frame_timeline(ui, self.mult, acc);
                    let extra = u32::from(self.mult.max(1)) - 1;
                    let msg = if extra == 0 {
                        format!("Sem geração de quadros: {} fps na tela.", self.base_fps)
                    } else {
                        format!("R = quadro real · G = gerado.  {} reais + {} gerados/s = {out_fps} fps na tela. A resposta do controle continua a de ~{} fps.",
                            self.base_fps, self.base_fps * extra, self.base_fps)
                    };
                    ui.label(RichText::new(msg).size(12.0).color(MUTED));
                });
                ui.add_space(4.0);

                // Install list
                card(ui, |ui| {
                    title(ui, "O que vai ser instalado");
                    let mut items = Vec::new();
                    match plan.opti {
                        Some(plan::Opti::Unlocked) => items.push("DLSS Unlocked (OptiScaler + DLSS 5)"),
                        Some(plan::Opti::Official) => items.push("OptiScaler"),
                        Some(plan::Opti::Nightly) => items.push("OptiScaler 10 (nightly, build do MFG Unlocker)"),
                        None => {}
                    }
                    if plan.reshade {
                        items.push("ReShade com add-ons");
                    }
                    if plan.mfg_addon {
                        items.push("MFG Unlock (RenoDX)");
                    }
                    if plan.mfg_unlocker {
                        items.push("MFG Unlocker (dashdogy)");
                    }
                    if plan.neuralhub {
                        items.push("NeuralHub + nvngx_dlssnr / dlss / dlssd / dlssg mais novos");
                    }
                    if items.is_empty() {
                        items.push("Nada: sua placa já faz isso nativamente");
                    }
                    ui.horizontal_wrapped(|ui| {
                        for i in items {
                            egui::Frame::new().fill(acc.gamma_multiply(0.15)).corner_radius(14).inner_margin(egui::Margin::symmetric(10, 4))
                                .show(ui, |ui| ui.label(RichText::new(i).color(Color32::WHITE)));
                        }
                    });
                    ui.add_space(4.0);
                    for n in &plan.notes {
                        ui.label(RichText::new(format!("•  {n}")).size(12.0).color(MUTED));
                    }
                    ui.label(RichText::new("•  Não use em jogos online com anti-cheat (risco de banimento). Todo download tem o SHA-256 conferido.").size(12.0).color(MUTED));
                });
            });
        });
    }
}

/// Restarts the app elevated with the same game folder (Program Files / Xbox games need admin).
#[cfg(windows)]
fn relaunch_as_admin(game: &Path) -> bool {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let Ok(exe) = std::env::current_exe() else { return false };
    let params = HSTRING::from(format!("--game \"{}\"", game.display()));
    let r = unsafe { ShellExecuteW(None, w!("runas"), &HSTRING::from(exe.as_os_str()), &params, None, SW_SHOWNORMAL) };
    r.0 as usize > 32
}

#[cfg(not(windows))]
fn relaunch_as_admin(_: &Path) -> bool {
    false
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let game = std::iter::from_fn(|| args.next())
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "--game")
        .map(|w| PathBuf::from(&w[1]));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 780.0]).with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native("DLSS 5 + Frame Generation", options, Box::new(|cc| Ok(Box::new(App::new(cc, game)))))
}
