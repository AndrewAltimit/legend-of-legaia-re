//! The orbit view both 3D panels share: drag to orbit, scroll to zoom, the
//! CPU-rasterised frame cached in a texture until something changes.

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

    /// Fill the rest of `ui` with the view of `model` sampling `vram`.
    pub fn show(&mut self, ui: &mut egui::Ui, model: &Model, vram: &Vram) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::drag());
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
        if let Some(tex) = &self.tex {
            ui.painter().image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    /// The shading toggle and reset button.
    pub fn controls(&mut self, ui: &mut egui::Ui, textured_hint: &str) {
        ui.selectable_value(
            &mut self.shading,
            Shading::Textured,
            "Textured (retail shading)",
        )
        .on_hover_text(textured_hint);
        ui.selectable_value(&mut self.shading, Shading::Lit, "Untextured + viewer light")
            .on_hover_text("A viewer aid with a fixed light - not how retail shades");
        if ui.button("Reset view").clicked() {
            self.cam = Camera::default();
        }
    }
}
