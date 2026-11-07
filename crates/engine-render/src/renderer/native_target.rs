//! The native-resolution frame target: every pass of a frame drawn into an
//! offscreen texture the size of the PSX display, then presented with a
//! nearest-neighbour integer upscale.
//!
//! Retail's GPU rasterises at the display's own size - `320 x 240` in every
//! mode the engine draws - and each stage pixel is one primitive's. Drawn at
//! a window-sized target the host covers a framebuffer pixel wherever a
//! triangle crosses one of its sample points, a supersample of the stage
//! pixel, so thin geometry and every edge land on different pixels. With
//! this target on, the frame *is* the display: the 3D pass, the screen
//! primitives, the text / sprite overlays and the fades all draw at stage
//! scale 1, and the window only magnifies the finished picture.
//!
//! The hosts lay their 2D out against [`Renderer::surface_size`], which
//! reports the native size while the target is on, so nothing above the
//! renderer needs to know the window is larger.

use super::*;

/// The PSX display the native target reproduces: the `320 x 240` screen
/// the GTE projects onto, the stage every host draws in.
pub const PSX_DISPLAY_SIZE: (u32, u32) = (320, 240);

/// Which texel PSX mode on the native target takes for a fragment's UV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PsxTexelRule {
    /// The console's: the UV at the integer sample point plus half a texel
    /// on both axes (the GPU's interpolators start from the vertex value
    /// plus one half), so the texel is the UV rounded. Mednafen captures
    /// match it.
    #[default]
    Gpu,
    /// PCSX-Redux's software GPU: no half-texel bias - the texel at the
    /// pixel centre along the row and at the row itself. Not a player mode:
    /// the retail-compare image child takes it on a PCSX-Redux capture, so
    /// the score measures the engine and not that emulator's rasteriser
    /// (`LEGAIA_PSX_CAPTURE_RASTER=pcsx-redux`).
    PcsxRedux,
}

/// How far back a persisting frame reads: two pages at frame step `4`.
const FRAME_PERSIST_VSYNCS: usize = 8;

fn copy_frame(
    enc: &mut wgpu::CommandEncoder,
    from: &wgpu::Texture,
    to: &wgpu::Texture,
    native: &NativeTarget,
) {
    enc.copy_texture_to_texture(
        from.as_image_copy(),
        to.as_image_copy(),
        wgpu::Extent3d {
            width: native.width,
            height: native.height,
            depth_or_array_layers: 1,
        },
    );
}

/// The offscreen frame of [`Renderer::set_native_resolution`].
pub(crate) struct NativeTarget {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    /// The frame as the blit pipeline's group 0 (texture + nearest sampler).
    pub(super) blit_bg: wgpu::BindGroup,
}

/// How close a window has to come to the next whole step of the 320x240
/// stage before [`fill_target_for`] draws at that step and shrinks the
/// frame onto the window: `0.95` keeps the shrink under one pixel in twenty.
pub const FILL_MIN_RATIO: f32 = 0.95;

/// The frame target a clean (non-PSX) window draws at, when not its own
/// size: a window a little short of a whole step of the 320x240 stage (a
/// 960x720 window a 768-pixel-tall screen hands back 699 tall) draws the
/// whole frame at that step - 3D, text and sprites alike, so the 2D stays
/// on its whole-step grid and lined up with the scene - and the window shows
/// it shrunk to fit ([`native_present_rect`]'s fit branch). Without it the
/// stage fell a whole step (640x480 in a 960x699 window) and the rest was
/// black. `None`: the window holds a whole step, or is too far short of
/// the next one for the shrink to stay sharp.
pub fn fill_target_for(sw: u32, sh: u32) -> Option<(u32, u32)> {
    let (pw, ph) = PSX_DISPLAY_SIZE;
    let fit = (sw as f32 / pw as f32).min(sh as f32 / ph as f32);
    let whole = fit.floor();
    if whole < 1.0 || fit == whole {
        return None;
    }
    let next = whole + 1.0;
    (fit / next >= FILL_MIN_RATIO).then(|| (pw * next as u32, ph * next as u32))
}

/// The integer-scaled, centred rect a `(nw, nh)` frame is presented in on a
/// `(sw, sh)` surface: the largest whole multiple that fits, or the largest
/// aspect-preserving fit when the surface is smaller than the frame.
pub(crate) fn native_present_rect(sw: u32, sh: u32, nw: u32, nh: u32) -> (f32, f32, f32, f32) {
    let (nw, nh) = (nw.max(1), nh.max(1));
    let scale = (sw / nw).min(sh / nh);
    let (w, h) = if scale >= 1 {
        ((nw * scale) as f32, (nh * scale) as f32)
    } else {
        let f = (sw as f32 / nw as f32).min(sh as f32 / nh as f32);
        (nw as f32 * f, nh as f32 * f)
    };
    let x = ((sw as f32 - w) / 2.0).floor().max(0.0);
    let y = ((sh as f32 - h) / 2.0).floor().max(0.0);
    (x, y, w, h)
}

impl Renderer {
    /// Draw every frame at a native `(width, height)` target and present it
    /// with a nearest-neighbour integer upscale (`Some`), or straight onto
    /// the surface at its own size (`None`, the default). PSX mode
    /// ([`Self::set_psx_mode`]) is meant to run on it: the vertex snap, the
    /// fill rule and the dither are retail's only at stage scale 1.
    ///
    /// While it is on, [`Self::surface_size`] and every capture
    /// ([`Self::capture_rgba`]) are the native size; the window's own size is
    /// [`Self::presented_size`].
    pub fn set_native_resolution(&mut self, size: Option<(u32, u32)>) {
        self.native = size.map(|(w, h)| self.build_native_target(w.max(1), h.max(1)));
        self.frame_ring.borrow_mut().clear();
        self.frame_ring_next.set(0);
        let (w, h) = self.target_size();
        self.depth_view = create_depth_view(&self.device, w, h);
    }

    /// The native target's size, when one is set.
    pub fn native_resolution(&self) -> Option<(u32, u32)> {
        self.native.as_ref().map(|n| (n.width, n.height))
    }

    /// The window surface's own size, whatever the frame is drawn at.
    pub fn presented_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// The size every pass of a frame draws at: the native target's when
    /// one is set, the surface's otherwise.
    pub(crate) fn target_size(&self) -> (u32, u32) {
        self.native_resolution()
            .unwrap_or((self.config.width, self.config.height))
    }

    fn build_native_target(&self, width: u32, height: u32) -> NativeTarget {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("native frame target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // The attachment format every pipeline renders through, so the
            // passes draw into it unchanged and the blit copies its bytes.
            format: self.view_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native frame blit bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    // Nearest: each stage pixel becomes a solid block.
                    resource: wgpu::BindingResource::Sampler(&self.text_sampler),
                },
            ],
        });
        NativeTarget {
            width,
            height,
            texture: tex,
            view,
            blit_bg,
        }
    }

    /// Magnify the finished native frame onto `present`: black around the
    /// integer-scaled, centred picture.
    /// Build the coming frames on the pixels an earlier one left instead of a
    /// clear, as retail's battle frames do: its draw environment never clears,
    /// so whatever a frame's opaque draws leave uncovered still holds the buffer
    /// from an earlier frame. Only the native target outlives its frame; on any
    /// other target this does nothing.
    ///
    /// Retail's "earlier frame" is the same page two game frames back, which at
    /// the frame step a heavy arm runs at (`4`) is eight vsyncs. The engine
    /// draws one frame a vsync, so each frame builds on the one
    /// [`FRAME_PERSIST_VSYNCS`] back: built on the last frame, an additive layer
    /// fills the buffer eight times too fast.
    pub fn set_frame_persist(&self, on: bool) {
        let was = self.frame_persist.replace(on);
        if on && !was && self.psx_mode.get() {
            self.seed_frame_ring();
        }
    }

    /// The ring takes what the target holds now: the frames before a persisting
    /// run are all the earlier frames it has.
    fn seed_frame_ring(&self) {
        let Some(native) = &self.native else {
            return;
        };
        let mut ring = self.frame_ring.borrow_mut();
        if ring.is_empty() {
            *ring = (0..FRAME_PERSIST_VSYNCS)
                .map(|_| {
                    self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("frame persist ring"),
                        size: wgpu::Extent3d {
                            width: native.width,
                            height: native.height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.view_format,
                        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    })
                })
                .collect();
        }
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame ring seed"),
            });
        for t in ring.iter() {
            copy_frame(&mut enc, &native.texture, t, native);
        }
        self.queue.submit(std::iter::once(enc.finish()));
    }

    /// The frame a persisting frame builds on, loaded into `dest`.
    pub(super) fn load_persisted_frame(&self, dest: &wgpu::Texture) {
        let Some(native) = &self.native else {
            return;
        };
        let ring = self.frame_ring.borrow();
        let Some(oldest) = ring.get(self.frame_ring_next.get()) else {
            return;
        };
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame ring load"),
            });
        copy_frame(&mut enc, oldest, dest, native);
        self.queue.submit(std::iter::once(enc.finish()));
    }

    /// The presented frame takes the oldest slot of the ring.
    pub(super) fn store_persisted_frame(&self) {
        let Some(native) = &self.native else {
            return;
        };
        let ring = self.frame_ring.borrow();
        let next = self.frame_ring_next.get();
        let Some(slot) = ring.get(next) else {
            return;
        };
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame ring store"),
            });
        copy_frame(&mut enc, &native.texture, slot, native);
        self.queue.submit(std::iter::once(enc.finish()));
        self.frame_ring_next.set((next + 1) % ring.len());
    }

    pub(super) fn present_native(&self, native: &NativeTarget, present: &wgpu::TextureView) {
        self.queue.write_buffer(
            &self.uniforms_buf,
            0,
            bytemuck::cast_slice(&[Uniforms {
                scale: [1.0, 1.0, 0.0, 0.0],
            }]),
        );
        let (x, y, w, h) = native_present_rect(
            self.config.width,
            self.config.height,
            native.width,
            native.height,
        );
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("native present encoder"),
            });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("native present pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: present,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                multiview_mask: None,
                timestamp_writes: None,
            });
            rp.set_viewport(x, y, w, h, 0.0, 1.0);
            rp.set_pipeline(&self.pipeline);
            rp.set_bind_group(0, &native.blit_bg, &[]);
            rp.set_bind_group(1, &self.uniforms_bg, &[]);
            rp.draw(0..4, 0..1);
        }
        self.queue.submit(std::iter::once(enc.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::{fill_target_for, native_present_rect};

    #[test]
    fn a_window_just_short_of_a_step_draws_at_it() {
        // The clamped 960x720 window: drawn at 3x, shown at 932x699.
        assert_eq!(fill_target_for(960, 699), Some((960, 720)));
        let (x, y, w, h) = native_present_rect(960, 699, 960, 720);
        assert_eq!((y, h), (0.0, 699.0));
        assert!((w - 932.0).abs() < 1.0 && x >= 0.0);
        // A whole step, or far from the next: the window's own size.
        assert_eq!(fill_target_for(960, 720), None);
        assert_eq!(fill_target_for(1440, 1080), None);
        assert_eq!(fill_target_for(1280, 800), None);
        assert_eq!(fill_target_for(300, 200), None);
        assert_eq!(fill_target_for(1366, 705), Some((960, 720)));
    }

    #[test]
    fn the_native_frame_presents_at_the_largest_whole_scale() {
        assert_eq!(
            native_present_rect(960, 720, 320, 240),
            (0.0, 0.0, 960.0, 720.0)
        );
        // 1440x1080 holds 4x with an 80 / 60 px frame.
        assert_eq!(
            native_present_rect(1440, 1080, 320, 240),
            (80.0, 60.0, 1280.0, 960.0)
        );
        // A window smaller than the frame fits it, aspect kept.
        assert_eq!(
            native_present_rect(160, 240, 320, 240),
            (0.0, 60.0, 160.0, 120.0)
        );
    }
}
