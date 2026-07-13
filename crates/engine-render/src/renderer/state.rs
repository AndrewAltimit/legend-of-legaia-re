//! The [`Renderer`] struct field definitions plus its small state
//! accessors (PSX-mode toggle, texture-window register, resize, surface
//! size). Split out of `renderer/core.rs`.

use super::*;

pub struct Renderer {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    /// Format every colour attachment is rendered through - the UNORM twin of
    /// [`Self::config`]'s format (see `choose_surface_format`). The shaders
    /// emit PSX framebuffer bytes, so the attachment must never re-encode
    /// them to sRGB.
    pub(super) view_format: wgpu::TextureFormat,
    /// Quad pipeline (Phase 1 TIM viewer).
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) sampler: wgpu::Sampler,
    pub(super) bind_group_layout: wgpu::BindGroupLayout,
    pub(super) uniforms_buf: wgpu::Buffer,
    pub(super) uniforms_bg: wgpu::BindGroup,
    /// Mesh pipeline (Phase 1 TMD viewer).
    pub(super) mesh_pipeline: wgpu::RenderPipeline,
    pub(super) mesh_uniforms_buf: wgpu::Buffer,
    pub(super) mesh_uniforms_bg: wgpu::BindGroup,
    /// Textured-mesh pipeline (Phase 1 TMD viewer with paired TIM).
    /// Reuses [`Self::bind_group_layout`] for the per-mesh texture binding.
    pub(super) textured_mesh_pipeline: wgpu::RenderPipeline,
    /// VRAM-mesh pipeline: per-vertex CBA/TSB + R16Uint VRAM texture lookup.
    pub(super) vram_mesh_pipeline: wgpu::RenderPipeline,
    /// PSX semi-transparency blend pipelines for [`Self::vram_mesh_pipeline`],
    /// one per ABR mode 0..=3 (see [`psx_blend`]). Drawn as a second pass
    /// over each mesh's semi-transparent index tail when PSX-faithful mode
    /// is on ([`Self::set_psx_mode`]).
    pub(super) vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    pub(super) vram_bgl: wgpu::BindGroupLayout,
    /// Multi-actor "scene" VRAM-mesh pipeline. Identical to
    /// [`Self::vram_mesh_pipeline`] but binds [`Self::scene_uniforms_bgl`]
    /// at group 0 (with `has_dynamic_offset = true`) so a single render
    /// pass can draw N actors with one bind group + N dynamic offsets.
    pub(super) scene_vram_mesh_pipeline: wgpu::RenderPipeline,
    /// Scene-layout twins of [`Self::vram_mesh_blend_pipelines`].
    pub(super) scene_vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    /// Lines pipeline shadowing the scene path (uses the scene-uniforms
    /// dynamic-offset layout). Used by [`Self::render`] when a `Scene`
    /// carries `overlay_lines`.
    pub(super) scene_lines_pipeline: wgpu::RenderPipeline,
    /// Untextured vertex-colour mesh pipeline shadowing the scene path (same
    /// scene-uniforms dynamic-offset layout, no VRAM group). Draws a `Scene`'s
    /// `color_draws` (`F*`/`G*` props).
    pub(super) scene_color_mesh_pipeline: wgpu::RenderPipeline,
    /// PSX semi-transparency blend pipelines for
    /// [`Self::scene_color_mesh_pipeline`], one per ABR mode 0..=3 (see
    /// [`psx_blend`]). Unlike the textured blend pass there is no per-texel
    /// STP gate: an untextured ABE prim blends *all* its pixels, so the
    /// fragment entries just emit the interpolated vertex colour (mode 3
    /// pre-scaled by 0.25) and the fixed-function blend state does the rest.
    pub(super) scene_color_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    pub(super) scene_uniforms_bgl: wgpu::BindGroupLayout,
    pub(super) scene_uniforms_bg: std::cell::RefCell<wgpu::BindGroup>,
    pub(super) scene_uniforms_buf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity (in `MeshUniforms` slots) of [`Self::scene_uniforms_buf`].
    pub(super) scene_uniforms_capacity: std::cell::Cell<usize>,
    /// `min_uniform_buffer_offset_alignment` reported by the adapter.
    /// Per-actor uniform writes are padded up to this stride.
    pub(super) uniform_offset_alignment: u32,
    /// Lines pipeline: LineList topology, per-vertex color, depth-tested.
    /// Used for wireframe rendering of stage geometry.
    pub(super) lines_pipeline: wgpu::RenderPipeline,
    /// Text pipeline: 2D textured quads, alpha-blended, no depth. Group 0
    /// binds a sampled font atlas. Used for HUD / debug / dialog overlays.
    pub(super) text_pipeline: wgpu::RenderPipeline,
    /// Bind-group layout for the font-atlas texture binding (group 0 of
    /// [`Self::text_pipeline`]). Reused when uploading new atlases.
    pub(super) text_atlas_bgl: wgpu::BindGroupLayout,
    /// Sampler used by the text pipeline. Nearest-neighbour to keep PSX
    /// pixel-art glyphs crisp.
    pub(super) text_sampler: wgpu::Sampler,
    /// Per-frame text vertex / index buffers (RefCell-borrowed from the
    /// non-mut `render` API; resized geometrically on demand).
    pub(super) text_vbuf: std::cell::RefCell<wgpu::Buffer>,
    pub(super) text_ibuf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity of [`Self::text_vbuf`] in vertex slots and
    /// [`Self::text_ibuf`] in index slots. Both grow together - one quad
    /// per `TextDraw` is 4 vertices and 6 indices.
    pub(super) text_vertex_capacity: std::cell::Cell<u32>,
    pub(super) text_index_capacity: std::cell::Cell<u32>,
    /// Per-overlay quad ranges from the most recent staging call -
    /// `[(base_quad, count), ...]` in the same order overlays were passed.
    /// Drained by the in-pass draw to issue one `draw_indexed` per overlay
    /// with the matching atlas bound.
    pub(super) scene_quad_ranges: std::cell::RefCell<Vec<(u32, u32)>>,
    /// Per-frame blend-pass ordering list (see
    /// [`psx_blend::BlendListEntry`]). RefCell-borrowed from the non-mut
    /// `render` API; cleared and refilled each frame, capacity persists so
    /// the per-prim sort allocates nothing in steady state.
    pub(super) blend_list: std::cell::RefCell<Vec<psx_blend::BlendListEntry>>,
    /// Depth target - recreated on resize.
    pub(super) depth_view: wgpu::TextureView,
    /// PSX-faithful rendering mode. When `true`, the VRAM-mesh shader uses
    /// affine (linear-in-screen-space) UV interpolation instead of
    /// perspective-correct, and snaps clip-space x/y to integer pixel
    /// positions to reproduce the GTE's per-vertex sub-pixel-truncation
    /// "vertex jitter." Default `false` for clean smooth rendering.
    pub(super) psx_mode: std::cell::Cell<bool>,
    /// Count of [`Self::upload_vram`] calls; stamps each [`UploadedVram`]
    /// with a strictly-increasing generation (see
    /// [`UploadedVram::generation`]).
    pub(super) vram_upload_counter: std::cell::Cell<u64>,
    /// GP0(0xE2) "Texture Window setting" - `(mask_x, mask_y, off_x, off_y)`
    /// each in 8-pixel steps (0..=31). Applied per-fragment in the
    /// VRAM-mesh shader. Defaults to all-zero (no-op), which matches
    /// retail Legaia's typical state - the register only gets non-zero
    /// values from a handful of effect / scene-init scripts.
    pub(super) tex_window: std::cell::Cell<[u32; 4]>,
    /// Full-scene colour grade `(gold_r, gold_g, gold_b, strength)` staged
    /// into every field `MeshUniforms` (see [`super::uploaded::MeshUniforms`]).
    /// Defaults to `(1, 1, 1, 0)` = identity (no grade). Set with
    /// [`Renderer::set_color_grade`]; drives the opening prologue sepia.
    pub(super) color_grade: std::cell::Cell<[f32; 4]>,
    /// Screen-space 2D overlay pass (see [`crate::screen_overlay`]): PSX
    /// `POLY_FT4` textured quads + flat quads in NDC, ordering-table order,
    /// per-ABR semi-transparency. Opaque pipeline (replace).
    pub(super) screen_overlay_pipeline: wgpu::RenderPipeline,
    /// Per-ABR-mode blend pipelines for the screen-overlay pass (mode 0..=3).
    pub(super) screen_overlay_blend_pipelines: [wgpu::RenderPipeline; 4],
    /// Per-frame screen-overlay vertex/index buffers, grown geometrically on
    /// demand (RefCell-borrowed from the non-mut `render` API, like the text
    /// path).
    pub(super) screen_overlay_vbuf: std::cell::RefCell<wgpu::Buffer>,
    pub(super) screen_overlay_ibuf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity of [`Self::screen_overlay_vbuf`] in vertices and
    /// [`Self::screen_overlay_ibuf`] in indices.
    pub(super) screen_overlay_vcap: std::cell::Cell<u32>,
    pub(super) screen_overlay_icap: std::cell::Cell<u32>,
    /// Draw runs staged for the current frame's screen overlay (one indexed
    /// draw per run; see [`crate::screen_overlay::DrawRun`]).
    pub(super) screen_overlay_runs: std::cell::RefCell<Vec<crate::screen_overlay::DrawRun>>,
}

impl Renderer {
    /// Toggle PSX-style rendering on the 3D mesh pipelines. With
    /// `psx_mode = true` the pipeline mimics the retail GTE/GPU: UVs
    /// interpolate linearly in screen space (no perspective correction → the
    /// characteristic surface-warp on slanted surfaces), clip-space X/Y are
    /// snapped to integer pixels (the GTE's per-vertex jitter), and the
    /// shaded colour is ordered-dithered down to 15-bit BGR555 (the PSX
    /// framebuffer depth - see `PSX_DITHER_WGSL` / [`psx_dither`]). Default
    /// `false` (smooth, full-precision modern rendering).
    pub fn set_psx_mode(&self, enable: bool) {
        self.psx_mode.set(enable);
    }

    /// Read current PSX-mode flag.
    pub fn psx_mode(&self) -> bool {
        self.psx_mode.get()
    }

    /// Set the GP0(0xE2) "Texture Window setting" register state used by
    /// the VRAM-mesh fragment shader. Values are in 8-pixel steps
    /// (0..=31): `mask` selects which texel-coordinate bits get forced
    /// from `offset` (the standard PSX wrap-window mechanic). All-zero
    /// (the default) is a no-op - texel coords pass through unchanged.
    ///
    /// Retail Legaia leaves this register at zero almost everywhere; a
    /// handful of effect / scene-init scripts in the move-VM extension
    /// table touch it. Exposed primarily so that future work driving the
    /// runtime LoadImage / DMA-to-VRAM trace can replay the register
    /// state faithfully.
    pub fn set_texture_window(&self, mask_x: u8, mask_y: u8, off_x: u8, off_y: u8) {
        self.tex_window.set([
            (mask_x as u32) & 0x1F,
            (mask_y as u32) & 0x1F,
            (off_x as u32) & 0x1F,
            (off_y as u32) & 0x1F,
        ]);
    }

    /// Read the current `(mask_x, mask_y, off_x, off_y)` texture window
    /// register state, in 8-pixel steps (0..=31). All zero means no-op.
    pub fn texture_window(&self) -> [u32; 4] {
        self.tex_window.get()
    }

    /// Set the full-scene colour grade applied by the field mesh shaders:
    /// each shaded pixel is tone-mapped to `luminance * (gold_r, gold_g,
    /// gold_b)` and cross-faded to that by `strength`. `strength = 0.0`
    /// (the default) leaves the scene untouched; `1.0` is a full sepia
    /// collapse. Reproduces the opening prologue's gold/amber grade
    /// (the `opdeene` "It was the Seru." cutscene), which renders the whole
    /// 3D scene in warm monochrome while the narration text stays white.
    ///
    /// The retail grade measured off a VRAM capture averages RGB
    /// `(69, 62, 23)` with hue ≈ 50° and no surviving green/blue, i.e. a
    /// gold direction of `≈(1.0, 0.90, 0.33)` at near-full strength.
    pub fn set_color_grade(&self, gold: [f32; 3], strength: f32) {
        self.color_grade
            .set([gold[0], gold[1], gold[2], strength.clamp(0.0, 1.0)]);
    }

    /// Read the current colour grade `(gold_r, gold_g, gold_b, strength)`.
    pub fn color_grade(&self) -> [f32; 4] {
        self.color_grade.get()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, self.config.width, self.config.height);
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }
}
