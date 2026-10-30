#![cfg_attr(windows, windows_subsystem = "windows")]
mod art;
mod games;
mod gpu;
mod install;
mod net;
mod plan;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, pos2, vec2,
};
use gpu::{Gpu, Tier};

const BG: Color32 = Color32::from_rgb(11, 12, 16);
const RAIL: Color32 = Color32::from_rgb(16, 17, 23);
const CARD: Color32 = Color32::from_rgb(22, 24, 31);
const CARD_HI: Color32 = Color32::from_rgb(32, 35, 45);
const MUTED: Color32 = Color32::from_rgb(138, 144, 160);
const CARD_W: f32 = 168.0;
const GAP: f32 = 18.0;

enum Msg {
    Log(String),
    Done(Result<(), String>),
    Library(Vec<Entry>),
    Art(PathBuf, art::Art),
}

/// A library game, the folder the mods go in, what is installed there and its cover art.
struct Entry {
    found: games::Found,
    dir: Option<PathBuf>,
    state: Option<install::State>,
    art: art::Art,
}

impl Entry {
    fn new(found: games::Found) -> Entry {
        let dir = install::resolve_game_dir(&found.dir).ok();
        let state = dir.as_deref().and_then(install::read_state);
        Entry { found, dir, state, art: art::Art::default() }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Store(&'static str),
    Modded,
}

#[derive(Clone, PartialEq)]
enum View {
    Library,
    /// Keyed by the library folder (`Found::dir`).
    Game(PathBuf),
}

/// What the user clicked this frame; handled after drawing so nothing is borrowed.
enum Act {
    Open(PathBuf),
    Quick(PathBuf),
    RemoveMods(PathBuf),
    Forget(PathBuf),
    OpenFolder(PathBuf),
    AddFolder,
    Added(PathBuf),
}

/// The running (or last) apply/remove job, shown as a floating card.
struct Task {
    title: String,
    log: Vec<String>,
    done: Option<Result<(), String>>,
    full_log: bool,
}

struct App {
    gpu: Gpu,
    detected: bool,
    dlss5: bool,
    mult: u8,
    library: Vec<Entry>,
    scanning: bool,
    search: String,
    filter: Filter,
    view: View,
    pending_open: Option<PathBuf>,
    base_fps: u32,
    task: Option<Task>,
    fade: Option<TextureHandle>,
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
        "Rockstar" => Color32::from_rgb(252, 175, 23),
        _ => Color32::from_rgb(236, 72, 153),
    }
}

fn store_label(store: &str) -> &str {
    if store == "Manual" { "Adicionado" } else { store }
}

fn ago(when: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    match now.saturating_sub(when) / 86_400 {
        0 => "hoje".into(),
        1 => "ontem".into(),
        d => format!("há {d} dias"),
    }
}

/// Up to two initials for the generated cover ("Red Dead Redemption 2" -> "RD").
fn initials(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).take(2)
        .filter_map(|w| w.chars().next()).flat_map(char::to_uppercase).collect()
}

/// A stable colour per game for the generated cover.
fn name_hue(name: &str) -> f32 {
    let h = name.bytes().fold(2_166_136_261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16_777_619));
    (h % 360) as f32 / 360.0
}

fn file_uri(p: &Path) -> String {
    format!("file://{}", p.display())
}

fn scan_library(tx: Sender<Msg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let entries = games::scan().into_iter().map(Entry::new).collect();
        let _ = tx.send(Msg::Library(entries));
        ctx.request_repaint();
    });
}

fn fetch_art(games: Vec<games::Found>, tx: Sender<Msg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        for g in games {
            let a = art::fetch(&g);
            if tx.send(Msg::Art(g.dir, a)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
}

impl App {
    fn new(ctx: &egui::Context, game_arg: Option<PathBuf>) -> App {
        egui_extras::install_image_loaders(ctx);
        let detected = gpu::detect();
        let gpu = detected.clone().unwrap_or(Gpu { name: "Placa não detectada".into(), tier: Tier::Rtx40 });
        let (tx, rx) = channel();
        style(ctx, accent(gpu.tier));
        scan_library(tx.clone(), ctx.clone());
        App {
            dlss5: gpu.tier.is_rtx(),
            mult: default_mult(gpu.tier),
            detected: detected.is_some(),
            gpu,
            library: Vec::new(),
            scanning: true,
            search: String::new(),
            filter: Filter::All,
            view: View::Library,
            pending_open: game_arg,
            base_fps: 60,
            task: None,
            fade: None,
            tx,
            rx,
        }
    }

    fn entry(&self, key: &Path) -> Option<&Entry> {
        self.library.iter().find(|e| e.found.dir == key)
    }

    /// Opens the game page; a folder that isn't in the library is added to it (and remembered).
    fn open(&mut self, path: &Path, ctx: &egui::Context) {
        let path = if path.is_file() { path.parent().unwrap_or(path).to_path_buf() } else { path.to_path_buf() };
        if let Some(e) = self.library.iter().find(|e| path.starts_with(&e.found.dir) || e.dir.as_deref() == Some(path.as_path())) {
            let key = e.found.dir.clone();
            self.select(&key);
            return;
        }
        match install::resolve_game_dir(&path).and_then(|_| games::add_manual(&path)) {
            Ok(found) => {
                fetch_art(vec![found.clone()], self.tx.clone(), ctx.clone());
                let key = found.dir.clone();
                self.library.push(Entry::new(found));
                self.select(&key);
            }
            Err(e) => self.notify("Não deu para adicionar", Err(e)),
        }
    }

    /// Selecting a modded game brings back the choices it was installed with.
    fn select(&mut self, key: &Path) {
        if let Some(st) = self.entry(key).and_then(|e| e.state.clone()) {
            self.dlss5 = st.dlss5 && plan::dlss5_supported(self.gpu.tier);
            if plan::multipliers(self.gpu.tier).contains(&st.mult) {
                self.mult = st.mult;
            }
        }
        self.view = View::Game(key.to_path_buf());
    }

    fn notify(&mut self, title: &str, r: Result<(), String>) {
        let log = r.as_ref().err().map(|e| vec![format!("ERRO: {e}")]).unwrap_or_default();
        self.task = Some(Task { title: title.into(), log, done: Some(r), full_log: false });
    }

    fn run(&mut self, ctx: &egui::Context, title: String, job: impl FnOnce(net::Log) -> Result<(), String> + Send + 'static) {
        self.task = Some(Task { title, log: Vec::new(), done: None, full_log: false });
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

    fn busy(&self) -> bool {
        self.task.as_ref().is_some_and(|t| t.done.is_none())
    }

    fn apply(&mut self, ctx: &egui::Context, key: &Path) {
        if self.busy() {
            return;
        }
        let plan = plan::build(self.gpu.tier, self.dlss5, self.mult);
        let (gpu, dlss5, mult) = (self.gpu.clone(), self.dlss5, self.mult);
        if plan.neuralhub {
            self.run(ctx, "DLSS 5 na AMD (NeuralHub)".into(), move |log| install::apply(&gpu, dlss5, mult, Path::new(""), log));
            return;
        }
        let Some(e) = self.entry(key) else { return };
        let name = e.found.name.clone();
        let Some(dir) = e.dir.clone() else {
            self.notify(&name, Err("Não achei o .exe do jogo nessa pasta".into()));
            return;
        };
        if !install::can_write(&dir) {
            if relaunch_as_admin(&dir) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.notify(&name, Err(format!("Sem permissão para gravar em {}. Abra o app como Administrador.", dir.display())));
            }
            return;
        }
        self.run(ctx, format!("Aplicando em {name}"), move |log| install::apply(&gpu, dlss5, mult, &dir, log));
    }

    fn refresh_states(&mut self) {
        for e in &mut self.library {
            e.state = e.dir.as_deref().and_then(install::read_state);
        }
    }

    fn handle(&mut self, act: Act, ctx: &egui::Context) {
        match act {
            Act::Open(k) => self.select(&k),
            Act::Quick(k) => self.apply(ctx, &k),
            Act::RemoveMods(k) => {
                if let Some((name, dir)) = self.entry(&k).and_then(|e| Some((e.found.name.clone(), e.dir.clone()?))) {
                    self.run(ctx, format!("Removendo de {name}"), move |log| install::uninstall(&dir, log));
                }
            }
            Act::Forget(k) => {
                let r = games::remove_manual(&k);
                self.library.retain(|e| e.found.dir != k);
                self.view = View::Library;
                if r.is_err() {
                    self.notify("Tirar da biblioteca", r);
                }
            }
            Act::OpenFolder(p) => {
                let _ = std::process::Command::new(if cfg!(windows) { "explorer" } else { "open" }).arg(p).spawn();
            }
            Act::AddFolder => {
                if let Some(p) = rfd::FileDialog::new().set_title("Pasta do jogo").pick_folder() {
                    self.open(&p, ctx);
                }
            }
            Act::Added(p) => self.open(&p, ctx),
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
        v.window_stroke = Stroke::new(1.0, Color32::from_gray(45));
        v.extreme_bg_color = Color32::from_rgb(8, 9, 12);
        v.selection.bg_fill = accent.gamma_multiply(0.55);
        v.selection.stroke = Stroke::new(1.0, accent);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
            w.corner_radius = CornerRadius::same(10);
        }
        v.widgets.inactive.weak_bg_fill = CARD_HI;
        v.widgets.inactive.bg_fill = CARD_HI;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 48, 60);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.7));
        v.widgets.active.weak_bg_fill = accent.gamma_multiply(0.6);
    });
}

/// Vertical fade (transparent at the top, `color` at the bottom), with rounded corners.
fn fade(p: &egui::Painter, tex: &TextureHandle, rect: Rect, radius: impl Into<CornerRadius>, color: Color32) {
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    p.add(egui::epaint::RectShape::filled(rect, radius, color).with_texture(tex.id(), uv));
}

/// Cover art, or a generated one (colour from the name, initials) when the game has none.
fn paint_cover(ui: &egui::Ui, tex: &TextureHandle, rect: Rect, e: &Entry) {
    let p = ui.painter();
    if let Some(poster) = &e.art.poster {
        egui::Image::new(file_uri(poster)).corner_radius(14).paint_at(ui, rect);
        return;
    }
    let hue = name_hue(&e.found.name);
    p.rect_filled(rect, 14, Color32::from(egui::ecolor::Hsva::new(hue, 0.55, 0.32, 1.0)));
    fade(p, tex, rect, 14, Color32::from(egui::ecolor::Hsva::new((hue + 0.12) % 1.0, 0.7, 0.12, 1.0)));
    p.text(rect.center() - vec2(0.0, 24.0), Align2::CENTER_CENTER, initials(&e.found.name), FontId::proportional(54.0), Color32::from_white_alpha(225));
    let g = p.layout(e.found.name.clone(), FontId::proportional(15.0), Color32::WHITE, rect.width() - 24.0);
    p.galley(pos2(rect.center().x - g.size().x / 2.0, rect.center().y + 22.0), g, Color32::WHITE);
}

fn chip(p: &egui::Painter, pos: Pos2, anchor: Align2, text: &str, fg: Color32, bg: Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_string(), FontId::proportional(11.0), fg);
    let r = anchor.anchor_size(pos, g.size() + vec2(14.0, 6.0));
    p.rect_filled(r, 8, bg);
    p.galley(r.min + vec2(7.0, 3.0), g, fg);
    r
}

fn card_grid(ui: &mut egui::Ui, tex: &TextureHandle, entries: &[&Entry], acc: Color32, acts: &mut Vec<Act>) {
    // As many ~168 px columns as fit, then stretched to fill the row exactly (posters stay 2:3).
    let avail = ui.available_width();
    let cols = (((avail + GAP) / (CARD_W + GAP)).floor() as usize).max(1);
    let card_w = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32).min(CARD_W * 1.35);
    let card_h = card_w * 1.5;
    let mut items: Vec<Option<&Entry>> = vec![None];
    items.extend(entries.iter().map(|e| Some(*e)));
    for row in items.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for item in row {
                let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h + 24.0), Sense::click());
                let t = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.14);
                let card = Rect::from_min_size(rect.min - vec2(0.0, 6.0 * t), vec2(card_w, card_h));
                let p = ui.painter();
                let Some(e) = item else {
                    // "Add game" tile.
                    p.rect_filled(card, 14, CARD.gamma_multiply(0.6 + 0.4 * t));
                    p.rect_stroke(card, 14, Stroke::new(1.5, acc.gamma_multiply(0.35 + 0.65 * t)), StrokeKind::Inside);
                    p.text(card.center() - vec2(0.0, 20.0), Align2::CENTER_CENTER, "+", FontId::proportional(56.0), acc);
                    p.text(card.center() + vec2(0.0, 30.0), Align2::CENTER_CENTER, "Adicionar jogo", FontId::proportional(14.0), Color32::WHITE);
                    p.text(card.center() + vec2(0.0, 50.0), Align2::CENTER_CENTER, "escolha a pasta ou arraste aqui", FontId::proportional(11.0), MUTED);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        acts.push(Act::AddFolder);
                    }
                    continue;
                };
                if t > 0.0 {
                    p.rect_filled(card.expand(6.0 * t), 18, acc.gamma_multiply(0.18 * t));
                }
                paint_cover(ui, tex, card, e);
                let p = ui.painter();
                let bottom = Rect::from_min_max(pos2(card.left(), card.bottom() - 90.0), card.max);
                fade(p, tex, bottom, CornerRadius { nw: 0, ne: 0, sw: 14, se: 14 }, Color32::from_black_alpha(230));
                let edge = if t > 0.0 { acc.gamma_multiply(0.4 + 0.6 * t) } else { Color32::from_white_alpha(18) };
                p.rect_stroke(card, 14, Stroke::new(1.0 + t, edge), StrokeKind::Inside);
                chip(p, card.left_top() + vec2(8.0, 8.0), Align2::LEFT_TOP, store_label(e.found.store), Color32::BLACK, store_color(e.found.store));
                if let Some(st) = &e.state {
                    let label = if st.mult > 0 { format!("✔ {}x", st.mult) } else { "✔ MOD".into() };
                    chip(p, card.right_top() + vec2(-8.0, 8.0), Align2::RIGHT_TOP, &label, Color32::BLACK, acc);
                }
                // Quick apply: appears on hover, applies the current preset.
                let quick = Rect::from_min_size(pos2(card.left() + 10.0, card.bottom() - 44.0), vec2(card_w - 20.0, 34.0));
                if t > 0.01 {
                    let over = resp.hover_pos().is_some_and(|h| quick.contains(h));
                    p.rect_filled(quick, 10, acc.gamma_multiply(if over { 1.0 } else { 0.8 } * t));
                    p.text(quick.center(), Align2::CENTER_CENTER, "⚡ Aplicar", FontId::proportional(14.0), BG);
                } else {
                    let g = p.layout(e.found.name.clone(), FontId::proportional(13.0), Color32::WHITE, card_w - 24.0);
                    p.galley(pos2(card.left() + 12.0, card.bottom() - 12.0 - g.size().y), g, Color32::WHITE);
                }
                if let Some(s) = &e.state {
                    let d5 = if s.dlss5 { "DLSS 5 · " } else { "" };
                    let fg = if s.mult > 0 { format!("{}x · ", s.mult) } else { String::new() };
                    p.text(pos2(rect.left() + 2.0, rect.bottom() - 10.0), Align2::LEFT_CENTER, format!("{d5}{fg}modado {}", ago(s.when)), FontId::proportional(11.0), acc);
                }

                let key = e.found.dir.clone();
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    let on_quick = resp.interact_pointer_pos().is_some_and(|h| quick.contains(h));
                    acts.push(if on_quick { Act::Quick(key.clone()) } else { Act::Open(key.clone()) });
                }
                resp.context_menu(|ui| {
                    if ui.button("Abrir").clicked() {
                        acts.push(Act::Open(key.clone()));
                    }
                    if ui.button("⚡ Aplicar preset").clicked() {
                        acts.push(Act::Quick(key.clone()));
                    }
                    if e.state.is_some() && ui.button("Remover mods").clicked() {
                        acts.push(Act::RemoveMods(key.clone()));
                    }
                    if ui.button("Abrir pasta").clicked() {
                        acts.push(Act::OpenFolder(e.dir.clone().unwrap_or(key.clone())));
                    }
                    if e.found.store == "Manual" && ui.button("Tirar da biblioteca").clicked() {
                        acts.push(Act::Forget(key.clone()));
                    }
                });
            }
        });
        ui.add_space(GAP - 8.0);
    }
}

/// DLSS 5 switch + frame generation segmented control; `big` for the game page.
fn feature_controls(ui: &mut egui::Ui, app: &mut App, acc: Color32, big: bool) {
    let h = if big { 46.0 } else { 30.0 };
    let size = if big { 16.0 } else { 13.0 };
    let ok5 = plan::dlss5_supported(app.gpu.tier);
    ui.add_enabled_ui(ok5, |ui| {
        let on = app.dlss5 && ok5;
        let t = RichText::new(if on { "DLSS 5  ●" } else { "DLSS 5  ○" }).size(size).strong();
        let w = if big { ui.available_width() } else { 96.0 };
        let b = egui::Button::new(if on { t.color(BG) } else { t }).fill(if on { acc } else { CARD_HI }).min_size(vec2(w, h));
        let hint = if app.gpu.tier.is_nvidia() { "DLSS 5 Neural Rendering via DLSS Unlocked" } else { "DLSS 5 via NeuralHub (DLSS-NR on AMD)" };
        if ui.add(b).on_hover_text(hint).on_disabled_hover_text("Esta placa não roda DLSS 5").clicked() {
            app.dlss5 = !app.dlss5;
        }
    });
    if big {
        ui.add_space(4.0);
        ui.label(RichText::new("FRAME GENERATION").size(11.0).color(MUTED).strong());
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let ok = plan::multipliers(app.gpu.tier);
        let w = if big { (ui.available_width() - 16.0) / 5.0 } else { 44.0 };
        for m in [0u8, 2, 3, 4, 6] {
            let on = app.mult == m;
            let t = RichText::new(if m == 0 { "Off".into() } else { format!("{m}x") }).size(size).strong();
            let b = egui::Button::new(if on { t.color(BG) } else { t }).fill(if on { acc } else { CARD_HI }).min_size(vec2(w, h));
            if ui.add_enabled(ok.contains(&m), b).on_disabled_hover_text("Esta placa não chega nesse multiplicador").clicked() {
                app.mult = m;
            }
        }
    });
}

/// Jogo → Upscaler → DLSS 5 → Frame Gen → Tela, each stage lit when it does something.
fn pipeline(ui: &mut egui::Ui, stages: &[(String, String, bool)], accent: Color32) {
    let n = stages.len() as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 78.0), Sense::hover());
    let p = ui.painter_at(rect);
    let gap = 18.0;
    let w = (rect.width() - gap * (n - 1.0)) / n;
    for (i, (head, body, on)) in stages.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * (w + gap), rect.top()), vec2(w, rect.height()));
        p.rect_filled(r, 10, if *on { accent.gamma_multiply(0.16) } else { CARD_HI });
        p.rect_stroke(r, 10, Stroke::new(1.0, if *on { accent } else { Color32::from_gray(55) }), StrokeKind::Inside);
        p.text(pos2(r.center().x, r.top() + 15.0), Align2::CENTER_CENTER, head, FontId::proportional(10.0), MUTED);
        let g = p.layout(body.clone(), FontId::proportional(11.5), if *on { Color32::WHITE } else { MUTED }, w - 8.0);
        p.galley(pos2(r.center().x - g.size().x / 2.0, r.top() + 26.0), g, Color32::WHITE);
        if i + 1 < stages.len() {
            let (y, a, b) = (r.center().y, r.right() + 4.0, r.right() + gap - 4.0);
            let c = if *on { accent } else { Color32::from_gray(70) };
            p.line_segment([pos2(a, y), pos2(b, y)], Stroke::new(2.0, c));
            p.line_segment([pos2(b - 5.0, y - 4.0), pos2(b, y)], Stroke::new(2.0, c));
            p.line_segment([pos2(b - 5.0, y + 4.0), pos2(b, y)], Stroke::new(2.0, c));
        }
    }
}

/// Real frames (solid) and generated ones (outlined), with a cursor running at the output rate.
fn frame_timeline(ui: &mut egui::Ui, mult: u8, accent: Color32) {
    let n = mult.max(1) as usize;
    let total = 4 * n;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let p = ui.painter_at(rect);
    let gap = 3.0;
    let w = (rect.width() - gap * (total as f32 - 1.0)) / total as f32;
    // Same wall-clock pace for every multiplier: more frames per second means a faster cursor.
    let lit = (ui.input(|i| i.time) * 1.5 * n as f64) as usize % total;
    for i in 0..total {
        let r = Rect::from_min_size(pos2(rect.left() + i as f32 * (w + gap), rect.top()), vec2(w, rect.height()));
        let (real, glow) = (i % n == 0, if i == lit { 1.0 } else { 0.0 });
        if real {
            p.rect_filled(r, 5, accent.gamma_multiply(0.75 + 0.25 * glow));
        } else {
            p.rect_filled(r, 5, accent.gamma_multiply(0.1 + 0.45 * glow));
            p.rect_stroke(r, 5, Stroke::new(1.0, accent.gamma_multiply(0.6)), StrokeKind::Inside);
        }
        if w > 14.0 {
            p.text(r.center(), Align2::CENTER_CENTER, if real { "R" } else { "G" }, FontId::monospace(11.0), if real { BG } else { Color32::WHITE });
        }
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
}

fn section<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().fill(CARD).corner_radius(16).inner_margin(18).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title.to_uppercase()).size(11.0).color(MUTED).strong());
        ui.add_space(2.0);
        add(ui)
    }).inner
}

fn game_page(ui: &mut egui::Ui, app: &mut App, key: &Path, acc: Color32, acts: &mut Vec<Act>) {
    let Some(e) = app.entry(key) else {
        app.view = View::Library;
        return;
    };
    let (name, store, dir, state, art) = (e.found.name.clone(), e.found.store, e.dir.clone(), e.state.clone(), e.art.clone());
    let tex = app.fade.clone().expect("fade texture");
    let plan = plan::build(app.gpu.tier, app.dlss5, app.mult);

    // Hero: Steam background cropped to fill, fading into the page; logo or title on top.
    let (hero, _) = ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
    if let Some(h) = &art.hero {
        let (img, r) = (1920.0 / 620.0, hero.width() / hero.height());
        let uv = if r > img {
            let hgt = img / r;
            Rect::from_min_max(pos2(0.0, (1.0 - hgt) / 2.0), pos2(1.0, (1.0 + hgt) / 2.0))
        } else {
            let wid = r / img;
            Rect::from_min_max(pos2((1.0 - wid) / 2.0, 0.0), pos2((1.0 + wid) / 2.0, 1.0))
        };
        egui::Image::new(file_uri(h)).uv(uv).paint_at(ui, hero);
    } else {
        let hue = name_hue(&name);
        ui.painter().rect_filled(hero, 0, Color32::from(egui::ecolor::Hsva::new(hue, 0.6, 0.35, 1.0)));
        ui.painter().text(hero.right_center() - vec2(80.0, 0.0), Align2::RIGHT_CENTER, initials(&name), FontId::proportional(180.0), Color32::from_white_alpha(28));
    }
    fade(ui.painter(), &tex, hero, 0, BG);
    let logo_box = Rect::from_min_size(pos2(hero.left() + 36.0, hero.bottom() - 190.0), vec2(480.0, 140.0));
    match &art.logo {
        Some(l) => {
            // A child Ui: the logo must not move the page's cursor (the cards start below the hero).
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(logo_box).layout(egui::Layout::left_to_right(egui::Align::Max)));
            child.add(egui::Image::new(file_uri(l)).fit_to_exact_size(logo_box.size()).maintain_aspect_ratio(true));
        }
        None => {
            let g = ui.painter().layout(name.clone(), FontId::proportional(40.0), Color32::WHITE, 700.0);
            ui.painter().galley(pos2(logo_box.left(), logo_box.bottom() - g.size().y), g, Color32::WHITE);
        }
    }
    let p = ui.painter();
    let y = hero.bottom() - 34.0;
    let x = chip(p, pos2(hero.left() + 36.0, y), Align2::LEFT_CENTER, store_label(store), Color32::BLACK, store_color(store)).right() + 6.0;
    if let Some(st) = &state {
        let fg = if st.mult > 0 { format!(" · {}x", st.mult) } else { String::new() };
        let d5 = if st.dlss5 { " · DLSS 5" } else { "" };
        chip(p, pos2(x, y), Align2::LEFT_CENTER, &format!("✔ modado {}{d5}{fg}", ago(st.when)), Color32::BLACK, acc);
    }
    let back = Rect::from_min_size(hero.left_top() + vec2(20.0, 18.0), vec2(118.0, 34.0));
    let b = ui.interact(back, Id::unique("back"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter().rect_filled(back, 17, Color32::from_black_alpha(if b.hovered() { 220 } else { 150 }));
    ui.painter().text(back.center(), Align2::CENTER_CENTER, "←  Biblioteca", FontId::proportional(13.0), Color32::WHITE);
    if b.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.view = View::Library;
    }

    ui.add_space(6.0);
    egui::Frame::new().inner_margin(egui::Margin { left: 28, right: 28, top: 0, bottom: 28 }).show(ui, |ui| {
        ui.columns(2, |c| {
            section(&mut c[0], "Recursos", |ui| feature_controls(ui, app, acc, true));
            c[0].add_space(10.0);
            let [up, nr, fg] = plan.pipeline(app.gpu.tier, app.mult);
            let out = app.base_fps * u32::from(app.mult.max(1));
            section(&mut c[0], "Caminho da imagem", |ui| {
                let stages = [
                    ("JOGO".into(), format!("{} fps", app.base_fps), true),
                    ("UPSCALER".into(), up, true),
                    ("DLSS 5".into(), nr.clone(), nr != "Desligado"),
                    ("FRAME GEN".into(), fg.clone(), fg != "Desligado"),
                    ("TELA".into(), format!("{out} fps"), true),
                ];
                pipeline(ui, &stages, acc);
                ui.add_space(6.0);
                frame_timeline(ui, app.mult, acc);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FPS do jogo").size(12.0).color(MUTED));
                    ui.add(egui::Slider::new(&mut app.base_fps, 30..=144).suffix(" fps"));
                });
                let extra = u32::from(app.mult.max(1)) - 1;
                let msg = if extra == 0 {
                    format!("Sem geração de quadros: {} fps na tela.", app.base_fps)
                } else {
                    format!("R = real · G = gerado. {} reais + {} gerados = {out} fps na tela; o controle responde como em ~{} fps.", app.base_fps, app.base_fps * extra, app.base_fps)
                };
                ui.label(RichText::new(msg).size(12.0).color(MUTED));
            });

            section(&mut c[1], "Instalação", |ui| {
                let label = if plan.neuralhub { "⚡  Instalar DLSS 5 (NeuralHub)" } else { "⚡  Aplicar neste jogo" };
                let busy = app.busy();
                let go = egui::Button::new(RichText::new(label).size(18.0).strong().color(BG)).fill(acc).min_size(vec2(ui.available_width(), 56.0));
                if ui.add_enabled(!busy && (plan.neuralhub || dir.is_some()), go).clicked() {
                    acts.push(Act::Quick(key.to_path_buf()));
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy && state.is_some(), egui::Button::new("Remover mods")).clicked() {
                        acts.push(Act::RemoveMods(key.to_path_buf()));
                    }
                    if ui.button("Abrir pasta").clicked() {
                        acts.push(Act::OpenFolder(dir.clone().unwrap_or(key.to_path_buf())));
                    }
                    if store == "Manual" && ui.button("Tirar da biblioteca").clicked() {
                        acts.push(Act::Forget(key.to_path_buf()));
                    }
                });
                match &dir {
                    Some(d) => ui.label(RichText::new(d.display().to_string()).size(11.0).color(MUTED)),
                    None => ui.colored_label(Color32::LIGHT_RED, "Não achei o .exe do jogo nesta pasta"),
                };
                if let Some(st) = &state {
                    ui.add_space(4.0);
                    for (k, v) in &st.versions {
                        ui.label(RichText::new(format!("✔ {k}  {v}")).size(12.0).color(acc));
                    }
                }
                ui.separator();
                ui.label(RichText::new("VAI SER INSTALADO").size(11.0).color(MUTED).strong());
                let mut items = Vec::new();
                match plan.opti {
                    Some(plan::Opti::Unlocked) => items.push("DLSS Unlocked"),
                    Some(plan::Opti::Official) => items.push("OptiScaler"),
                    Some(plan::Opti::Nightly) => items.push("OptiScaler 10"),
                    None => {}
                }
                if plan.reshade {
                    items.push("ReShade");
                }
                if plan.mfg_addon {
                    items.push("MFG Unlock (RenoDX)");
                }
                if plan.mfg_unlocker {
                    items.push("MFG Unlocker");
                }
                if plan.neuralhub {
                    items.push("NeuralHub + DLLs DLSS");
                }
                if items.is_empty() {
                    items.push("Nada: a placa já faz isso");
                }
                ui.horizontal_wrapped(|ui| {
                    for i in items {
                        egui::Frame::new().fill(acc.gamma_multiply(0.16)).corner_radius(12).inner_margin(egui::Margin::symmetric(10, 4))
                            .show(ui, |ui| ui.label(RichText::new(i).color(Color32::WHITE)));
                    }
                });
                for n in &plan.notes {
                    ui.label(RichText::new(format!("•  {n}")).size(12.0).color(MUTED));
                }
                ui.label(RichText::new("•  Não use em jogos online com anti-cheat. Todo download tem o SHA-256 conferido.").size(12.0).color(MUTED));
            });
        });
    });
}

fn library_page(ui: &mut egui::Ui, app: &mut App, acc: Color32, acts: &mut Vec<Act>) {
    egui::Frame::new().inner_margin(egui::Margin { left: 28, right: 28, top: 22, bottom: 28 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            let title = match app.filter {
                Filter::All => "Todos os jogos".to_string(),
                Filter::Modded => "Modados".into(),
                Filter::Store("Manual") => "Adicionados".into(),
                Filter::Store(s) => s.to_string(),
            };
            ui.label(RichText::new(title).size(26.0).strong().color(Color32::WHITE));
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut app.search).hint_text("🔍  Buscar jogo").desired_width(240.0).margin(vec2(10.0, 7.0)));
        });
        ui.add_space(8.0);
        egui::Frame::new().fill(CARD).corner_radius(14).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("⚡ APLICAR RÁPIDO USA").size(11.0).color(MUTED).strong());
                ui.add_space(6.0);
                feature_controls(ui, app, acc, false);
                let [_, _, fg] = plan::build(app.gpu.tier, app.dlss5, app.mult).pipeline(app.gpu.tier, app.mult);
                ui.label(RichText::new(fg).size(12.0).color(acc));
            });
        });
        ui.add_space(16.0);
        let q = art::normalize(&app.search);
        let shown: Vec<&Entry> = app.library.iter()
            .filter(|e| match app.filter {
                Filter::All => true,
                Filter::Modded => e.state.is_some(),
                Filter::Store(s) => e.found.store == s,
            })
            .filter(|e| art::normalize(&e.found.name).contains(&q))
            .collect();
        let tex = app.fade.as_ref().expect("fade texture");
        card_grid(ui, tex, &shown, acc, acts);
        if shown.is_empty() && !app.scanning {
            ui.label(RichText::new("Nenhum jogo aqui. Clique em “Adicionar jogo” ou arraste a pasta do jogo para a janela.").color(MUTED));
        }
    });
}

fn rail(ui: &mut egui::Ui, app: &mut App, acc: Color32, ctx: &egui::Context) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("◆").size(22.0).color(acc));
        ui.vertical(|ui| {
            ui.label(RichText::new("NEURAL · FG").size(16.0).strong().color(Color32::WHITE));
            ui.label(RichText::new(concat!("DLSS 5 + MFG  ·  v", env!("CARGO_PKG_VERSION"))).size(10.0).color(MUTED));
        });
    });
    ui.add_space(12.0);
    egui::Frame::new().fill(acc.gamma_multiply(0.12)).stroke(Stroke::new(1.0, acc.gamma_multiply(0.6))).corner_radius(14).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let heading = ui.label(RichText::new("SUA PLACA").size(10.0).color(MUTED).strong());
        let name = if app.detected { app.gpu.name.as_str() } else { "Não detectada" };
        ui.label(RichText::new(name).size(13.0).strong().color(Color32::WHITE));
        let before = app.gpu.tier;
        egui::ComboBox::from_id_salt("tier").width(ui.available_width()).selected_text(app.gpu.tier.label()).show_ui(ui, |ui| {
            for t in Tier::ALL {
                ui.selectable_value(&mut app.gpu.tier, t, t.label());
            }
        }).response.labelled_by(heading.id);
        if app.gpu.tier != before {
            app.dlss5 = app.gpu.tier.is_rtx();
            app.mult = default_mult(app.gpu.tier);
            style(ctx, accent(app.gpu.tier));
        }
    });
    ui.add_space(14.0);
    ui.label(RichText::new("BIBLIOTECA").size(10.0).color(MUTED).strong());
    let mut stores: Vec<&'static str> = app.library.iter().map(|e| e.found.store).collect();
    stores.sort();
    stores.dedup();
    let modded = app.library.iter().filter(|e| e.state.is_some()).count();
    let mut nav = vec![(Filter::All, "Todos os jogos".to_string(), app.library.len(), acc), (Filter::Modded, "Modados".into(), modded, acc)];
    for s in stores {
        let label = if s == "Manual" { "Adicionados".to_string() } else { s.to_string() };
        nav.push((Filter::Store(s), label, app.library.iter().filter(|e| e.found.store == s).count(), store_color(s)));
    }
    for (f, label, count, color) in nav {
        let on = app.filter == f && app.view == View::Library;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        let p = ui.painter();
        if on || resp.hovered() {
            p.rect_filled(r, 10, if on { acc.gamma_multiply(0.16) } else { CARD });
        }
        if on {
            p.rect_filled(Rect::from_min_size(r.left_top() + vec2(0.0, 8.0), vec2(3.0, 18.0)), 2, acc);
        }
        p.circle_filled(pos2(r.left() + 16.0, r.center().y), 4.0, color);
        p.text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), if on { Color32::WHITE } else { Color32::from_gray(200) });
        p.text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, count.to_string(), FontId::proportional(12.0), MUTED);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.filter = f;
            app.view = View::Library;
        }
    }
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        let dirs: Vec<PathBuf> = app.library.iter().filter(|e| e.state.is_some()).filter_map(|e| e.dir.clone()).collect();
        let b = egui::Button::new(format!("⟳  Atualizar todos ({})", dirs.len())).min_size(vec2(ui.available_width(), 36.0));
        if ui.add_enabled(!app.busy() && !dirs.is_empty(), b).on_hover_text("Reinstala as escolhas salvas de cada jogo modado com as versões mais novas").clicked() {
            let gpu = app.gpu.clone();
            app.run(ctx, format!("Atualizando {} jogos", dirs.len()), move |log| install::update_all(&gpu, &dirs, log));
        }
        if app.scanning {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Procurando jogos…").size(11.0).color(MUTED));
            });
        } else if ui.add(egui::Button::new(RichText::new("Procurar jogos de novo").size(11.0)).frame(false)).clicked() {
            app.scanning = true;
            scan_library(app.tx.clone(), ctx.clone());
        }
    });
}

fn task_card(ctx: &egui::Context, task: &mut Task, acc: Color32) -> bool {
    let mut close = false;
    egui::Area::new(Id::unique("task")).anchor(Align2::RIGHT_BOTTOM, vec2(-20.0, -20.0)).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, acc.gamma_multiply(0.6))).corner_radius(16).inner_margin(14)
            .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(160) })
            .show(ui, |ui| {
                ui.set_width(400.0);
                ui.horizontal(|ui| {
                    match &task.done {
                        None => {
                            ui.spinner();
                        }
                        Some(Ok(())) => {
                            ui.label(RichText::new("✔").size(18.0).color(acc));
                        }
                        Some(Err(_)) => {
                            ui.label(RichText::new("✖").size(18.0).color(Color32::LIGHT_RED));
                        }
                    }
                    ui.label(RichText::new(&task.title).strong().color(Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if task.done.is_some() && ui.small_button("✕").clicked() {
                            close = true;
                        }
                    });
                });
                let take = if task.full_log { task.log.len() } else { 4 };
                egui::ScrollArea::vertical().max_height(if task.full_log { 260.0 } else { 90.0 }).stick_to_bottom(true).show(ui, |ui| {
                    for l in task.log.iter().rev().take(take).rev() {
                        let c = if l.starts_with("ERRO") { Color32::LIGHT_RED } else { Color32::from_gray(185) };
                        ui.label(RichText::new(l).size(11.5).color(c));
                    }
                });
                if ui.add(egui::Button::new(RichText::new(if task.full_log { "Ver menos" } else { "Ver log completo" }).size(11.0)).frame(false)).clicked() {
                    task.full_log = !task.full_log;
                }
            });
    });
    close
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

impl App {
    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.fade.is_none() {
            let px: Vec<Color32> = (0..256).map(|i| Color32::from_white_alpha(((i as f32 / 255.0).powf(1.6) * 255.0) as u8)).collect();
            self.fade = Some(ctx.load_texture("fade", egui::ColorImage::new([1, 256], px), egui::TextureOptions::LINEAR));
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Log(s) => {
                    if let Some(t) = &mut self.task {
                        t.log.push(s);
                    }
                }
                Msg::Done(r) => {
                    if let Some(t) = &mut self.task {
                        if let Err(e) = &r {
                            t.log.push(format!("ERRO: {e}"));
                        }
                        t.done = Some(r);
                    }
                    self.refresh_states();
                }
                Msg::Library(mut l) => {
                    // Keep art already fetched.
                    for e in &mut l {
                        if let Some(old) = self.library.iter().find(|o| o.found.dir == e.found.dir) {
                            e.art = old.art.clone();
                        }
                    }
                    fetch_art(l.iter().filter(|e| e.art.poster.is_none()).map(|e| e.found.clone()).collect(), self.tx.clone(), ctx.clone());
                    self.library = l;
                    self.scanning = false;
                    if let Some(p) = self.pending_open.take() {
                        self.open(&p, &ctx);
                    }
                }
                Msg::Art(key, a) => {
                    if let Some(e) = self.library.iter_mut().find(|e| e.found.dir == key) {
                        e.art = a;
                    }
                }
            }
        }
        let acc = accent(self.gpu.tier);
        let mut acts = Vec::new();

        // Dropped folders / executables become library games.
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            if !f.path().as_os_str().is_empty() {
                acts.push(Act::Added(f.path().to_path_buf()));
            }
        }

        egui::Panel::left("rail").exact_size(232.0).resizable(false)
            .frame(egui::Frame::new().fill(RAIL).inner_margin(egui::Margin::symmetric(14, 18)))
            .show(ui, |ui| rail(ui, self, acc, &ctx));

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| match self.view.clone() {
                View::Game(key) => game_page(ui, self, &key, acc, &mut acts),
                View::Library => library_page(ui, self, acc, &mut acts),
            });
        });

        // Drop target overlay.
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.content_rect();
            egui::Area::new(Id::unique("drop")).fixed_pos(screen.min).order(egui::Order::Foreground).show(&ctx, |ui| {
                let p = ui.painter();
                p.rect_filled(screen, 0, Color32::from_black_alpha(190));
                p.rect_stroke(screen.shrink(24.0), 24, Stroke::new(3.0, acc), StrokeKind::Inside);
                p.text(screen.center() - vec2(0.0, 20.0), Align2::CENTER_CENTER, "Solte a pasta ou o .exe do jogo", FontId::proportional(30.0), Color32::WHITE);
                p.text(screen.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, "ele entra na biblioteca e abre pronto para aplicar", FontId::proportional(15.0), MUTED);
            });
        }

        if let Some(t) = &mut self.task
            && task_card(&ctx, t, acc)
        {
            self.task = None;
        }

        for a in acts {
            self.handle(a, &ctx);
        }
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
        viewport: egui::ViewportBuilder::default().with_inner_size([1320.0, 840.0]).with_min_inner_size([980.0, 640.0]).with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Neural · FG — DLSS 5 + Frame Generation", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx, game)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ago_in_days() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(ago(now), "hoje");
        assert_eq!(ago(now - 86_400), "ontem");
        assert_eq!(ago(now - 5 * 86_400), "há 5 dias");
        assert_eq!(ago(now + 3600), "hoje", "clock skew does not underflow");
    }

    #[test]
    fn each_vendor_and_store_has_its_colour() {
        assert_ne!(accent(Tier::Rtx40), accent(Tier::Rdna4));
        assert_ne!(accent(Tier::Rdna4), accent(Tier::Intel));
        assert_eq!(accent(Tier::NvidiaOld), accent(Tier::Rtx50));
        let stores = ["Steam", "Epic", "GOG", "Ubisoft", "EA", "Xbox", "Rockstar", "Manual"];
        let mut colours: Vec<_> = stores.iter().map(|s| store_color(s).to_array()).collect();
        colours.sort();
        colours.dedup();
        assert_eq!(colours.len(), stores.len());
    }

    #[test]
    fn default_multiplier_is_always_available() {
        for t in Tier::ALL {
            assert!(plan::multipliers(t).contains(&default_mult(t)), "{t:?}");
        }
    }

    /// Renders the library and a game page offscreen into docs/ (`cargo test -- --ignored screenshots`).
    /// Needs internet for the cover art and a GPU for wgpu.
    #[test]
    #[ignore]
    fn screenshots() {
        let home = std::env::temp_dir().join(format!("gpumods-shots-{}", std::process::id()));
        let games = home.join("Games");
        let names = ["Cyberpunk 2077", "Hogwarts Legacy", "Forza Horizon 5", "Alan Wake 2", "ELDEN RING", "Black Myth Wukong",
            "Red Dead Redemption 2", "Baldur's Gate 3", "Indiana Jones and the Great Circle", "Starfield", "Hades II"];
        let list: Vec<_> = names.iter().map(|n| {
            std::fs::create_dir_all(games.join(n)).unwrap();
            std::fs::write(games.join(n).join("game.exe"), "x").unwrap();
            serde_json::json!({"name": n, "dir": games.join(n)})
        }).collect();
        // SAFETY: single-threaded at this point; only this test reads LOCALAPPDATA.
        unsafe { std::env::set_var("LOCALAPPDATA", &home) };
        std::fs::create_dir_all(home.join("GpuMods")).unwrap();
        std::fs::write(home.join("GpuMods/manual-games.json"), serde_json::to_string(&list).unwrap()).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        std::fs::write(games.join("Cyberpunk 2077/.gpu-mods-state.json"), serde_json::json!({
            "gpu": "NVIDIA RTX 40", "dlss5": true, "mult": 4, "when": now - 2 * 86_400,
            "versions": {"DLSS Unlocked": "NR-v0.9.80", "RenoDX MFG Unlock": "1.7.0", "ReShade": "6.8.0"}}).to_string()).unwrap();
        std::fs::write(games.join("Cyberpunk 2077/.gpu-mods-files.txt"), "").unwrap();

        let mut h = egui_kittest::Harness::builder().with_size(vec2(1320.0, 840.0)).build_ui_state(
            |ui, app: &mut Option<App>| {
                let a = app.get_or_insert_with(|| {
                    // Same symbol fallback eframe gives the real window.
                    ui.ctx().add_font_provider(std::sync::Arc::new(egui_system_font_provider::SystemFontProvider::new()));
                    let mut a = App::new(ui.ctx(), None);
                    a.gpu = Gpu { name: "NVIDIA GeForce RTX 4080 SUPER".into(), tier: Tier::Rtx40 };
                    a.detected = true;
                    a
                });
                a.draw(ui);
            },
            None,
        );
        // Background threads: library scan, cover downloads, image decoding.
        for _ in 0..120 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        h.render().unwrap().save(docs.join("screenshot.png")).unwrap();

        let key = games.join("Cyberpunk 2077");
        h.state_mut().as_mut().unwrap().select(&key);
        for _ in 0..30 {
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        h.render().unwrap().save(docs.join("screenshot-jogo.png")).unwrap();
    }

    #[test]
    fn generated_cover_text_and_colour() {
        assert_eq!(initials("Red Dead Redemption 2"), "RD");
        assert_eq!(initials("DOOM: The Dark Ages"), "DT");
        assert_eq!(initials("hades"), "H");
        assert_eq!(initials("—"), "");
        assert_eq!(name_hue("Cyberpunk 2077"), name_hue("Cyberpunk 2077"));
        assert!((0.0..1.0).contains(&name_hue("x")));
    }
}
