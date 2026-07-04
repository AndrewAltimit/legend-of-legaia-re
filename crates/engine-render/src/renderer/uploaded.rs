//! GPU-resident data types: the uniform blocks, all `Uploaded*` GPU
//! resources, and the [`RenderTarget`] / [`Scene`] draw descriptors.
//! Split out of `renderer.rs`.

use super::*;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Uniforms {
    /// (scale_x, scale_y, _pad, _pad) - multiplied with the unit quad to
    /// produce final NDC coordinates. Set by the host based on window vs
    /// texture aspect ratio.
    pub(super) scale: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct MeshUniforms {
    pub(super) mvp: [[f32; 4]; 4],
    /// Direction the light is *coming from*, in world space, normalized.
    /// Stored as vec4 for std140 padding.
    pub(super) light_dir: [f32; 4],
    /// PSX-faithful rendering knobs:
    /// - `[0]` viewport width in pixels (used for the sub-pixel snap)
    /// - `[1]` viewport height in pixels
    /// - `[2]` `1.0` to snap clip-space x/y to integer pixels (PSX-style
    ///   "vertex jitter"); `0.0` for smooth modern subpixel positions
    /// - `[3]` `1.0` to ordered-dither the shaded colour to 15-bit BGR555
    ///   (PSX framebuffer depth); `0.0` for full-precision colour
    pub(super) psx_params: [f32; 4],
    /// GP0(0xE2) "Texture Window setting" - per-frame scene state.
    /// `[0..4]` = `(mask_x, mask_y, offset_x, offset_y)` each in 8-pixel
    /// steps (0..=31). The fragment shader applies, per texel:
    ///   `tex_x = (tex_x AND NOT (mask_x*8)) OR ((offset_x AND mask_x)*8)`
    ///   (and the same for Y), which clamps texture sampling to a smaller
    ///   window inside the texture page. No-op when all zero.
    ///
    /// Set with [`Renderer::set_texture_window`]. Defaults to all-zero so
    /// existing callers aren't affected.
    pub(super) tex_window: [u32; 4],
    /// Full-scene colour grade - `(gold_r, gold_g, gold_b, strength)`. The
    /// textured / VRAM / colour fragment shaders tone-map the shaded pixel to
    /// `luminance * gold` and cross-fade to it by `strength` (`0.0` = no grade,
    /// the default; `1.0` = full sepia). Drives the opening prologue's
    /// gold/amber sepia grade (`opdeene` cutscene) without touching the text /
    /// UI overlays (they use separate shaders). Set with
    /// [`Renderer::set_color_grade`]. Defaults to `(1, 1, 1, 0)` = identity.
    pub(super) grade: [f32; 4],
}

pub struct UploadedTexture {
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl UploadedTexture {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// A 3D mesh uploaded to the GPU. Built from
/// `(positions: Vec<[f32;3]>, indices: Vec<u32>)` where indices form
/// independent triangles (3 indices = 1 tri). Per-vertex normals are
/// computed at upload time by averaging adjacent face normals - adequate
/// for the TMD viewer where the source format only stores per-object
/// normals (not per-vertex).
pub struct UploadedMesh {
    pub(super) vertex_buf: wgpu::Buffer,
    pub(super) index_buf: wgpu::Buffer,
    pub(super) index_count: u32,
}

impl UploadedMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }
}

/// GPU-resident textured mesh: position+UV per vertex (interleaved), index
/// buffer, plus a bind-group holding the texture+sampler used to paint it.
/// Uploaded by [`Renderer::upload_textured_mesh`].
pub struct UploadedTexturedMesh {
    pub(super) vertex_buf: wgpu::Buffer,
    pub(super) index_buf: wgpu::Buffer,
    pub(super) index_count: u32,
}

impl UploadedTexturedMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }
}

/// GPU-resident PSX VRAM (1024×512 R16Uint texture). Built by
/// [`Renderer::upload_vram`] from a CPU-side [`Vram`]. Bound at @group(2)
/// of the VRAM-mesh pipeline so the fragment shader can do faithful PSX
/// texture-page + CLUT lookups (4bpp / 8bpp / 15bpp) using each prim's
/// per-vertex CBA + TSB.
pub struct UploadedVram {
    pub(super) bind_group: wgpu::BindGroup,
    /// Monotonic per-[`Renderer`] upload stamp. Lets a host that expects a
    /// specific VRAM upload to stay GPU-resident (e.g. the battle texture
    /// across battle frames) detect that another path re-uploaded over it.
    pub(super) generation: u64,
}

impl UploadedVram {
    /// The renderer-wide upload stamp this texture was created under.
    /// Strictly increasing across [`Renderer::upload_vram`] calls.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// VRAM-mesh: position + per-vertex UV (u8) + per-vertex CBA + TSB (u16).
/// Combined with an [`UploadedVram`] this lets the fragment shader do a
/// proper PSX texture lookup that selects the right texture page + CLUT
/// per primitive, regardless of which TIM the texels came from.
pub struct UploadedVramMesh {
    pub(super) vertex_buf: wgpu::Buffer,
    pub(super) index_buf: wgpu::Buffer,
    pub(super) index_count: u32,
    /// `[(first_index, index_count); 4]` ranges of the per-ABR-mode
    /// semi-transparent "tail" appended past `index_count` in `index_buf`
    /// (see [`psx_blend::append_semi_tail`]). Describes the tail layout;
    /// all-zero counts when the mesh has no semi prims.
    pub(super) semi_ranges: [(u32, u32); 4],
    /// Per-prim blend-pass metadata, one entry per semi-transparent
    /// triangle in original mesh order (see [`psx_blend::SemiPrim`]). The
    /// PSX-faithful blend pass re-orders these per frame by projected
    /// depth, mirroring the retail ordering table.
    pub(super) semi_prims: Vec<psx_blend::SemiPrim>,
}

impl UploadedVramMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }

    /// Per-ABR-mode index ranges of the semi-transparent tail (see
    /// [`psx_blend::append_semi_tail`]).
    pub fn semi_ranges(&self) -> [(u32, u32); 4] {
        self.semi_ranges
    }

    /// Per-prim blend-pass metadata in original mesh order (see
    /// [`psx_blend::SemiPrim`]).
    pub fn semi_prims(&self) -> &[psx_blend::SemiPrim] {
        &self.semi_prims
    }

    /// True when any prim in this mesh carries the semi-transparency enable.
    pub fn has_semi_prims(&self) -> bool {
        !self.semi_prims.is_empty()
    }
}

/// GPU-resident **untextured** triangle mesh: per-vertex position + RGB colour,
/// flat face-shaded with no VRAM lookup. Built by [`Renderer::upload_color_mesh`]
/// from a [`legaia_tmd::mesh::ColorMesh`] for the `F*`/`G*` props whose prims
/// carry per-vertex colours instead of UVs (which the textured VRAM-mesh path
/// drops).
pub struct UploadedColorMesh {
    pub(super) vertex_buf: wgpu::Buffer,
    pub(super) index_buf: wgpu::Buffer,
    pub(super) index_count: u32,
    /// `[(first_index, index_count); 4]` ranges of the per-ABR-mode
    /// semi-transparent "tail" appended past `index_count` in `index_buf`
    /// (see [`psx_blend::append_semi_tail_words`]). Describes the tail
    /// layout; all-zero counts when the mesh has no semi prims (always the
    /// case via [`Renderer::upload_color_mesh`] - blend words come in
    /// through [`Renderer::upload_color_mesh_blended`]).
    pub(super) semi_ranges: [(u32, u32); 4],
    /// Per-prim blend-pass metadata, one entry per semi-transparent
    /// triangle in original mesh order (see [`psx_blend::SemiPrim`]). The
    /// PSX-faithful blend pass re-orders these per frame by projected
    /// depth, mirroring the retail ordering table.
    pub(super) semi_prims: Vec<psx_blend::SemiPrim>,
}

impl UploadedColorMesh {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn triangle_count(&self) -> u32 {
        self.index_count / 3
    }

    /// Per-ABR-mode index ranges of the semi-transparent tail (see
    /// [`psx_blend::append_semi_tail_words`]).
    pub fn semi_ranges(&self) -> [(u32, u32); 4] {
        self.semi_ranges
    }

    /// Per-prim blend-pass metadata in original mesh order (see
    /// [`psx_blend::SemiPrim`]).
    pub fn semi_prims(&self) -> &[psx_blend::SemiPrim] {
        &self.semi_prims
    }

    /// True when any prim in this mesh carries the semi-transparency enable.
    pub fn has_semi_prims(&self) -> bool {
        !self.semi_prims.is_empty()
    }
}

/// GPU-resident font atlas. Built by [`Renderer::upload_font_atlas`] from a
/// pre-decoded RGBA8 buffer. Used as the texture binding for the 2D text
/// pipeline.
pub struct UploadedFontAtlas {
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl UploadedFontAtlas {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// A wireframe line mesh: position + per-vertex RGB color, drawn as
/// `LineList` (every pair of indices is one line segment). Unlit and
/// depth-tested. Used by the stage-geometry viewer.
pub struct UploadedLines {
    pub(super) vertex_buf: wgpu::Buffer,
    pub(super) index_buf: wgpu::Buffer,
    pub(super) index_count: u32,
}

impl UploadedLines {
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn line_count(&self) -> u32 {
        self.index_count / 2
    }
}

/// What to draw this frame.
pub enum RenderTarget<'a> {
    /// Clear-only - the PROT browser uses this for entries with no preview.
    Clear,
    /// 2D textured quad (TIM viewer, sprite previews).
    Texture(&'a UploadedTexture),
    /// 3D triangulated mesh (TMD viewer). `mvp` is the full
    /// model-view-projection matrix, supplied per-frame so the host can
    /// drive rotation/zoom without re-uploading geometry.
    Mesh { mesh: &'a UploadedMesh, mvp: Mat4 },
    /// 3D textured mesh: same as `Mesh` but samples a bound texture using
    /// per-vertex UVs. Used by the TMD viewer when a paired TIM is found.
    TexturedMesh {
        mesh: &'a UploadedTexturedMesh,
        texture: &'a UploadedTexture,
        mvp: Mat4,
    },
    /// 3D mesh with full PSX VRAM emulation: per-vertex UV/CBA/TSB
    /// addresses into a 1024×512 software VRAM, with 4/8/15bpp + CLUT
    /// decode in the fragment shader.
    VramMesh {
        mesh: &'a UploadedVramMesh,
        vram: &'a UploadedVram,
        mvp: Mat4,
    },
    /// Wireframe line mesh (stage-geometry viewer). Same depth-tested 3D
    /// pipeline; per-vertex color, no lighting.
    Lines { mesh: &'a UploadedLines, mvp: Mat4 },
    /// Multi-actor scene: N VRAM meshes drawn in one render pass with a
    /// shared VRAM and per-actor MVPs. Optionally overlays a single
    /// wireframe-lines mesh (stage geometry / debug grid) drawn after the
    /// solid actors.
    ///
    /// Used by the `world` viewer to render every active actor in the
    /// World composite per frame.
    Scene(&'a Scene<'a>),
    /// 2D text-only frame: clear to background, then draw a single
    /// [`TextOverlay`]. Used by the dialog viewer / any UI mode that
    /// has no 3D scene to render.
    TextOnly(&'a TextOverlay<'a>),
}

/// Per-actor draw inside a [`Scene`].
pub struct SceneDraw<'a> {
    pub mesh: &'a UploadedVramMesh,
    pub mvp: Mat4,
}

/// An untextured, vertex-coloured mesh draw inside a [`Scene`] (the `F*`/`G*`
/// props). Drawn after the textured [`SceneDraw`]s, on the same depth buffer.
pub struct ColorSceneDraw<'a> {
    pub mesh: &'a UploadedColorMesh,
    pub mvp: Mat4,
}

/// Multi-actor scene payload. Drawn against a single shared VRAM with one
/// MVP per actor. Optionally overlays a [`UploadedLines`] mesh (e.g. a
/// stage-geometry wireframe) using the supplied MVP, and/or a 2D text
/// batch (HUD / debug text / dialog).
pub struct Scene<'a> {
    pub vram: &'a UploadedVram,
    pub draws: &'a [SceneDraw<'a>],
    /// Untextured vertex-coloured meshes (`F*`/`G*` props), drawn after
    /// [`Self::draws`] on the shared depth buffer. Usually empty.
    pub color_draws: &'a [ColorSceneDraw<'a>],
    pub overlay_lines: Option<(&'a UploadedLines, Mat4)>,
    /// 2D sprite batch drawn after the 3D meshes and lines, before
    /// [`Self::overlay_text`]. Used by the actor sprite pipeline.
    pub overlay_sprites: Option<&'a SpriteOverlay<'a>>,
    pub overlay_text: Option<&'a TextOverlay<'a>>,
    /// Optional second sprite-overlay slot drawn between
    /// [`Self::overlay_sprites`] and [`Self::overlay_text`]. Used when
    /// two distinct sprite atlases need to render on the same frame
    /// (e.g. title-art bands + menu-glyph atlas during the title
    /// menu phase).
    pub overlay_sprites_2: Option<&'a SpriteOverlay<'a>>,
    /// Optional override of the surface clear colour (linear RGBA). When
    /// `None` the renderer falls back to its default dark-grey clear.
    /// Used during the boot publisher-logos phase to force pure black.
    pub clear_color: Option<[f32; 4]>,
}
