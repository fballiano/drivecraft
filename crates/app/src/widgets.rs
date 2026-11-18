//! Small widgets in the Studio style.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Frame, Id, InnerResponse, Margin, Rect,
    Response, RichText, Sense, Stroke, Ui, pos2, vec2,
};

use eframe::egui::text::{LayoutJob, TextFormat};

use crate::theme::{RADIUS, RADIUS_LG, Tokens, icon_font, medium, regular, semibold};

/// Text that can contain Lucide icons. Icon characters (Unicode private use
/// area) use the icon font. All other characters use `font`.
pub fn mixed(text: &str, font: egui::FontId, color: Color32) -> LayoutJob {
    let is_icon = |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c);
    let icon = icon_font(font.size + 1.0);
    let mut job = LayoutJob::default();
    let mut run = String::new();
    let mut run_is_icon = false;
    let flush = |job: &mut LayoutJob, run: &mut String, run_is_icon: bool| {
        if !run.is_empty() {
            let font_id = if run_is_icon {
                icon.clone()
            } else {
                font.clone()
            };
            job.append(
                run,
                0.0,
                TextFormat {
                    font_id,
                    color,
                    valign: egui::Align::Center,
                    ..Default::default()
                },
            );
            run.clear();
        }
    };
    for c in text.chars() {
        if is_icon(c) != run_is_icon {
            flush(&mut job, &mut run, run_is_icon);
            run_is_icon = is_icon(c);
        }
        run.push(c);
    }
    flush(&mut job, &mut run, run_is_icon);
    job
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::of(ui);
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
}

/// A card with only a thin padding, for lists with full-width rows.
pub fn list_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    let t = Tokens::of(ui);
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
}

/// The title row of a card: an icon, a title, and an optional subtitle.
pub fn card_title(ui: &mut Ui, icon: &str, title: &str, subtitle: Option<&str>) {
    let t = Tokens::of(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).font(icon_font(16.0)).color(t.icon));
        ui.label(RichText::new(title).font(semibold(14.0)).color(t.text));
    });
    if let Some(sub) = subtitle {
        ui.label(RichText::new(sub).font(regular(12.5)).color(t.text_dim));
    }
}

fn button(text: &str, fg: Color32, fill: Color32, stroke: Stroke) -> egui::Button<'static> {
    egui::Button::new(mixed(text, medium(13.0), fg))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS))
        .min_size(vec2(0.0, 32.0))
}

pub fn primary(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let t = Tokens::of(ui);
    ui.add_enabled(
        enabled,
        button(text, t.primary_text, t.primary_bg, Stroke::NONE),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

pub fn secondary(ui: &mut Ui, enabled: bool, text: &str) -> Response {
    let t = Tokens::of(ui);
    ui.add_enabled(
        enabled,
        button(text, t.text, t.field, Stroke::new(1.0, t.field_border)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// A frameless icon button.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    let t = Tokens::of(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tooltip));
    let bg = if resp.is_pointer_button_down_on() {
        t.pressed
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(RADIUS), bg);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        icon_font(16.0),
        t.icon,
    );
    resp.on_hover_text(tooltip)
        .on_hover_cursor(CursorIcon::PointingHand)
}

/// A link-style text button.
pub fn link(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::of(ui);
    ui.add(
        egui::Label::new(RichText::new(text).font(medium(12.5)).color(t.accent_text))
            .sense(Sense::click()),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

pub fn nav_item(ui: &mut Ui, icon: &str, label: &str, selected: bool) -> Response {
    let t = Tokens::of(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let bg = if selected {
        t.accent_soft
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(RADIUS), bg);
    let (icon_color, text_color) = if selected {
        (t.accent, t.accent_text)
    } else {
        (t.icon, t.text_dim)
    };
    p.text(
        rect.left_center() + vec2(14.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        icon_font(16.0),
        icon_color,
    );
    p.text(
        rect.left_center() + vec2(42.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        medium(13.5),
        text_color,
    );
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Paints an on/off switch in `rect` (34 x 20 points) and returns its response.
pub fn toggle_at(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    on: bool,
    enabled: bool,
    label: &str,
) -> Response {
    let t = Tokens::of(ui);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, id, sense);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, enabled, on, label));
    let how_on = ui.ctx().animate_bool_responsive(id, on);
    let off_bg = if t.dark {
        t.field_border
    } else {
        Color32::from_rgb(200, 200, 208)
    };
    let mut bg = lerp_color(off_bg, t.accent, how_on);
    if !enabled {
        bg = bg.gamma_multiply(0.45);
    }
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(10), bg);
    let x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), how_on);
    p.circle_filled(pos2(x, rect.center().y), 8.0, Color32::WHITE);
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn lerp_color(a: Color32, b: Color32, f: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

/// Paints a pill that ends at `right` on the center line `y`. Returns its width.
pub fn paint_pill(ui: &Ui, right: f32, y: f32, text: &str, fg: Color32, bg: Color32) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), medium(11.5), fg);
    let size = galley.size() + vec2(18.0, 6.0);
    let rect = Rect::from_min_size(pos2(right - size.x, y - size.y / 2.0), size);
    ui.painter().rect_filled(rect, CornerRadius::same(255), bg);
    ui.painter().galley(rect.min + vec2(9.0, 3.0), galley, fg);
    size.x
}

/// An icon in a filled circle.
pub fn icon_circle(ui: &mut Ui, icon: &str, diameter: f32, fg: Color32, bg: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, bg);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        icon_font(diameter * 0.46),
        fg,
    );
    resp
}

/// A thin progress bar. `None` shows a moving bar for unknown progress.
pub fn progress(ui: &mut Ui, fraction: Option<f32>) {
    let t = Tokens::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        CornerRadius::same(3),
        if t.dark { t.field_border } else { t.pressed },
    );
    let fill = match fraction {
        Some(f) => Rect::from_min_size(
            rect.min,
            vec2(rect.width() * f.clamp(0.0, 1.0), rect.height()),
        ),
        None => {
            let time = ui.input(|i| i.time) as f32;
            let w = rect.width() * 0.3;
            let x = rect.left() - w + (time * 0.6).fract() * (rect.width() + w);
            ui.ctx().request_repaint();
            Rect::from_min_max(
                pos2(x.max(rect.left()), rect.top()),
                pos2((x + w).min(rect.right()), rect.bottom()),
            )
        }
    };
    if fill.width() > 0.0 {
        p.rect_filled(fill, CornerRadius::same(3), t.accent);
    }
}

/// A segmented control. Returns the index that the user clicked.
pub fn segmented(ui: &mut Ui, items: &[(&str, &str)], selected: usize) -> Option<usize> {
    let t = Tokens::of(ui);
    let mut clicked = None;
    let galleys: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(i, (icon, label))| {
            let color = if i == selected { t.text } else { t.text_dim };
            ui.painter()
                .layout_job(mixed(&format!("{icon}  {label}"), medium(12.5), color))
        })
        .collect();
    let gap = 2.0;
    let inner: f32 =
        galleys.iter().map(|g| g.size().x + 24.0).sum::<f32>() + gap * (items.len() - 1) as f32;
    // A fixed-size, left-to-right area, also inside a right-to-left row.
    let size = vec2(inner + 8.0, 36.0);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            Frame::new()
                .fill(t.field)
                .stroke(Stroke::new(1.0, t.field_border))
                .corner_radius(CornerRadius::same(RADIUS))
                .inner_margin(Margin::same(3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    {
                        for (i, ((_, label), galley)) in items.iter().zip(galleys).enumerate() {
                            let on = i == selected;
                            let color = if on { t.text } else { t.text_dim };
                            let size = vec2(galley.size().x + 24.0, 28.0);
                            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                            resp.widget_info(|| {
                                egui::WidgetInfo::selected(egui::Role::Button, true, on, *label)
                            });
                            let bg = if on {
                                t.card
                            } else if resp.hovered() {
                                t.hover
                            } else {
                                Color32::TRANSPARENT
                            };
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(RADIUS - 2), bg);
                            if on {
                                ui.painter().rect_stroke(
                                    rect,
                                    CornerRadius::same(RADIUS - 2),
                                    Stroke::new(1.0, t.card_border),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let pos = rect.center() - galley.size() / 2.0;
                            ui.painter().galley(pos, galley, color);
                            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                clicked = Some(i);
                            }
                        }
                    }
                });
        },
    );
    clicked
}

/// The title and subtitle of a page, with optional actions on the right.
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    let t = Tokens::of(ui);
    text_and_actions(
        ui,
        egui::Align::Min,
        |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(title).font(semibold(22.0)).color(t.text));
            ui.label(
                RichText::new(subtitle)
                    .font(regular(13.0))
                    .color(t.text_dim),
            );
        },
        |ui, ()| actions(ui),
    );
    ui.add_space(14.0);
}

/// A row with text on the left and actions (buttons) on the right. The text
/// gets only the width that the actions leave, so it wraps or truncates and
/// never runs under them. The actions get a rectangle with the height of the
/// text; `align` places them in it (`Center`: centered on the text, `Min`: at
/// the top, for example next to a title).
///
/// The text comes first, so that an action can use what the text returns
/// (for example the id of a label). The width of the actions comes from the
/// previous frame; when it changes, egui draws one more frame.
pub fn text_and_actions<R>(
    ui: &mut Ui,
    align: egui::Align,
    text: impl FnOnce(&mut Ui) -> R,
    actions: impl FnOnce(&mut Ui, R),
) {
    const GAP: f32 = 16.0;
    let id = ui.auto_id_with("actions-width");
    let reserved: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let right = ui.available_rect_before_wrap().right();
    let width = (ui.available_width() - reserved - GAP).max(80.0);
    let text = ui.allocate_ui_with_layout(
        vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            text(ui)
        },
    );
    let text_rect = text.response.rect;
    let area = egui::Rect::from_min_max(
        pos2(text_rect.right() + GAP, text_rect.top()),
        pos2(right, text_rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::right_to_left(align)),
    );
    actions(&mut child, text.inner);
    let used = child.min_rect();
    // The actions can be taller than the text: keep the next widget below.
    ui.advance_cursor_after_rect(text_rect.union(used));
    if (used.width() - reserved).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, used.width()));
        ui.ctx().request_repaint();
    }
}

/// A label for a setting, with a description under it.
/// Returns the id of the title label, so that controls can name themselves with it.
pub fn setting_label(ui: &mut Ui, title: &str, description: &str) -> Id {
    let t = Tokens::of(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let id = ui
            .label(RichText::new(title).font(medium(13.0)).color(t.text))
            .id;
        ui.label(
            RichText::new(description)
                .font(regular(12.0))
                .color(t.text_faint),
        );
        id
    })
    .inner
}

pub fn separator(ui: &mut Ui) {
    let t = Tokens::of(ui);
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, t.separator);
    ui.add_space(6.0);
}
