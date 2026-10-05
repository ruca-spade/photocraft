//! Properties panel for pixel (and other bounded) layers, Photoshop 2026 style: a Transform section
//! (W/H/X/Y of the layer content, editable), Align and Distribute, and Quick Actions. Edits dispatch
//! `edit.transform`, `layer.translate`, `layer.align.*` and selection commands.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use photocraft_doc::Layer;
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::{PhotocraftApp, widgets};

/// `edit.transform` params that scale the content box `b` = [x0, y0, x1, y1] to `w` x `h`, keeping
/// the top-left corner (Photoshop's Properties W/H fields), or `layer.translate` params for X/Y.
pub fn transform_params(layer: u64, b: [i32; 4], w: Option<f32>, h: Option<f32>, linked: bool) -> Option<Value> {
    let (bw, bh) = ((b[2] - b[0]) as f32, (b[3] - b[1]) as f32);
    if bw <= 0.0 || bh <= 0.0 {
        return None;
    }
    let (mut nw, mut nh) = (w.unwrap_or(bw).max(1.0), h.unwrap_or(bh).max(1.0));
    if linked {
        if w.is_some() {
            nh = (nw * bh / bw).max(1.0);
        } else if h.is_some() {
            nw = (nh * bw / bh).max(1.0);
        }
    }
    if (nw - bw).abs() < 0.5 && (nh - bh).abs() < 0.5 {
        return None;
    }
    let (x0, y0) = (b[0] as f32, b[1] as f32);
    let quad = [[x0, y0], [x0 + nw, y0], [x0 + nw, y0 + nh], [x0, y0 + nh]];
    Some(json!({"layer": layer, "rect": b, "quad": quad}))
}

fn section(ui: &mut egui::Ui, id: &str, title: &str) -> bool {
    let title_tr = crate::i18n::ts(title);
    let title = title_tr.as_str();
    let t = Tokens::get(ui.ctx());
    let key = egui::Id::new(("layer-props-section", id));
    let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(true);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    crate::icons::paint(
        ui,
        Rect::from_center_size(pos2(r.left() + 7.0, r.center().y), vec2(12.0, 12.0)),
        if open { "chevron-down" } else { "chevron-right" },
        11.0,
        t.text_dim,
    );
    ui.painter().text(pos2(r.left() + 18.0, r.center().y), egui::Align2::LEFT_CENTER, title, crate::theme::semibold(12.0), t.text);
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(key, open));
    }
    open
}

/// A number field that reports a value once committed (drag released, Enter, focus lost).
fn field(ui: &mut egui::Ui, id: &str, label: &str, current: f32) -> Option<f32> {
    let label_tr = crate::i18n::ts(label);
    let label = label_tr.as_str();
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(16.0, 22.0), Sense::hover());
    ui.painter().text(pos2(r.right() - 2.0, r.center().y), egui::Align2::RIGHT_CENTER, label, egui::FontId::proportional(12.0), t.text_dim);
    let key = egui::Id::new(("layer-props-field", id));
    let mut v = ui.data(|d| d.get_temp::<f32>(key)).unwrap_or(current);
    let resp = widgets::value_field(ui, &mut v, -300_000.0..=300_000.0, "px", 80.0);
    if resp.dragged() || resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(key, v));
        return None;
    }
    ui.data_mut(|d| d.remove::<f32>(key));
    (resp.drag_stopped() || resp.lost_focus() || resp.changed()).then_some(v).filter(|v| (*v - current).abs() >= 0.5)
}

/// Align glyph: a reference line and two bars (Photoshop's align icons).
fn align_glyph(ui: &egui::Ui, r: Rect, kind: &str, color: egui::Color32) {
    let p = ui.painter();
    let c = r.center();
    let s = Stroke::new(1.2, color);
    let vertical_ref = matches!(kind, "leftEdges" | "horizontalCenters" | "rightEdges");
    if vertical_ref {
        let x = match kind {
            "leftEdges" => c.x - 6.0,
            "rightEdges" => c.x + 6.0,
            _ => c.x,
        };
        p.line_segment([pos2(x, c.y - 7.0), pos2(x, c.y + 7.0)], s);
        for (dy, w) in [(-3.0, 10.0), (3.0, 6.0)] {
            let x0 = match kind {
                "leftEdges" => x,
                "rightEdges" => x - w,
                _ => x - w / 2.0,
            };
            p.rect_filled(Rect::from_min_size(pos2(x0, c.y + dy - 1.5), vec2(w, 3.0)), 0.0, color);
        }
    } else {
        let y = match kind {
            "topEdges" => c.y - 6.0,
            "bottomEdges" => c.y + 6.0,
            _ => c.y,
        };
        p.line_segment([pos2(c.x - 7.0, y), pos2(c.x + 7.0, y)], s);
        for (dx, h) in [(-3.0, 10.0), (3.0, 6.0)] {
            let y0 = match kind {
                "topEdges" => y,
                "bottomEdges" => y - h,
                _ => y - h / 2.0,
            };
            p.rect_filled(Rect::from_min_size(pos2(c.x + dx - 1.5, y0), vec2(3.0, h)), 0.0, color);
        }
    }
}

/// Pro Properties body for a non-adjustment layer.
pub fn properties(app: &mut PhotocraftApp, ui: &mut egui::Ui, layer: &Layer) {
    let t = Tokens::get(ui.ctx());
    let mut run: Vec<(String, Value)> = Vec::new();
    if let Some(s) = layer.surface() {
        let b = app.cached_bounds(layer.id.0, s);
        if section(ui, "transform", "Transform") {
            let link_key = egui::Id::new("layer-props-link");
            let linked = ui.data(|d| d.get_temp::<bool>(link_key)).unwrap_or(true);
            let bb = [b.x0, b.y0, b.x1, b.y1];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(v) = field(ui, "w", "W", b.width() as f32) {
                    run.extend(transform_params(layer.id.0, bb, Some(v), None, linked).map(|p| ("edit.transform".to_string(), p)));
                }
                if crate::icons::button(ui, if linked { "link" } else { "unlink" }, 20.0, linked, "Link width and height").clicked() {
                    ui.data_mut(|d| d.insert_temp(link_key, !linked));
                }
                if let Some(v) = field(ui, "h", "H", b.height() as f32) {
                    run.extend(transform_params(layer.id.0, bb, None, Some(v), linked).map(|p| ("edit.transform".to_string(), p)));
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(v) = field(ui, "x", "X", b.x0 as f32) {
                    run.push(("layer.translate".into(), json!({"layer": layer.id.0, "dx": (v - b.x0 as f32).round() as i32, "dy": 0})));
                }
                ui.add_space(24.0);
                if let Some(v) = field(ui, "y", "Y", b.y0 as f32) {
                    run.push(("layer.translate".into(), json!({"layer": layer.id.0, "dx": 0, "dy": (v - b.y0 as f32).round() as i32})));
                }
            });
            ui.add_space(4.0);
        }
        widgets::hairline(ui);
        ui.add_space(2.0);
    }
    if section(ui, "align", "Align and Distribute") {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, kind) in ["leftEdges", "horizontalCenters", "rightEdges", "topEdges", "verticalCenters", "bottomEdges"].into_iter().enumerate() {
                if i == 3 {
                    ui.add_space(8.0);
                }
                let id = format!("layer.align.{kind}");
                let on = app.session.is_enabled(&id);
                let label = photocraft_engine::commands::find(&id).map_or(kind, |c| c.label);
                let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), if on { Sense::click() } else { Sense::hover() });
                if resp.hovered() && on {
                    ui.painter().rect_filled(r, t.radius_sm, t.hover);
                }
                align_glyph(ui, r, kind, if on { t.icon } else { t.text_faint });
                if resp.on_hover_text(format!("Align {label}")).clicked() {
                    run.push((id, json!({})));
                }
            }
        });
        ui.add_space(4.0);
    }
    widgets::hairline(ui);
    ui.add_space(2.0);
    if section(ui, "quick", "Quick Actions") {
        ui.horizontal(|ui| {
            for (label, id) in [("Remove Background", "layer.removeBackground"), ("Select Subject", "select.subject")] {
                if photocraft_engine::commands::find(id).is_some() && widgets::secondary_button(ui, label, 0.0).clicked() {
                    run.push((id.to_string(), json!({})));
                }
            }
        });
    }
    for (id, p) in run {
        if let Err(e) = app.run(&id, p) {
            app.ui.status = e;
            app.ui.status_error = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_edit_scales_from_the_top_left() {
        let p = transform_params(7, [10, 20, 110, 70], Some(200.0), None, true).unwrap();
        assert_eq!(p["layer"], 7);
        assert_eq!(p["quad"], json!([[10.0, 20.0], [210.0, 20.0], [210.0, 120.0], [10.0, 120.0]]));
        let p = transform_params(7, [10, 20, 110, 70], None, Some(25.0), false).unwrap();
        assert_eq!(p["quad"][2], json!([110.0, 45.0]));
        assert!(transform_params(7, [10, 20, 110, 70], Some(100.0), None, true).is_none());
        assert!(transform_params(7, [0, 0, 0, 0], Some(5.0), None, true).is_none());
    }

    #[test]
    fn transform_params_resize_a_layer_in_the_engine() {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 200, "height": 200, "background": "transparent"})).unwrap();
        s.execute("select.rect", json!({"x": 10, "y": 20, "width": 100, "height": 50})).unwrap();
        s.execute("edit.fill", json!({"color": "#ff0000"})).unwrap();
        s.execute("select.deselect", json!({})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let p = transform_params(id.0, [10, 20, 110, 70], Some(50.0), None, true).unwrap();
        s.execute("edit.transform", p).unwrap();
        let b = s.active().unwrap().doc.layer(id).unwrap().surface().unwrap().content_bounds();
        assert_eq!((b.x0, b.y0, b.width(), b.height()), (10, 20, 50, 25));
    }
}
