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

/// The offscreen frame of [`Renderer::set_native_resolution`].
pub(crate) struct NativeTarget {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) view: wgpu::TextureView,
    /// The frame as the blit pipeline's group 0 (texture + nearest sampler).
    pub(super) blit_bg: wgpu::BindGroup,
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
            view,
            blit_bg,
        }
    }

    /// Magnify the finished native frame onto `present`: black around the
    /// integer-scaled, centred picture.
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
    use super::native_present_rect;

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
