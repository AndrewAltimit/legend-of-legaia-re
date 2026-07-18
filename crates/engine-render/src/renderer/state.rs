//! The [`Renderer`] struct field definitions plus its small state
//! accessors (PSX-mode toggle, texture-window register, resize, surface
//! size). Split out of `renderer/core.rs`.

use super::*;

/// Dynamic-lighting tunables (the opt-in enhancement - see
/// [`Renderer::set_dynamic_lighting`]). Kept as named constants so the look
/// is easy to iterate on; the WGSL-side weights (`DYN_DIFFUSE` / `DYN_POOL` /
/// `DYN_MAX_GAIN` / the pool geometry) live in the `dyn_light` helper in
/// `shaders.rs`.
///
/// Unit direction TOWARD the light, in mesh model space: mostly "up"
/// (TMD/PSX space is Y-down, so up is `-Y`) with a slight X/Z tilt so
/// differently-facing wall planes read at different brightness. The
/// orientation term is `|N.L|`, so the vertical component is sign-tolerant.
pub const DYN_LIGHT_DIR: [f32; 3] = [0.32, -0.89, 0.31];
/// Warm (slightly amber) light tint multiplied into the diffuse + pool
/// terms. Red-heavy on purpose - the reference look is "PSX game + modern
/// soft warm lighting", not a neutral studio light.
pub const DYN_LIGHT_TINT: [f32; 3] = [1.0, 0.93, 0.80];
/// Ambient floor: the gain a surface gets with no diffuse and no pool
/// contribution at all (a wall facing exactly along the light at a screen
/// corner). Keeps the enhancement a *shading*, not a blackout.
pub const DYN_LIGHT_AMBIENT: f32 = 0.55;

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
    /// GTE depth cue `(far_r, far_g, far_b, ir0)` staged into every field
    /// `MeshUniforms`. Defaults to all-zero = `ir0 = 0` = identity, which is
    /// what the field passes for an unfogged scene. Set with
    /// [`Renderer::set_depth_cue`].
    pub(super) depth_cue: std::cell::Cell<[f32; 4]>,
    /// View-depth IR0 ramp `(near_z, inv_range, max_ir0, enable)` staged into
    /// every field `MeshUniforms` (see [`super::uploaded::MeshUniforms`]).
    /// Defaults to all-zero = disabled (constant-`IR0` fallback). Set with
    /// [`Renderer::set_depth_cue_ramp`] / cleared with
    /// [`Renderer::clear_depth_cue_ramp`]; drives the opening prologue's
    /// per-render-node far-colour crush.
    pub(super) cue_ramp: std::cell::Cell<[f32; 4]>,
    /// Backface-cull mode staged into `MeshUniforms.flags[0]` (see there).
    /// `0.0` (default) = draw both sides; `1.0` / `2.0` = discard back /
    /// front-facing fragments. Set with [`Renderer::set_backface_cull`].
    pub(super) backface_cull: std::cell::Cell<f32>,
    /// PSX semi-transparency (ABE) blending, staged into `MeshUniforms.flags[1]`.
    /// When `true` (the default), a semi-transparent prim's blended fragments
    /// are deferred out of the opaque pass and re-drawn by the per-ABR-mode
    /// blend pass, so water / glass / additive effects composite with the
    /// framebuffer as they do on retail. When `false` those prims draw fully
    /// opaque. This is decoupled from [`Self::psx_mode`] on purpose: the
    /// modulation-correct blend is retail behaviour worth having in the clean
    /// "enhanced" render too, whereas the GTE vertex jitter / affine UVs /
    /// 15-bit dither that `psx_mode` also enables are strict-PS1 artefacts.
    /// Set with [`Renderer::set_semi_blend`].
    pub(super) semi_blend: std::cell::Cell<bool>,
    /// Opt-in dynamic-lighting enhancement, staged into
    /// `MeshUniforms.light_dir[3]`. `false` (the default) keeps every mesh
    /// path pixel-identical to the faithful baked-shading render - retail's
    /// field path has NO light source (see [`crate::psx_light`]), so this is
    /// explicitly a non-retail enhancement. When `true` the VRAM / colour
    /// mesh shaders layer a soft warm directional light (off the smoothed
    /// per-vertex normals) + a screen-space light pool over the baked
    /// colours, capped at ~1.3x. Set with [`Renderer::set_dynamic_lighting`].
    pub(super) dyn_lighting: std::cell::Cell<bool>,
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

    /// Toggle PSX semi-transparency (ABE) blending on the VRAM / colour mesh
    /// passes. When `true` (the default) a semi-transparent prim's blended
    /// texels are deferred out of the opaque pass and composited by the
    /// per-ABR-mode blend pass, so field water (e.g. the Hunter's Spring
    /// fountain), glass and additive effects show the framebuffer through
    /// them the way retail's GPU blend does. When `false` those prims draw
    /// fully opaque (the harsh-edged "solid water" look). Independent of
    /// [`Self::set_psx_mode`]: blending is correct in the clean render, so it
    /// is on regardless of the strict-PS1 jitter / dither knobs.
    pub fn set_semi_blend(&self, enable: bool) {
        self.semi_blend.set(enable);
    }

    /// Read the current semi-transparency-blend flag.
    pub fn semi_blend(&self) -> bool {
        self.semi_blend.get()
    }

    /// Toggle the opt-in **dynamic-lighting enhancement** on the VRAM /
    /// colour mesh passes. Off by default, and OFF IS RETAIL: the field
    /// path has no runtime light source (shading is baked into the TMD
    /// colour words - see [`crate::psx_light`]), so the disabled path is
    /// pixel-identical to the faithful render and the parity oracles are
    /// unaffected.
    ///
    /// When enabled, each fragment's baked colour is scaled by
    /// `ambient + (diffuse * |N.L| + pool) * warm_tint`, where `N` is the
    /// smoothed per-vertex normal already carried by the VRAM-mesh vertex
    /// format (with a screen-space-derivative fallback for the normal-less
    /// colour-mesh prims), `L` is [`DYN_LIGHT_DIR`], and `pool` is a soft
    /// screen-centred light pool - the "modern soft warm lighting over
    /// crisp PSX texels" look. The gain is capped at ~1.3x the baked
    /// brightness. Tunables: [`DYN_LIGHT_DIR`] / [`DYN_LIGHT_TINT`] /
    /// [`DYN_LIGHT_AMBIENT`] plus the `DYN_*` consts in the `dyn_light`
    /// WGSL helper.
    pub fn set_dynamic_lighting(&self, enable: bool) {
        self.dyn_lighting.set(enable);
    }

    /// Read the current dynamic-lighting flag.
    pub fn dynamic_lighting(&self) -> bool {
        self.dyn_lighting.get()
    }

    /// The `MeshUniforms.light_dir` word for the current frame:
    /// `[dir_x, dir_y, dir_z, enable]`. All-zero `w` = the identity
    /// (default off) path.
    pub(super) fn dyn_light_dir_uniform(&self) -> [f32; 4] {
        let on = if self.dyn_lighting.get() { 1.0 } else { 0.0 };
        [DYN_LIGHT_DIR[0], DYN_LIGHT_DIR[1], DYN_LIGHT_DIR[2], on]
    }

    /// The `MeshUniforms.light_color` word: `[tint_r, tint_g, tint_b,
    /// ambient]`. Constant; only read by the shader when the enable is set.
    pub(super) fn dyn_light_color_uniform(&self) -> [f32; 4] {
        [
            DYN_LIGHT_TINT[0],
            DYN_LIGHT_TINT[1],
            DYN_LIGHT_TINT[2],
            DYN_LIGHT_AMBIENT,
        ]
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
    /// each shaded pixel is cross-faded toward the per-channel multiply
    /// `rgb * (gold_r, gold_g, gold_b)` by `strength`. `strength = 0.0`
    /// (the default) leaves the scene untouched; `1.0` is the full tint.
    /// Reproduces the opening prologue's gold/amber grade (the `opdeene`
    /// "It was the Seru." cutscene), which renders the whole 3D scene in
    /// warm amber while the narration text stays white.
    ///
    /// A multiply matches the retail mechanism: the amber is baked into the
    /// drawn gouraud / texture-modulation colour words (measured on-geometry
    /// ratio ≈ `255:240:110`), while backdrop textures draw at neutral
    /// modulation and keep their pre-baked warm texel chroma - which a
    /// multiply preserves and a luminance collapse would flatten.
    pub fn set_color_grade(&self, gold: [f32; 3], strength: f32) {
        self.color_grade
            .set([gold[0], gold[1], gold[2], strength.clamp(0.0, 1.0)]);
    }

    /// Set the GTE **depth cue** the field mesh shaders apply after the
    /// texture-modulation pass - a port of `DPCS` (`cop2 0x780010`), the only
    /// colour op either retail TMD renderer (`FUN_8002735c`, `FUN_80029888`)
    /// executes.
    ///
    /// `far` is the GTE far colour (cr21-23) in `0..1`; `ir0` is the blend
    /// factor (`IR0`, hardware `0..0x1000`) in `0..1`. Each shaded pixel
    /// becomes `c + (far - c) * ir0`, so `ir0 = 0` (the default) is the
    /// identity. Retail sets both per drawn object; an unfogged field scene
    /// passes `ir0 = 0`, which is why a town0c capture shows the baked prim
    /// colours emerging from the GTE's RGB FIFO byte-unchanged.
    pub fn set_depth_cue(&self, far: [f32; 3], ir0: f32) {
        self.depth_cue.set([
            far[0].clamp(0.0, 1.0),
            far[1].clamp(0.0, 1.0),
            far[2].clamp(0.0, 1.0),
            ir0.clamp(0.0, 1.0),
        ]);
    }

    /// Read the current depth cue `(far_r, far_g, far_b, ir0)`.
    pub fn depth_cue(&self) -> [f32; 4] {
        self.depth_cue.get()
    }

    /// Stage the **per-render-node depth-cue pull** as a view-depth `IR0`
    /// ramp: far colour `far` (display `0..1`, written into the
    /// [`Self::set_depth_cue`] far slot) plus a linear ramp that takes each
    /// fragment's projected view depth to
    /// `ir0 = clamp((z - near_z) / (far_z - near_z), 0, 1) * max_ir0`.
    ///
    /// Retail stages the DPCS far colour and `IR0` per render node (`+0x74` /
    /// `+0x78` into GTE cr21-23 / `IR0`, `FUN_8002735C`); the cutscene host
    /// sets a gold far colour with depth-graded `IR0`s across the opening
    /// prologue so far scenery crushes toward gold while near ground keeps
    /// the modulation tint. The ramp reproduces that depth dependence
    /// per-fragment. The constant-`IR0` fog path ([`Self::set_depth_cue`]) is
    /// unaffected when the ramp is cleared.
    pub fn set_depth_cue_ramp(&self, far: [f32; 3], near_z: f32, far_z: f32, max_ir0: f32) {
        let cur = self.depth_cue.get();
        self.depth_cue.set([
            far[0].clamp(0.0, 1.0),
            far[1].clamp(0.0, 1.0),
            far[2].clamp(0.0, 1.0),
            cur[3],
        ]);
        let inv_range = if far_z > near_z {
            1.0 / (far_z - near_z)
        } else {
            0.0
        };
        self.cue_ramp
            .set([near_z, inv_range, max_ir0.clamp(0.0, 1.0), 1.0]);
    }

    /// Disable the view-depth `IR0` ramp (the default): the shaders fall back
    /// to the constant `IR0` staged by [`Self::set_depth_cue`], which is `0`
    /// (identity) unless a fog op set it.
    pub fn clear_depth_cue_ramp(&self) {
        self.cue_ramp.set([0.0, 0.0, 0.0, 0.0]);
    }

    /// Read the current cue ramp `(near_z, inv_range, max_ir0, enable)`.
    pub fn depth_cue_ramp(&self) -> [f32; 4] {
        self.cue_ramp.get()
    }

    /// Enable/disable the shader-side backface cull on the VRAM / colour
    /// mesh passes - the port of retail's GTE **NCLIP** screen-winding
    /// rejection (`FUN_8002735c` skips a prim whose projected winding is
    /// negative). The engine's rasterizer pipelines draw both sides
    /// (`cull_mode: None`) because winding parity differs per render frame
    /// (battle composes a per-model Y-flip, field a camera-side one), so
    /// this is a per-fragment discard keyed off `front_facing` instead:
    /// mode `1` discards back-facing fragments, `2` front-facing (pick the
    /// one that matches the active frame's parity), `0` (default) draws
    /// both sides.
    ///
    /// Without it a camera placed *inside* a closed mesh shell renders the
    /// shell's near wall over the whole scene - the opdeene prologue's
    /// crater-rim tableau shot sits inside the cave-wall backdrop mesh, and
    /// retail's NCLIP is what makes the near wall invisible.
    pub fn set_backface_cull(&self, mode: u32) {
        self.backface_cull.set(mode.min(2) as f32);
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
