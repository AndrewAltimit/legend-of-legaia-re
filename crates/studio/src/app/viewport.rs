//! The orbit view both 3D panels share: drag to orbit, scroll to zoom,
//! double-click to reset, the CPU-rasterised frame cached in a texture until
//! something changes. The hint and a caption (the triangle count) are drawn
//! inside the view, so a narrow window spends its rows on the model, not on
//! toolbar lines.

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use legaia_tim::Vram;

use crate::raster::{self, Camera, Model, Shading};

pub(crate) struct Viewport {
    pub cam: Camera,
    pub shading: Shading,
    /// Bumped by the owner whenever the model's pose or VRAM changes.
    pub version: u64,
    rendered: Option<(Camera, Shading, [usize; 2], u64)>,
    tex: Option<TextureHandle>,
}

impl Viewport {
    pub fn new(shading: Shading) -> Self {
        Self {
            cam: Camera::default(),
            shading,
            version: 0,
            rendered: None,
            tex: None,
        }
    }

    /// Fill the rest of `ui` with the view of `model` sampling `vram`;
    /// `caption` is drawn in the view's top-left corner.
    pub fn show(&mut self, ui: &mut egui::Ui, model: &Model, vram: &Vram, caption: &str) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        if resp.double_clicked() {
            self.cam = Camera::default();
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            self.cam.yaw += d.x * 0.01;
            // Dragging down tips the model's top toward the viewer (the eye
            // rises), as on the site's mesh viewers.
            self.cam.pitch = (self.cam.pitch + d.y * 0.01).clamp(-1.5, 1.5);
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.cam.zoom = (self.cam.zoom * (1.0 - scroll * 0.002)).clamp(0.1, 10.0);
            }
        }
        let ppp = ui.ctx().pixels_per_point();
        let px = [
            ((rect.width() * ppp) as usize).clamp(16, 1600),
            ((rect.height() * ppp) as usize).clamp(16, 1200),
        ];
        let state = (self.cam, self.shading, px, self.version);
        if self.rendered != Some(state) || self.tex.is_none() {
            let rgba = raster::render(model, vram, self.cam, self.shading, px[0], px[1]);
            let img = ColorImage::from_rgba_unmultiplied(px, &rgba);
            match &mut self.tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => {
                    self.tex = Some(ui.ctx().load_texture("model", img, TextureOptions::LINEAR))
                }
            }
            self.rendered = Some(state);
        }
        ui.painter()
            .rect_filled(rect, 0.0, crate::theme::c(crate::theme::palette(ui).stage));
        if let Some(tex) = &self.tex {
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        overlay(ui, rect, caption, VIEW_HINT);
    }

    /// The shading picker and the reset button: two compact controls, so
    /// the toolbar they sit in stays one row at the minimum window width.
    pub fn controls(&mut self, ui: &mut egui::Ui, textured_hint: &str) {
        egui::ComboBox::from_id_salt(ui.id().with("shading"))
            .selected_text(shading_name(self.shading))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.shading,
                    Shading::Textured,
                    "Textured (retail shading)",
                )
                .on_hover_text(textured_hint);
                ui.selectable_value(&mut self.shading, Shading::Lit, "Untextured + viewer light")
                    .on_hover_text(LIT_HINT);
            })
            .response
            .on_hover_text(match self.shading {
                Shading::Textured => textured_hint,
                Shading::Lit => LIT_HINT,
            });
        if ui
            .button("Reset")
            .on_hover_text("Reset the view: the starting angle and zoom (or double-click the view)")
            .clicked()
        {
            self.cam = Camera::default();
        }
    }
}

/// The interaction hint drawn in the view's bottom-left corner.
pub(crate) const VIEW_HINT: &str = "Drag to orbit, scroll to zoom, double-click to reset";

const LIT_HINT: &str = "A viewer aid with a fixed light - not how retail shades";

/// The picker's short name for a shading mode.
fn shading_name(s: Shading) -> &'static str {
    match s {
        Shading::Textured => "Textured (retail)",
        Shading::Lit => "Untextured + light",
    }
}

/// `caption` in the top-left and `hint` in the bottom-left corner of the
/// view, each on a translucent plate so it reads over any model.
fn overlay(ui: &egui::Ui, rect: egui::Rect, caption: &str, hint: &str) {
    let p = ui.painter_at(rect);
    let ink = crate::theme::c(crate::theme::palette(ui).ink_dim);
    let plate = crate::theme::c(crate::theme::palette(ui).ground).gamma_multiply(0.75);
    let pad = egui::vec2(6.0, 3.0);
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    for (text, font, anchor, at) in [
        (
            caption,
            body,
            egui::Align2::LEFT_TOP,
            rect.left_top() + egui::vec2(6.0, 6.0),
        ),
        (
            hint,
            small,
            egui::Align2::LEFT_BOTTOM,
            rect.left_bottom() + egui::vec2(6.0, -6.0),
        ),
    ] {
        if text.is_empty() {
            continue;
        }
        let galley = p.layout_no_wrap(text.to_owned(), font, ink);
        let box_rect = anchor.anchor_size(at, galley.size() + pad * 2.0);
        p.rect_filled(box_rect, 3.0, plate);
        p.galley(box_rect.min + pad, galley, ink);
    }
}
