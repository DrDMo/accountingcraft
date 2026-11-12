//! Look and feel: palette, fonts, and the few building blocks every screen uses
//! (page title, card, primary button, nav item, stat tile, table header).

use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Theme, Vec2};

/// Teal from the abacus icon, deep enough for white text on it.
pub const ACCENT: Color32 = Color32::from_rgb(17, 122, 122);
pub const MONEY_IN: Color32 = Color32::from_rgb(34, 150, 70);
pub const MONEY_OUT: Color32 = Color32::from_rgb(214, 60, 55);

pub struct Palette {
    pub bg: Color32,
    pub side: Color32,
    pub card: Color32,
    pub border: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub hover: Color32,
    pub field: Color32,
    pub stripe: Color32,
}

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(244, 246, 248),
    side: Color32::from_rgb(255, 255, 255),
    card: Color32::from_rgb(255, 255, 255),
    border: Color32::from_rgb(223, 227, 232),
    text: Color32::from_rgb(31, 35, 40),
    weak: Color32::from_rgb(104, 112, 122),
    hover: Color32::from_rgb(236, 242, 243),
    field: Color32::from_rgb(248, 250, 251),
    stripe: Color32::from_rgb(248, 250, 251),
};

const DARK: Palette = Palette {
    bg: Color32::from_rgb(21, 23, 27),
    side: Color32::from_rgb(27, 30, 35),
    card: Color32::from_rgb(32, 36, 42),
    border: Color32::from_rgb(46, 51, 58),
    text: Color32::from_rgb(230, 232, 235),
    weak: Color32::from_rgb(154, 163, 174),
    hover: Color32::from_rgb(42, 47, 54),
    field: Color32::from_rgb(24, 27, 32),
    stripe: Color32::from_rgb(36, 40, 47),
};

pub fn pal(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode { &DARK } else { &LIGHT }
}

/// The palette for whichever theme is showing, when there's no `Ui` at hand (floating windows).
pub fn pal_ctx(ctx: &egui::Context) -> &'static Palette {
    if ctx.theme() == Theme::Dark { &DARK } else { &LIGHT }
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn install(ctx: &egui::Context) {
    fonts(ctx);
    for (theme, p) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        ctx.style_mut_of(theme, |s| style(s, p));
    }
}

/// Inter (the Craft suite's font) when the build had craft-fonts; else Segoe UI when present
/// (every Windows install); else egui's own font. The "semibold" family always exists.
fn fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    let base = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut semi = base.clone();
    let dir = std::path::Path::new("C:\\Windows\\Fonts");
    let (regular, semibold) = match crate::pdf::INTER {
        Some((r, b)) => (Some(r.to_vec()), Some(b.to_vec())),
        None => (std::fs::read(dir.join("segoeui.ttf")).ok(), std::fs::read(dir.join("seguisb.ttf")).ok()),
    };
    if let Some(b) = regular {
        defs.font_data.insert("ui".into(), egui::FontData::from_owned(b).into());
        defs.families.entry(FontFamily::Proportional).or_default().insert(0, "ui".into());
        semi.insert(0, "ui".into());
    }
    if let Some(b) = semibold {
        defs.font_data.insert("ui-semibold".into(), egui::FontData::from_owned(b).into());
        semi.insert(0, "ui-semibold".into());
    }
    defs.families.insert(FontFamily::Name("semibold".into()), semi);
    ctx.set_fonts(defs);
}

fn style(s: &mut egui::Style, p: &Palette) {
    use egui::TextStyle::*;
    s.text_styles = [
        (Heading, semibold(20.0)),
        (Body, FontId::proportional(15.0)),
        (Button, FontId::proportional(15.0)),
        (Small, FontId::proportional(12.5)),
        (Monospace, FontId::monospace(14.0)),
    ]
    .into();
    let sp = &mut s.spacing;
    sp.item_spacing = egui::vec2(10.0, 10.0);
    sp.button_padding = egui::vec2(14.0, 6.0);
    sp.interact_size.y = 34.0;
    sp.text_edit_width = 260.0;
    sp.combo_width = 240.0;

    let v = &mut s.visuals;
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = 12.0.into();
    v.menu_corner_radius = 10.0.into();
    v.extreme_bg_color = p.field;
    v.faint_bg_color = p.stripe;
    v.code_bg_color = p.field;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    let w = &mut v.widgets;
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.bg_fill = p.card;
    for (wv, fill, stroke) in [
        (&mut w.inactive, p.field, p.border),
        (&mut w.hovered, p.hover, ACCENT),
        (&mut w.active, p.hover, ACCENT),
        (&mut w.open, p.hover, ACCENT),
    ] {
        wv.bg_fill = fill;
        wv.weak_bg_fill = fill;
        wv.bg_stroke = Stroke::new(1.0, stroke);
        wv.fg_stroke = Stroke::new(1.0, p.text);
        wv.expansion = 0.0;
    }
    for wv in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.corner_radius = 8.0.into();
    }
}

/// Screen title with an optional one-line explanation under it.
pub fn page(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(semibold(26.0)));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).color(pal(ui).weak));
    }
    ui.add_space(14.0);
}

/// A white (or dark) rounded panel spanning the width, with an optional title.
pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = pal(ui);
    let r = egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(12.0)
        .inner_margin(20.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).font(semibold(17.0)));
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(16.0);
    r
}

/// The main action on a screen: filled accent button with white text.
pub fn primary(ui: &mut egui::Ui, text: &str) -> bool {
    let b = egui::Button::new(RichText::new(text).font(semibold(15.0)).color(Color32::WHITE)).fill(ACCENT).stroke(Stroke::NONE);
    ui.add(b).clicked()
}

/// Muted column header for tables.
pub fn th(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(semibold(13.0)).color(pal(ui).weak));
}

/// Muted form label.
pub fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(pal(ui).weak));
}

/// Full-width sidebar entry; the current screen is filled with the accent color.
pub fn nav_item(ui: &mut egui::Ui, text: &str, selected: bool, indent: f32) -> bool {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), egui::Sense::click());
    let (bg, fg) = match (selected, resp.hovered()) {
        (true, _) => (ACCENT, Color32::WHITE),
        (false, true) => (p.hover, p.text),
        _ => (Color32::TRANSPARENT, p.text),
    };
    ui.painter().rect_filled(rect, 8.0, bg);
    let font = if selected { semibold(15.0) } else { FontId::proportional(15.0) };
    ui.painter().text(rect.left_center() + Vec2::new(14.0 + indent, 0.0), egui::Align2::LEFT_CENTER, text, font, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A collapsible sidebar group heading with a chevron. `active` marks a closed group that holds the
/// current screen. Returns true when clicked.
pub fn nav_group(ui: &mut egui::Ui, text: &str, open: bool, active: bool) -> bool {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, p.hover);
    }
    let fg = if active { ACCENT } else { p.weak };
    let c = rect.left_center() + Vec2::new(18.0, 0.0);
    // Chevron: right when closed, down when open.
    let pts = if open {
        vec![c + Vec2::new(-4.0, -2.0), c + Vec2::new(4.0, -2.0), c + Vec2::new(0.0, 3.0)]
    } else {
        vec![c + Vec2::new(-2.0, -4.0), c + Vec2::new(-2.0, 4.0), c + Vec2::new(3.0, 0.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, fg, Stroke::NONE));
    ui.painter().text(rect.left_center() + Vec2::new(32.0, 0.0), egui::Align2::LEFT_CENTER, text.to_uppercase(), semibold(12.5), fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Dashboard tiles laid out in as many equal columns as fit, so they never run off the window.
pub fn stat_tiles(ui: &mut egui::Ui, tiles: &[(&str, String, String)]) {
    const MIN_W: f32 = 170.0;
    const GAP: f32 = 16.0;
    let avail = ui.available_width();
    let fit = (((avail + GAP) / (MIN_W + GAP)).floor() as usize).clamp(1, tiles.len().max(1));
    // Balance rows (5 tiles at 4 per row become 3 + 2, not 4 + 1).
    let rows = tiles.len().div_ceil(fit);
    let n = tiles.len().div_ceil(rows.max(1)).max(1);
    let w = (avail - GAP * (n as f32 - 1.0)) / n as f32;
    let p = pal(ui);
    for row in tiles.chunks(n) {
        // Top-aligned so tiles line up exactly instead of stepping down.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (title, value, note) in row {
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(12.0)
                    .inner_margin(18.0)
                    .show(ui, |ui| {
                        // Fixed outer size: frame margins (2 × 18) and stroke (2 × 1) come off the content.
                        ui.set_width(w - 38.0);
                        ui.set_height(78.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 4.0;
                            ui.label(RichText::new(*title).color(p.weak));
                            ui.label(RichText::new(value).font(semibold(28.0)));
                            ui.label(RichText::new(if note.is_empty() { " " } else { note }).small().color(p.weak));
                        });
                    });
            }
        });
        ui.add_space(GAP - ui.spacing().item_spacing.y);
    }
}
