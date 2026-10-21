//! Offscreen readback: frame capture into RGBA and into the software VRAM.

use super::*;

/// A CPU-side RGBA8 readback of one rendered frame, produced by
/// [`Renderer::capture_rgba`]. `rgba` is exactly `width * height * 4`
/// bytes, row-major, top-to-bottom, no row padding.
#[derive(Debug, Clone)]
pub struct CaptureImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Cut an `(x, y, w, h)` window out of a captured frame. The caller has
/// bounded the window by the image.
fn crop_capture(img: &CaptureImage, x: u32, y: u32, w: u32, h: u32) -> CaptureImage {
    let stride = img.width as usize * 4;
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y..y + h {
        let start = row as usize * stride + x as usize * 4;
        rgba.extend_from_slice(&img.rgba[start..start + w as usize * 4]);
    }
    CaptureImage {
        rgba,
        width: w,
        height: h,
    }
}

impl CaptureImage {
    /// Whether any pixel is not opaque. Every pass the frame encodes writes
    /// alpha `1` (the clear, and each blend state's alpha replaces), so a
    /// rendered frame is opaque everywhere; the damaged readback the
    /// harnesses have caught is a band of `(0, 0, 0, 0)` texels - tiles of
    /// the target the copy took before the frame had written them.
    pub fn has_unwritten_texels(&self) -> bool {
        self.rgba.as_chunks::<4>().0.iter().any(|px| px[3] != 0xFF)
    }
}

/// How many readbacks [`Renderer::capture_rgba_settled`] takes at most.
pub const SETTLED_CAPTURE_TRIES: usize = 8;

impl Renderer {
    /// Render one frame offscreen and land it in the **software PSX VRAM** at
    /// `dst`, quantised to BGR555 - the port's equivalent of the console's
    /// framebuffer living inside the same page textures are read from.
    ///
    /// This is what makes a drawn frame addressable as a texture. Retail's
    /// field-to-battle curtain slices a captured field frame into row and
    /// column strips and stretches them apart; its strips' texture pages
    /// decode to [`legaia_render_kernels::vram_capture::FIELD_CAPTURE_ROWS`] /
    /// `FIELD_CAPTURE_COLS`, so a caller reproducing that style captures into
    /// those rects and then draws the strips through
    /// [`RenderTarget::SceneWithScreenPrims`] (or `ScreenOverlay`) against the
    /// re-uploaded VRAM.
    ///
    /// The written page is the CPU-side [`Vram`], not a GPU-only copy, so the
    /// capture is equally visible to `Vram::move_image`, the VRAM parity
    /// oracle and `region_has_data`. Push it back to the GPU with
    /// [`Self::upload_vram`] before drawing against it.
    ///
    /// Cost: one offscreen frame plus a full readback and a resample, so this
    /// is a transition-frame primitive, not something to run every frame.
    pub fn capture_into_vram(
        &self,
        target: RenderTarget<'_>,
        vram: &mut Vram,
        dst: legaia_render_kernels::vram_capture::VramRect,
        opts: legaia_render_kernels::vram_capture::CaptureOpts,
    ) -> Result<usize> {
        let img = self.capture_scene_rgba(target)?;
        Ok(legaia_render_kernels::vram_capture::blit_rgba_into_vram(
            &img.rgba, img.width, img.height, vram, dst, opts,
        ))
    }

    /// [`Self::capture_rgba`], cropped to the scene viewport
    /// ([`Self::set_scene_viewport`]) when one is set: the picture the 3D
    /// pass drew, without the letterbox around it. What a frame-into-VRAM
    /// capture wants - retail's framebuffer *is* the 320x240 display, so the
    /// bars are not part of it.
    pub fn capture_scene_rgba(&self, target: RenderTarget<'_>) -> Result<CaptureImage> {
        let img = self.capture_rgba(target)?;
        let Some((x, y, w, h)) = self
            .scene_viewport
            .get()
            .filter(|&(x, y, w, h)| w > 0 && h > 0 && x + w <= img.width && y + h <= img.height)
        else {
            return Ok(img);
        };
        Ok(crop_capture(&img, x, y, w, h))
    }

    /// [`Self::capture_rgba`] read until it is a frame the scene drew: the
    /// first opaque readback ([`CaptureImage::has_unwritten_texels`]) that
    /// the next opaque one reproduces byte for byte, within
    /// [`SETTLED_CAPTURE_TRIES`] readbacks (the last opaque one otherwise,
    /// and the first one when none is opaque).
    ///
    /// Re-rendering an unchanged target is deterministic, yet under heavy
    /// GPU contention (the runners share one GPU) a readback has come back
    /// with tiles of its top rows still zero: a band of 1 to 16 rows whose
    /// lower edge steps every 32 columns, the rest byte-identical. Two
    /// consecutive readbacks can carry the same band, so agreement alone
    /// let it through; the alpha channel cannot be zero in a drawn frame.
    /// The retail-compare corpus scored `palma_summon_mid_cast` and
    /// `jedo_summon_mid_cast`, white flash frames, `0.036` low on such runs:
    /// the band is one stage row of black across the forty top blocks.
    pub fn capture_rgba_settled<'a>(
        &self,
        target: &dyn Fn() -> RenderTarget<'a>,
    ) -> Result<CaptureImage> {
        let mut last: Option<CaptureImage> = None;
        let mut last_err = None;
        for _ in 0..SETTLED_CAPTURE_TRIES {
            match self.capture_rgba(target()) {
                Ok(img) if img.has_unwritten_texels() => {
                    if last.is_none() {
                        last = Some(img);
                    }
                }
                Ok(img) => {
                    if let Some(prev) = &last
                        && !prev.has_unwritten_texels()
                        && prev.rgba == img.rgba
                    {
                        return Ok(img);
                    }
                    last = Some(img);
                }
                Err(e) => last_err = Some(e),
            }
        }
        match (last, last_err) {
            (Some(img), _) => Ok(img),
            (None, Some(e)) => Err(e),
            (None, None) => anyhow::bail!("no capture taken"),
        }
    }

    /// Render one frame into an offscreen texture at the current surface
    /// size and read it back to a CPU RGBA8 buffer (no window / no present).
    /// This is the headless screenshot path - the deterministic replacement
    /// for scrapping the on-screen window with `scrot`. The returned image
    /// is exactly `width * height * 4` bytes, row-major, no padding.
    pub fn capture_rgba(&self, target: RenderTarget<'_>) -> Result<CaptureImage> {
        let (width, height) = (self.config.width, self.config.height);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // UNORM, like the presented frame: a screenshot must read back the
            // exact bytes the window shows (see `choose_surface_format`).
            format: self.view_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.encode_frame(target, &view)?;
        // Let the frame's own submission finish before the copy is encoded.
        // The queue orders the two, but under heavy GPU contention a
        // retail-compare readback has come back with whole 32-px-wide tile
        // columns of its top rows still zero - a black band of 1 to 15 rows
        // whose lower edge steps every 32 columns, the rest of the frame
        // byte-identical. Waiting here keeps the copy off tiles the frame
        // is still writing; the screenshot harness also re-reads until two
        // readbacks agree.
        self.device
            .poll(wgpu::PollType::wait())
            .context("poll device for the captured frame")?;

        // copy_texture_to_buffer requires bytes_per_row aligned to 256.
        let bpp = 4u32;
        let unpadded = width * bpp;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (padded as u64) * (height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("capture encoder"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(enc.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait())
            .context("poll device for capture readback")?;
        rx.recv()
            .context("capture readback channel closed")?
            .context("map capture readback buffer")?;

        let data = buf.slice(..).get_mapped_range();
        // Swizzle BGRA->RGBA when the surface uses a Bgra format (typical on
        // desktop swapchains); Rgba formats pass through unchanged.
        let bgra = matches!(
            self.view_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut rgba = Vec::with_capacity((unpadded as usize) * (height as usize));
        for row in 0..height as usize {
            let start = row * padded as usize;
            let line = &data[start..start + unpadded as usize];
            if bgra {
                for px in line.as_chunks::<4>().0 {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            } else {
                rgba.extend_from_slice(line);
            }
        }
        drop(data);
        buf.unmap();
        Ok(CaptureImage {
            rgba,
            width,
            height,
        })
    }
}
