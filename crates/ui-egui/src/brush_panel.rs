//! Window › Brush Settings (F5) and Window › Brushes: Photoshop's floating brush panel.
//!
//! Brush Settings: the section list with enable boxes on the left (clicking a name shows and turns
//! on that section), the selected section's controls on the right (see [`crate::brush_sections`]),
//! and a live stroke preview strip below, rendered with the real brush engine and cached on the
//! settings (it re-renders only when they change). Brushes: the presets in collapsible groups with
//! tip thumbnails and stroke previews, a size slider and a search field.
//!
//! The panel never writes the session brush itself: each frame's edits become one
//! `tools.setBrush` call carrying only the changed fields (Rule 1), so the journal, the control
//! channel and the MCP server see exactly what the panel did and can do the same.

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, vec2};
use photocraft_engine::BrushSettings;
use photocraft_engine::paint::{self, TipShape};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::brush_preview;
use crate::theme::{self, Tokens};
use crate::{PhotocraftApp, icons, widgets};

pub use crate::brush_preview::{preview_pixels, preview_sig};
pub use crate::brush_sections::section_body;

/// Sections in Photoshop's order. The bool says whether the section has an enable box.
pub const SECTIONS: [(&str, bool); 13] = [
    ("Brush Tip Shape", false),
    ("Shape Dynamics", true),
    ("Scattering", true),
    ("Texture", true),
    ("Dual Brush", true),
    ("Color Dynamics", true),
    ("Transfer", true),
    ("Brush Pose", true),
    ("Noise", true),
    ("Wet Edges", true),
    ("Build-up", true),
    ("Smoothing", false),
    ("Protect Texture", true),
];

/// Panel width and the stroke strip's size.
const WIDTH: f32 = 540.0;
const STRIP: (u32, u32) = (488, 76);

/// Brushes panel view state (serde, so the control channel can read and drive it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BrushesPanelState {
    /// Names of collapsed groups.
    pub collapsed: Vec<String>,
    /// Case-insensitive name filter.
    pub filter: String,
}

/// The enable flag behind section `i` (None for Brush Tip Shape and Smoothing).
pub fn section_flag(b: &mut BrushSettings, i: usize) -> Option<&mut bool> {
    Some(match i {
        1 => &mut b.shape_dynamics.enabled,
        2 => &mut b.scattering.enabled,
        3 => &mut b.texture.enabled,
        4 => &mut b.dual_brush.enabled,
        5 => &mut b.color_dynamics.enabled,
        6 => &mut b.transfer.enabled,
        7 => &mut b.pose.enabled,
        8 => &mut b.noise,
        9 => &mut b.wet_edges,
        10 => &mut b.build_up,
        12 => &mut b.protect_texture,
        _ => return None,
    })
}

/// The brush as JSON with the bitmaps (sampled tips, pattern tiles) that `skip` names replaced by
/// placeholders: they can be megabytes of base64, and skipped ones are equal on both sides.
fn light_json(b: &BrushSettings, skip: (bool, bool, bool)) -> Value {
    let mut c = b.clone();
    if skip.0 {
        c.tip = TipShape::Round;
    }
    if skip.1 {
        c.dual_brush.tip = TipShape::Round;
    }
    if skip.2 {
        c.texture.pattern = paint::Pattern::default();
    }
    serde_json::to_value(&c).unwrap_or(Value::Null)
}

/// Fields of `new` that differ from `old`, nested objects diffed key by key: a minimal
/// `tools.setBrush` patch (`{}` when nothing changed).
pub fn brush_patch(old: &BrushSettings, new: &BrushSettings) -> Value {
    let skip = (old.tip == new.tip, old.dual_brush.tip == new.dual_brush.tip, old.texture.pattern == new.texture.pattern);
    fn diff(a: &Value, b: &Value) -> Option<Value> {
        match (a, b) {
            (Value::Object(ao), Value::Object(bo)) => {
                let mut out = Map::new();
                for (k, bv) in bo {
                    match ao.get(k) {
                        Some(av) => {
                            if let Some(d) = diff(av, bv) {
                                out.insert(k.clone(), d);
                            }
                        }
                        None => {
                            out.insert(k.clone(), bv.clone());
                        }
                    }
                }
                (!out.is_empty()).then_some(Value::Object(out))
            }
            // Enums with payloads (tips, patterns) are replaced whole.
            _ => (a != b).then(|| b.clone()),
        }
    }
    diff(&light_json(old, skip), &light_json(new, skip)).unwrap_or_else(|| json!({}))
}

/// Send the panel's edits (`before` → `after`) through `tools.setBrush`.
pub fn commit(app: &mut PhotocraftApp, before: &BrushSettings, after: &BrushSettings) {
    if before == after {
        return;
    }
    let patch = brush_patch(before, after);
    if patch.as_object().is_some_and(Map::is_empty) {
        return;
    }
    if let Err(e) = app.run("tools.setBrush", json!({ "brush": patch })) {
        app.ui.status = e;
    }
}

/// Group label for presets saved without a group.
pub const UNGROUPED: &str = "My Brushes";

/// Preset indices in panel order: groups in order of first appearance.
pub fn grouped_presets(presets: &[paint::BrushPreset]) -> Vec<(String, Vec<usize>)> {
    let mut out: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, p) in presets.iter().enumerate() {
        let g = if p.group.is_empty() { UNGROUPED } else { p.group.as_str() };
        match out.iter_mut().find(|(n, _)| n == g) {
            Some((_, v)) => v.push(i),
            None => out.push((g.to_string(), vec![i])),
        }
    }
    out
}

/// Does the current brush match `preset` (everything but colour and size)? Cheap fields first:
/// the full comparison clones the preset and its tip.
pub fn is_current(preset: &BrushSettings, brush: &BrushSettings) -> bool {
    preset.hardness == brush.hardness
        && preset.spacing == brush.spacing
        && preset.tip == brush.tip
        && BrushSettings { color: brush.color, size: brush.size, background: brush.background, ..preset.clone() } == *brush
}

/// A fresh "Brush N" name.
fn new_preset_name(presets: &[paint::BrushPreset]) -> String {
    (1..).map(|n| format!("Brush {n}")).find(|n| !presets.iter().any(|p| &p.name == n)).unwrap_or_else(|| "Brush".into())
}

fn run_or_status(app: &mut PhotocraftApp, id: &str, p: Value) {
    if let Err(e) = app.run(id, p) {
        app.ui.status = e;
    }
}

fn full_uv() -> egui::Rect {
    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0))
}

fn brushes_tab(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // Size of the current brush (Photoshop's Brushes panel slider).
    let before = app.session.tools.brush.clone();
    let mut b = before.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::ts("Size")).color(t.text_dim));
        let mut lv = b.size.max(1.0).ln();
        ui.add_sized(vec2(WIDTH - 140.0, 18.0), |ui: &mut egui::Ui| {
            let r = widgets::slider(ui, &mut lv, 0.0..=5000f32.ln(), None);
            if r.changed() {
                b.size = lv.exp().round().clamp(1.0, 5000.0);
            }
            r
        });
        let mut s = b.size;
        if widgets::value_field(ui, &mut s, 1.0..=5000.0, "px", 74.0).changed() {
            b.size = s.round().clamp(1.0, 5000.0);
        }
    });
    commit(app, &before, &b);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        icons::paint(ui, egui::Rect::from_min_size(ui.cursor().min + vec2(0.0, 3.0), vec2(16.0, 16.0)), "search", 14.0, t.text_faint);
        ui.add_space(20.0);
        ui.add(egui::TextEdit::singleline(&mut app.ui.brushes_panel.filter).hint_text(crate::i18n::ts("Search Brushes")).desired_width(WIDTH - 60.0));
    });
    ui.add_space(6.0);
    let filter = app.ui.brushes_panel.filter.trim().to_lowercase();
    let groups = grouped_presets(&app.session.tools.presets);
    let brush = &app.session.tools.brush;
    let mut clicked = None;
    let mut toggle = None;
    egui::ScrollArea::vertical().id_salt("brush-presets").max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for (group, items) in groups {
            let items: Vec<usize> = items
                .into_iter()
                .filter(|i| filter.is_empty() || app.session.tools.presets.get(*i).is_some_and(|p| p.name.to_lowercase().contains(&filter)))
                .collect();
            if items.is_empty() {
                continue;
            }
            let open = !filter.is_empty() || !app.ui.brushes_panel.collapsed.contains(&group);
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, t.radius_sm, t.hover);
            }
            let x = r.left() + 4.0;
            icons::paint(
                ui,
                egui::Rect::from_min_size(egui::pos2(x, r.center().y - 7.0), vec2(14.0, 14.0)),
                if open { "chevron-down" } else { "chevron-right" },
                12.0,
                t.text_dim,
            );
            icons::paint(
                ui,
                egui::Rect::from_min_size(egui::pos2(x + 18.0, r.center().y - 8.0), vec2(16.0, 16.0)),
                if open { "folder-open" } else { "folder" },
                14.0,
                t.icon,
            );
            ui.painter().text(egui::pos2(x + 40.0, r.center().y), egui::Align2::LEFT_CENTER, &group, theme::semibold(12.0), t.text);
            ui.painter().text(
                egui::pos2(r.right() - 8.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                items.len().to_string(),
                theme::medium(11.0),
                t.text_faint,
            );
            if resp.clicked() {
                toggle = Some(group.clone());
            }
            if !open {
                continue;
            }
            for i in items {
                let Some(p) = app.session.tools.presets.get(i) else { continue };
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
                if !ui.is_rect_visible(r) {
                    continue;
                }
                if is_current(&p.brush, brush) {
                    ui.painter().rect_filled(r, t.radius_sm, t.accent_soft);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, t.radius_sm, t.hover);
                }
                let pb = &p.brush;
                let tip = brush_preview::tip_texture(ui.ctx(), &format!("brushes-tip:{}", p.name), &pb.tip, (pb.hardness, pb.angle, pb.roundness), 36, t.text);
                let cell = egui::Rect::from_min_size(r.left_top() + vec2(22.0, 2.0), vec2(36.0, 30.0));
                let tr = egui::Rect::from_center_size(cell.center(), vec2(30.0, 30.0) * crate::brush_sections::thumb_scale(pb.size));
                ui.painter().image(tip.id(), tr, full_uv(), Color32::WHITE);
                ui.painter().text(
                    egui::pos2(cell.center().x, r.bottom() - 1.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("{}", pb.size.round() as i64),
                    egui::FontId::proportional(9.0),
                    t.text_faint,
                );
                let stroke = brush_preview::stroke_texture(ui.ctx(), &format!("brushes-stroke:{}", p.name), pb, 170, 36, t.text);
                let sr = egui::Rect::from_min_size(egui::pos2(cell.right() + 8.0, r.top() + 4.0), vec2(170.0, 36.0));
                ui.painter().image(stroke.id(), sr, full_uv(), Color32::WHITE);
                ui.painter().text(
                    egui::pos2(sr.right() + 12.0, r.center().y),
                    egui::Align2::LEFT_CENTER,
                    &p.name,
                    egui::FontId::proportional(12.0),
                    t.text_dim,
                );
                if resp.clicked() {
                    clicked = Some(p.name.clone());
                }
            }
            ui.add_space(2.0);
        }
    });
    if let Some(g) = toggle {
        let c = &mut app.ui.brushes_panel.collapsed;
        match c.iter().position(|x| *x == g) {
            Some(i) => {
                c.remove(i);
            }
            None => c.push(g),
        }
    }
    if let Some(name) = clicked {
        run_or_status(app, "tools.setBrush", json!({ "preset": name }));
    }
    ui.add_space(4.0);
    widgets::hairline(ui);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} presets", app.session.tools.presets.len())).color(t.text_faint));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let current = app.session.tools.presets.iter().find(|p| is_current(&p.brush, &app.session.tools.brush)).map(|p| p.name.clone());
            if ui.add_enabled_ui(current.is_some(), |ui| icons::button(ui, "trash", 24.0, false, "Delete brush")).inner.clicked()
                && let Some(name) = current
            {
                run_or_status(app, "brush.presets.delete", json!({ "name": name }));
            }
            if icons::button(ui, "square-plus", 24.0, false, "Create new brush from the current settings").clicked() {
                let name = new_preset_name(&app.session.tools.presets);
                run_or_status(app, "brush.presets.save", json!({ "name": name }));
            }
            if icons::button(ui, "folder-open", 24.0, false, "Import Brushes… (.abr)").clicked() {
                app.open_dialog_file();
            }
        });
    });
    // Forget previews of presets that no longer exist.
    let names: std::collections::HashSet<&str> = app.session.tools.presets.iter().map(|p| p.name.as_str()).collect();
    brush_preview::with_cache(ui.ctx(), |c| {
        c.retain(|slot| slot.split_once(':').is_none_or(|(_, n)| names.contains(n)));
    });
}

/// The section list: enable boxes, names, the selection. Clicking a name shows the section and
/// turns it on (Photoshop); clicking a box only toggles it.
fn section_list(app: &mut PhotocraftApp, ui: &mut egui::Ui, b: &mut BrushSettings) {
    let t = Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        ui.set_width(150.0);
        ui.spacing_mut().item_spacing.y = 1.0;
        for (i, (name, has_box)) in SECTIONS.iter().enumerate() {
            let sel = app.ui.brush_section == i;
            let (r, resp) = ui.allocate_exact_size(vec2(150.0, 23.0), Sense::click());
            if sel {
                ui.painter().rect_filled(r, t.radius_sm, t.accent_soft);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, t.radius_sm, t.hover);
            }
            let mut x = r.left() + 6.0;
            if *has_box && let Some(flag) = section_flag(b, i) {
                let br = egui::Rect::from_center_size(egui::pos2(x + 6.0, r.center().y), vec2(13.0, 13.0));
                let box_resp = ui.interact(br, ui.id().with(("brush-sec-box", i)), Sense::click());
                if *flag {
                    ui.painter().rect_filled(br, 2.0, t.accent);
                    let (a, m, c) = (br.left_center() + vec2(3.0, 0.5), br.center_bottom() + vec2(-1.0, -3.5), br.right_top() + vec2(-3.0, 3.5));
                    ui.painter().line_segment([a, m], Stroke::new(1.6, Color32::WHITE));
                    ui.painter().line_segment([m, c], Stroke::new(1.6, Color32::WHITE));
                } else {
                    ui.painter().rect_stroke(br, 2.0, Stroke::new(1.2, t.text_faint), egui::StrokeKind::Inside);
                }
                if box_resp.clicked() {
                    *flag = !*flag;
                } else if resp.clicked() {
                    *flag = true;
                }
                x += 20.0;
            } else if i > 0 {
                x += 20.0;
            }
            let color = if sel { t.text } else { t.text_dim };
            let font = if i == 0 { theme::semibold(12.0) } else { theme::medium(12.0) };
            ui.painter().text(egui::pos2(x, r.center().y), egui::Align2::LEFT_CENTER, *name, font, color);
            if resp.clicked() {
                app.ui.brush_section = i;
            }
            if i == 0 {
                ui.add_space(4.0);
            }
        }
    });
}

fn settings_tab(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let before = app.session.tools.brush.clone();
    let mut b = before.clone();
    let section = app.ui.brush_section.min(SECTIONS.len() - 1);
    ui.horizontal_top(|ui| {
        section_list(app, ui, &mut b);
        widgets::vline(ui, 482.0);
        ui.vertical(|ui| {
            ui.set_width(WIDTH - 190.0);
            ui.label(RichText::new(SECTIONS[section].0).font(theme::semibold(12.5)).color(t.text));
            ui.add_space(6.0);
            let on = section_flag(&mut b, section).is_none_or(|f| *f);
            egui::ScrollArea::vertical().id_salt(("brush-section", section)).max_height(450.0).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(WIDTH - 204.0);
                // A section that's off shows its options greyed out (Photoshop).
                ui.add_enabled_ui(on, |ui| section_body(ui, &mut b, section, &app.session.tools.presets));
            });
        });
    });
    ui.add_space(6.0);
    widgets::hairline(ui);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(STRIP.0 as f32, STRIP.1 as f32), Sense::hover());
        ui.painter().rect_filled(r, t.radius_sm, t.field);
        let tex = brush_preview::stroke_texture(ui.ctx(), "settings-strip", &b, STRIP.0, STRIP.1, t.text);
        ui.painter().image(tex.id(), r, full_uv(), Color32::WHITE);
        ui.vertical(|ui| {
            if icons::button(ui, "square-plus", 24.0, false, "Create new brush from these settings").clicked() {
                let name = new_preset_name(&app.session.tools.presets);
                run_or_status(app, "brush.presets.save", json!({ "name": name, "brush": serde_json::to_value(&b).unwrap_or(Value::Null) }));
            }
            if icons::button(ui, "undo-2", 24.0, false, "Reset the brush to the defaults").clicked() {
                b = BrushSettings { color: b.color, background: b.background, smoothing: b.smoothing.clone(), ..Default::default() };
            }
        });
    });
    commit(app, &before, &b);
}

pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.panels.brush_settings {
        return;
    }
    let t = Tokens::get(ctx);
    let frame = egui::Frame::NONE
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(CornerRadius::same(t.radius_lg as u8))
        .shadow(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: t.shadow })
        .inner_margin(egui::Margin::same(10));
    let canvas = app.last_canvas_rect;
    let mut open = true;
    egui::Window::new("Brush Settings")
        .id(egui::Id::new("brush-settings"))
        .title_bar(false)
        .resizable(false)
        .frame(frame)
        .default_pos(egui::pos2((canvas.right() - WIDTH - 30.0).max(canvas.left() + 8.0), canvas.top() + 24.0))
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.horizontal(|ui| {
                let mut tab = app.ui.brush_tab;
                for (i, name) in ["Brush Settings", "Brushes"].iter().enumerate() {
                    if widgets::pill_tab(ui, name, tab == i).clicked() {
                        tab = i;
                    }
                }
                app.ui.brush_tab = tab;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, "x", 20.0, false, "Close").clicked() {
                        open = false;
                    }
                });
            });
            ui.add_space(6.0);
            widgets::hairline(ui);
            ui.add_space(6.0);
            if app.ui.brush_tab == 1 {
                brushes_tab(app, ui);
            } else {
                settings_tab(app, ui);
            }
        });
    if !open {
        app.ui.panels.brush_settings = false;
    }
}

#[cfg(test)]
#[path = "brush_panel_tests.rs"]
mod tests;
