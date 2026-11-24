//! Dark theme and reusable UI components.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontData, FontDefinitions, FontFamily, FontId,
    Frame, Margin, Response, RichText, Rounding, Sense, Stroke, TextStyle, Ui,
};

pub const BG: Color32 = Color32::from_rgb(14, 17, 23);
pub const SIDEBAR: Color32 = Color32::from_rgb(18, 22, 31);
pub const CARD: Color32 = Color32::from_rgb(24, 29, 41);
pub const CARD_INNER: Color32 = Color32::from_rgb(30, 36, 50);
pub const CONSOLE: Color32 = Color32::from_rgb(10, 12, 17);
pub const BORDER: Color32 = Color32::from_rgb(38, 45, 61);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(58, 68, 92);
pub const TEXT: Color32 = Color32::from_rgb(230, 233, 239);
pub const MUTED: Color32 = Color32::from_rgb(139, 147, 167);
pub const ACCENT: Color32 = Color32::from_rgb(79, 140, 255);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(106, 159, 255);
pub const GREEN: Color32 = Color32::from_rgb(52, 199, 123);
pub const ORANGE: Color32 = Color32::from_rgb(245, 165, 36);
pub const RED: Color32 = Color32::from_rgb(240, 82, 90);

pub fn tint(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn bold_family() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn bold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(bold_family())
}

type FontFiles = Vec<(String, u32)>;

/// Uses the system font when available, egui's fonts as a fallback.
fn install_fonts(ctx: &egui::Context) {
    #[cfg(target_os = "windows")]
    let (regular, semibold): (FontFiles, FontFiles) = {
        let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        (
            vec![(format!("{dir}\\Fonts\\segoeui.ttf"), 0)],
            vec![
                (format!("{dir}\\Fonts\\seguisb.ttf"), 0),
                (format!("{dir}\\Fonts\\segoeuib.ttf"), 0),
            ],
        )
    };
    #[cfg(target_os = "macos")]
    let (regular, semibold): (FontFiles, FontFiles) = (
        vec![
            ("/System/Library/Fonts/SFNS.ttf".into(), 0),
            ("/System/Library/Fonts/HelveticaNeue.ttc".into(), 0),
        ],
        vec![("/System/Library/Fonts/HelveticaNeue.ttc".into(), 1)],
    );
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let (regular, semibold): (FontFiles, FontFiles) = (vec![], vec![]);

    let load = |list: &[(String, u32)]| {
        list.iter().find_map(|(path, index)| {
            std::fs::read(path).ok().map(|bytes| {
                let mut fd = FontData::from_owned(bytes);
                fd.index = *index;
                fd
            })
        })
    };

    let mut fonts = FontDefinitions::default();
    let defaults = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let has_regular = if let Some(fd) = load(&regular) {
        fonts.font_data.insert("ui".into(), fd);
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "ui".into());
        true
    } else {
        false
    };

    let mut bold_list = defaults;
    if let Some(fd) = load(&semibold) {
        fonts.font_data.insert("ui-bold".into(), fd);
        bold_list.insert(0, "ui-bold".into());
    } else if has_regular {
        bold_list.insert(0, "ui".into());
    }
    fonts.families.insert(bold_family(), bold_list);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = Color32::from_rgb(12, 15, 21);
    v.faint_bg_color = CARD_INNER;
    v.code_bg_color = CONSOLE;
    v.hyperlink_color = ACCENT_HOVER;
    v.selection.bg_fill = tint(ACCENT, 150);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0_f32, BORDER);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CARD;
    w.noninteractive.weak_bg_fill = CARD;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    w.inactive.bg_fill = CARD_INNER;
    w.inactive.weak_bg_fill = Color32::from_rgb(35, 42, 58);
    w.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    w.hovered.bg_fill = Color32::from_rgb(42, 50, 69);
    w.hovered.weak_bg_fill = Color32::from_rgb(42, 50, 69);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.hovered.expansion = 0.0;

    w.active.bg_fill = Color32::from_rgb(50, 59, 82);
    w.active.weak_bg_fill = Color32::from_rgb(50, 59, 82);
    w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    w.active.expansion = 0.0;

    w.open = w.active;
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.rounding = Rounding::same(8.0);
    }
    style.visuals = v;

    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.scroll.floating = true;

    style.text_styles = [
        (TextStyle::Heading, FontId::new(26.0, bold_family())),
        (TextStyle::Body, FontId::proportional(14.5)),
        (TextStyle::Button, FontId::proportional(14.5)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into();

    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let r = Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(14.0)
        .inner_margin(Margin::same(20.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner;
    ui.add_space(6.0);
    r
}

pub fn card_title(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    ui.label(bold(title).size(17.0).color(TEXT));
    if let Some(s) = subtitle {
        ui.add_space(-6.0);
        ui.label(RichText::new(s).color(MUTED));
    }
    ui.add_space(4.0);
}

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.heading(RichText::new(title).color(TEXT));
    ui.add_space(-4.0);
    ui.label(RichText::new(subtitle).size(15.0).color(MUTED));
    ui.add_space(14.0);
}

pub fn banner(ui: &mut Ui, color: Color32, icon: &str, title: &str, lines: &[String]) {
    Frame::none()
        .fill(tint(color, 30))
        .stroke(Stroke::new(1.0_f32, tint(color, 120)))
        .rounding(12.0)
        .inner_margin(Margin::symmetric(18.0, 14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                icon_circle(ui, 30.0, color, icon);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(bold(title).size(16.0).color(TEXT));
                    for l in lines {
                        ui.label(RichText::new(l).color(TEXT));
                    }
                });
            });
        });
    ui.add_space(6.0);
}

pub fn icon_circle(ui: &mut Ui, size: f32, color: Color32, icon: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        FontId::new(size * 0.5, bold_family()),
        Color32::WHITE,
    );
}

// Buttons: rectangles with a fill or outline. Badges: fully rounded, never clickable.
pub const BUTTON_ROUNDING: f32 = 8.0;

/// Status badge: information only, never clickable.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    Frame::none()
        .fill(tint(color, 50))
        .rounding(20.0)
        .inner_margin(Margin::symmetric(8.0, 2.0))
        .show(ui, |ui| {
            ui.label(
                bold(text)
                    .size(11.0)
                    .color(mix(color, Color32::WHITE, 0.45)),
            );
        });
}

pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(bold(text).size(15.0).color(Color32::WHITE))
        .fill(if enabled { ACCENT } else { tint(ACCENT, 70) })
        .stroke(Stroke::NONE)
        .rounding(BUTTON_ROUNDING)
        .min_size(vec2(180.0, 42.0));
    ui.add_enabled(enabled, btn)
        .on_hover_cursor(CursorIcon::PointingHand)
}

pub fn danger_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(bold(text).size(14.5).color(Color32::WHITE))
        .fill(if enabled { RED } else { tint(RED, 60) })
        .stroke(Stroke::NONE)
        .rounding(BUTTON_ROUNDING)
        .min_size(vec2(150.0, 38.0));
    ui.add_enabled(enabled, btn)
        .on_hover_cursor(CursorIcon::PointingHand)
}

pub fn secondary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let btn = egui::Button::new(RichText::new(text).size(14.5).color(TEXT))
        .fill(CARD_INNER)
        .stroke(Stroke::new(1.0_f32, BORDER_STRONG))
        .rounding(BUTTON_ROUNDING)
        .min_size(vec2(120.0, 38.0));
    ui.add_enabled(enabled, btn)
        .on_hover_cursor(CursorIcon::PointingHand)
}

/// Compact outlined button (status bar, log toolbar).
pub fn small_button(ui: &mut Ui, text: &str, color: Color32, outline: Color32) -> Response {
    let btn = egui::Button::new(RichText::new(text).size(13.0).color(color))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0_f32, outline))
        .rounding(BUTTON_ROUNDING)
        .min_size(vec2(0.0, 28.0));
    ui.add(btn).on_hover_cursor(CursorIcon::PointingHand)
}

/// Text field with a label above it.
pub fn field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    width: f32,
    password: bool,
    hint: &str,
) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        ui.label(RichText::new(label).size(12.5).color(MUTED));
        ui.add(
            egui::TextEdit::singleline(value)
                .password(password)
                .hint_text(hint)
                .margin(vec2(10.0, 8.0))
                .desired_width(width),
        )
    })
    .inner
}

/// Animated on/off switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(42.0, 24.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let r = rect.height() / 2.0;
    let p = ui.painter();
    p.rect_filled(rect, r, mix(BORDER_STRONG, ACCENT, t));
    let x = egui::lerp((rect.left() + r)..=(rect.right() - r), t);
    p.circle_filled(pos2(x, rect.center().y), r - 3.5, Color32::WHITE);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn toggle_row(ui: &mut Ui, on: &mut bool, title: &str, subtitle: &str) -> Response {
    ui.horizontal(|ui| {
        let resp = toggle(ui, on);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).color(TEXT));
            if !subtitle.is_empty() {
                ui.label(RichText::new(subtitle).size(12.5).color(MUTED));
            }
        });
        resp
    })
    .inner
}

#[derive(Clone, Copy, PartialEq)]
pub enum RowState {
    Pending,
    Running,
    Done,
    Warn,
    Error,
}

pub fn status_row(ui: &mut Ui, state: RowState, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        let size = 24.0;
        match state {
            RowState::Running => {
                ui.add_sized([size, size], egui::Spinner::new().size(18.0).color(ACCENT));
            }
            RowState::Pending => {
                let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
                ui.painter().circle_stroke(
                    rect.center(),
                    size / 2.0 - 1.0,
                    Stroke::new(1.5_f32, BORDER_STRONG),
                );
            }
            RowState::Done => icon_circle(ui, size, GREEN, "✔"),
            RowState::Warn => icon_circle(ui, size, ORANGE, "!"),
            RowState::Error => icon_circle(ui, size, RED, "✖"),
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let c = if state == RowState::Pending {
                MUTED
            } else {
                TEXT
            };
            ui.label(RichText::new(title).color(c));
            if !detail.is_empty() {
                ui.label(RichText::new(detail).size(12.5).color(MUTED));
            }
        });
    });
}

pub fn stat_tile(ui: &mut Ui, label: &str, value: &str, color: Color32) {
    Frame::none()
        .fill(CARD_INNER)
        .rounding(10.0)
        .inner_margin(Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(label.to_uppercase()).size(11.0).color(MUTED));
            ui.label(bold(value).size(16.0).color(color));
        });
}

/// Segmented control.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) {
    Frame::none()
        .fill(CARD_INNER)
        .rounding(10.0)
        .inner_margin(Margin::same(4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (v, label) in options {
                    let selected = *value == *v;
                    let btn = egui::Button::new(RichText::new(*label).color(if selected {
                        Color32::WHITE
                    } else {
                        MUTED
                    }))
                    .fill(if selected {
                        ACCENT
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .rounding(8.0)
                    .min_size(vec2(130.0, 32.0));
                    if ui
                        .add(btn)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        *value = *v;
                    }
                }
            });
        });
}

#[derive(Clone, Copy, PartialEq)]
pub enum StepState {
    Pending,
    Done,
    Warn,
    Error,
    /// Not reachable yet: the previous step is not done.
    Locked,
}

const LOCKED_TEXT: Color32 = Color32::from_rgb(92, 100, 120);

/// Sidebar navigation entry. A locked entry cannot be clicked.
pub fn nav_item(
    ui: &mut Ui,
    badge: &str,
    title: &str,
    subtitle: &str,
    state: StepState,
    selected: bool,
) -> Response {
    let locked = state == StepState::Locked;
    let sense = if locked {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), sense);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 10.0, tint(ACCENT, 34));
    } else if resp.hovered() && !locked {
        p.rect_filled(rect, 10.0, CARD);
    }

    let c = pos2(rect.left() + 24.0, rect.center().y);
    let r = 13.0;
    let (fill, glyph, glyph_color) = match state {
        StepState::Done => (Some(GREEN), "✔", Color32::WHITE),
        StepState::Warn => (Some(ORANGE), "!", Color32::WHITE),
        StepState::Error => (Some(RED), "✖", Color32::WHITE),
        StepState::Pending if selected => (Some(ACCENT), badge, Color32::WHITE),
        StepState::Pending => (None, badge, MUTED),
        StepState::Locked => (None, "", LOCKED_TEXT),
    };
    match fill {
        Some(f) => {
            p.circle_filled(c, r, f);
        }
        None => {
            let ring = if locked { BORDER } else { BORDER_STRONG };
            p.circle_stroke(c, r, Stroke::new(1.5_f32, ring));
        }
    }
    if locked {
        draw_lock(p, c, glyph_color);
    } else {
        p.text(
            c,
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(12.5, bold_family()),
            glyph_color,
        );
    }

    let x = rect.left() + 48.0;
    p.text(
        pos2(x, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(14.5, bold_family()),
        if selected {
            Color32::WHITE
        } else if locked {
            LOCKED_TEXT
        } else {
            TEXT
        },
    );
    p.text(
        pos2(x, rect.center().y + 10.0),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(12.0),
        if locked { LOCKED_TEXT } else { MUTED },
    );
    if locked {
        resp.on_hover_cursor(CursorIcon::NotAllowed)
    } else {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    }
}

/// A padlock drawn with shapes (the emoji glyph renders badly).
fn draw_lock(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let body = egui::Rect::from_center_size(pos2(c.x, c.y + 2.5), vec2(10.0, 7.5));
    p.rect_filled(body, 1.5, color);
    let (r, top) = (3.0_f32, c.y - 2.0);
    let mut points = vec![pos2(c.x - r, body.top())];
    for i in 0..=12 {
        let a = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
        points.push(pos2(c.x + r * a.cos(), top + r * a.sin()));
    }
    points.push(pos2(c.x + r, body.top()));
    p.add(egui::Shape::line(points, Stroke::new(1.6_f32, color)));
}

pub fn logo(ui: &mut Ui) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(bold("Android Auto Installer").size(17.0).color(TEXT));
        ui.label(
            RichText::new("BMW / MINI EntryNav2 · unofficial")
                .size(12.0)
                .color(MUTED),
        );
    });
}

pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(bold(text).size(11.0).color(MUTED));
    ui.add_space(-4.0);
}
