//! Photoshop 2026 window chrome details: the status bar's info field and its "Show" menu, and the
//! Home button at the start of the options bar.

use egui::{RichText, Sense, Stroke, vec2};
use photocraft_doc::{Document, LayerContent};
use serde::{Deserialize, Serialize};

use crate::theme::Tokens;
use crate::{PhotocraftApp, icons, widgets};

/// Shell-only chrome state (serialised with the UI state, so `ui.inspect` reports it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChromeState {
    /// What the status bar shows next to the zoom field (one of [`STATUS_INFO`] keys).
    pub status_info: String,
    /// Home screen shown over the open documents (Options bar › Home); holds the document count
    /// when it was opened, so opening or creating a document leaves it.
    pub home: Option<usize>,
}

impl ChromeState {
    /// Is the Home screen up, given the current number of open documents?
    pub fn shows_home(&self, documents: usize) -> bool {
        documents == 0 || self.home == Some(documents)
    }
}

impl Default for ChromeState {
    fn default() -> Self {
        Self { status_info: "dimensions".into(), home: None }
    }
}

/// The status bar "Show" menu, in Photoshop's order: (key, menu label).
pub const STATUS_INFO: &[(&str, &str)] = &[
    ("sizes", "Document Sizes"),
    ("profile", "Document Profile"),
    ("dimensions", "Document Dimensions"),
    ("measurementScale", "Measurement Scale"),
    ("scratch", "Scratch Sizes"),
    ("efficiency", "Efficiency"),
    ("tool", "Current Tool"),
    ("layers", "Layer Count"),
];

/// Photoshop's compact byte format: "10.3M", "412.5K".
pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1}G", b / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.1}M", b / (1024.0 * 1024.0))
    } else {
        format!("{:.1}K", b / 1024.0)
    }
}

/// Flattened size and layered size in bytes ("Doc: flat/layered").
pub fn document_sizes(doc: &Document) -> (u64, u64) {
    let bpc = (doc.depth.bits() / 8).max(1) as u64;
    let channels = doc.mode.color_channels().max(1) as u64;
    let flat = doc.size.width as u64 * doc.size.height as u64 * channels * bpc;
    let mut layered = 0u64;
    for (_, _, l) in doc.walk() {
        if let LayerContent::Raster(s) = &l.content {
            // Layers carry transparency on top of the colour channels; clip to the canvas.
            let r = s.tile_bounds().intersect(&doc.bounds());
            layered += r.width() as u64 * r.height() as u64 * (channels + 1) * bpc;
        }
    }
    (flat, layered.max(flat))
}

/// The status bar text for `key` (Photoshop wording).
pub fn status_info_text(doc: &Document, key: &str, tool: &str, profile: &str) -> String {
    let bits = doc.depth.bits();
    match key {
        "sizes" => {
            let (flat, layered) = document_sizes(doc);
            format!("Doc: {}/{}", fmt_bytes(flat), fmt_bytes(layered))
        }
        "profile" => format!("{profile} ({bits}bpc)"),
        "measurementScale" => "1 pixel = 1.0000 pixels".into(),
        "scratch" => {
            let (_, layered) = document_sizes(doc);
            format!("Scratch: {}", fmt_bytes(layered))
        }
        "efficiency" => "Efficiency: 100%".into(),
        "tool" => tool.to_string(),
        "layers" => {
            let n = doc.layer_count();
            format!("{n} {}", if n == 1 { "Layer" } else { "Layers" })
        }
        _ => format!("{} px x {} px ({} ppi)", doc.size.width, doc.size.height, widgets::fmt_num(doc.resolution_dpi as f64)),
    }
}

fn profile_name(doc: &Document) -> String {
    if doc.icc_profile.is_none() {
        return format!("Untagged {}", crate::canvas::mode_label(doc));
    }
    photocraft_engine::color_cmds::document_profile(doc).description.clone()
}

/// Status bar body (Pro): zoom %, the chosen info field and its ">" menu, then status messages.
pub fn status_bar_pro(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (Some(st), Some(i)) = (app.session.active(), app.session.active_index()) else {
        ui.label(RichText::new(crate::i18n::ts("No document")).color(t.text_dim));
        return;
    };
    let text = status_info_text(&st.doc, &app.ui.chrome.status_info, &crate::i18n::ts(app.ui.tool.label()), &profile_name(&st.doc));
    let mut pct = app.ui.views[i].zoom * 100.0;
    if widgets::value_field(ui, &mut pct, 1.0..=3200.0, "%", 64.0).changed() {
        app.ui.views[i].zoom = pct / 100.0;
        app.ui.views[i].fit_pending = false;
    }
    ui.add_space(12.0);
    ui.label(RichText::new(text).color(t.text_dim).size(12.0));
    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    icons::paint(ui, r, "chevron-right", 11.0, t.text_dim);
    let resp = resp.on_hover_text(crate::i18n::ts("Show"));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(200.0);
        for (key, label) in STATUS_INFO {
            let on = app.ui.chrome.status_info == *key;
            if ui.add(egui::Button::selectable(on, *label)).clicked() {
                app.ui.chrome.status_info = (*key).to_string();
                ui.close();
            }
        }
    });
    if !app.ui.status.is_empty() {
        let (r, _) = ui.allocate_exact_size(vec2(17.0, 16.0), Sense::hover());
        ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
        let is_err = app.ui.status_error || app.ui.status.starts_with("Couldn");
        ui.label(RichText::new(&app.ui.status).color(if is_err { t.warning } else { t.text_faint }));
    }
}

/// Home button at the very start of Photoshop 2026's options bar: toggles the Home (start)
/// screen over the open documents, which stay open.
pub fn home_button(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let n = app.session.documents().len();
    let on = app.ui.chrome.shows_home(n);
    if icons::button(ui, "house", 26.0, on && n > 0, "Home").clicked() && n > 0 {
        app.ui.chrome.home = if on { None } else { Some(n) };
    }
}

/// Marquee drag end point under the options-bar Style (Normal, Fixed Ratio, Fixed Size) and Shift
/// (square/circle while drawing with Normal style), Photoshop behaviour.
pub fn marquee_end(style: &str, w: f64, h: f64, shift: bool, start: [f64; 2], end: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let (sx, sy) = (if dx < 0.0 { -1.0 } else { 1.0 }, if dy < 0.0 { -1.0 } else { 1.0 });
    match style {
        "fixedSize" => [start[0] + sx * w.max(1.0), start[1] + sy * h.max(1.0)],
        "fixedRatio" if w > 0.0 && h > 0.0 => {
            // The larger drag extent wins; the other follows the ratio.
            let k = (dx.abs() / w).max(dy.abs() / h);
            [start[0] + sx * k * w, start[1] + sy * k * h]
        }
        _ if shift => {
            let m = dx.abs().max(dy.abs());
            [start[0] + sx * m, start[1] + sy * m]
        }
        _ => end,
    }
}

/// Crop options bar ratio presets: (key, label).
pub const CROP_RATIOS: &[(&str, &str)] = &[
    ("", "Ratio"),
    ("original", "Original Ratio"),
    ("1:1", "1 : 1 (Square)"),
    ("4:5", "4 : 5 (8 : 10)"),
    ("5:7", "5 : 7"),
    ("2:3", "2 : 3 (4 : 6)"),
    ("16:9", "16 : 9"),
];

/// Width/height of a crop ratio key (`original` uses the document size).
pub fn crop_ratio(key: &str, doc_w: f64, doc_h: f64) -> Option<(f64, f64)> {
    if key == "original" {
        return Some((doc_w, doc_h));
    }
    let (a, b) = key.split_once(':')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Document {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 2400, "height": 1500, "resolution": 72, "background": "white"})).unwrap();
        (*s.active().unwrap().doc).clone()
    }

    #[test]
    fn default_status_shows_dimensions_like_photoshop() {
        let d = doc();
        assert_eq!(ChromeState::default().status_info, "dimensions");
        assert_eq!(status_info_text(&d, "dimensions", "", ""), "2400 px x 1500 px (72 ppi)");
        assert_eq!(status_info_text(&d, "layers", "", ""), "1 Layer");
        assert_eq!(status_info_text(&d, "tool", "Brush Tool", ""), "Brush Tool");
        assert!(status_info_text(&d, "profile", "", "sRGB IEC61966-2.1").ends_with("(8bpc)"));
    }

    #[test]
    fn document_sizes_match_photoshop_rounding() {
        let d = doc();
        // 2400 x 1500 x 3 bytes = 10.3M flattened, as Photoshop shows.
        assert_eq!(status_info_text(&d, "sizes", "", ""), "Doc: 10.3M/13.7M");
        assert_eq!(fmt_bytes(512 * 1024), "512.0K");
    }

    #[test]
    fn home_screen_closes_when_a_document_opens() {
        let mut c = ChromeState::default();
        assert!(c.shows_home(0));
        assert!(!c.shows_home(2));
        c.home = Some(2);
        assert!(c.shows_home(2));
        assert!(!c.shows_home(3));
    }

    #[test]
    fn marquee_styles_constrain_the_drag() {
        assert_eq!(marquee_end("normal", 1.0, 1.0, false, [10.0, 10.0], [40.0, 20.0]), [40.0, 20.0]);
        assert_eq!(marquee_end("normal", 1.0, 1.0, true, [10.0, 10.0], [40.0, 20.0]), [40.0, 40.0]);
        assert_eq!(marquee_end("fixedSize", 64.0, 32.0, false, [10.0, 10.0], [5.0, 50.0]), [-54.0, 42.0]);
        assert_eq!(marquee_end("fixedRatio", 2.0, 1.0, false, [0.0, 0.0], [10.0, 30.0]), [60.0, 30.0]);
    }

    #[test]
    fn crop_ratios_parse() {
        assert_eq!(crop_ratio("16:9", 1.0, 1.0), Some((16.0, 9.0)));
        assert_eq!(crop_ratio("original", 2400.0, 1500.0), Some((2400.0, 1500.0)));
        assert_eq!(crop_ratio("", 1.0, 1.0), None);
        let (w, h) = crop_ratio("1:1", 0.0, 0.0).unwrap();
        assert_eq!(marquee_end("fixedRatio", w, h, false, [0.0, 0.0], [50.0, 20.0]), [50.0, 50.0]);
    }

    #[test]
    fn every_status_key_has_text() {
        let d = doc();
        for (k, _) in STATUS_INFO {
            assert!(!status_info_text(&d, k, "x", "p").is_empty());
        }
    }
}
