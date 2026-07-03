//! GPU-resident render resources ([`UploadedTexture`], [`UploadedMesh`],
//! [`UploadedVram`], ...) and the [`Renderer`] pipeline host. Extracted
//! from the crate root; see the crate-level docs for the pipeline overview.

use crate::shaders::*;
use crate::*;
use anyhow::{Context, Result};
use glam::Mat4;
use legaia_tim::{VRAM_HEIGHT, VRAM_WIDTH, Vram};
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    /// (scale_x, scale_y, _pad, _pad) - multiplied with the unit quad to
    /// produce final NDC coordinates. Set by the host based on window vs
    /// texture aspect ratio.
    scale: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshUniforms {
    mvp: [[f32; 4]; 4],
    /// Direction the light is *coming from*, in world space, normalized.
    /// Stored as vec4 for std140 padding.
    light_dir: [f32; 4],
    /// PSX-faithful rendering knobs:
    /// - `[0]` viewport width in pixels (used for the sub-pixel snap)
    /// - `[1]` viewport height in pixels
    /// - `[2]` `1.0` to snap clip-space x/y to integer pixels (PSX-style
    ///   "vertex jitter"); `0.0` for smooth modern subpixel positions
    /// - `[3]` `1.0` to ordered-dither the shaded colour to 15-bit BGR555
    ///   (PSX framebuffer depth); `0.0` for full-precision colour
    psx_params: [f32; 4],
    /// GP0(0xE2) "Texture Window setting" - per-frame scene state.
    /// `[0..4]` = `(mask_x, mask_y, offset_x, offset_y)` each in 8-pixel
    /// steps (0..=31). The fragment shader applies, per texel:
    ///   `tex_x = (tex_x AND NOT (mask_x*8)) OR ((offset_x AND mask_x)*8)`
    ///   (and the same for Y), which clamps texture sampling to a smaller
    ///   window inside the texture page. No-op when all zero.
    ///
    /// Set with [`Renderer::set_texture_window`]. Defaults to all-zero so
    /// existing callers aren't affected.
    tex_window: [u32; 4],
}

pub struct UploadedTexture {
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
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
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
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
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
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
    bind_group: wgpu::BindGroup,
    /// Monotonic per-[`Renderer`] upload stamp. Lets a host that expects a
    /// specific VRAM upload to stay GPU-resident (e.g. the battle texture
    /// across battle frames) detect that another path re-uploaded over it.
    generation: u64,
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
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    /// `[(first_index, index_count); 4]` ranges of the per-ABR-mode
    /// semi-transparent "tail" appended past `index_count` in `index_buf`
    /// (see [`psx_blend::append_semi_tail`]). Describes the tail layout;
    /// all-zero counts when the mesh has no semi prims.
    semi_ranges: [(u32, u32); 4],
    /// Per-prim blend-pass metadata, one entry per semi-transparent
    /// triangle in original mesh order (see [`psx_blend::SemiPrim`]). The
    /// PSX-faithful blend pass re-orders these per frame by projected
    /// depth, mirroring the retail ordering table.
    semi_prims: Vec<psx_blend::SemiPrim>,
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
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    /// `[(first_index, index_count); 4]` ranges of the per-ABR-mode
    /// semi-transparent "tail" appended past `index_count` in `index_buf`
    /// (see [`psx_blend::append_semi_tail_words`]). Describes the tail
    /// layout; all-zero counts when the mesh has no semi prims (always the
    /// case via [`Renderer::upload_color_mesh`] - blend words come in
    /// through [`Renderer::upload_color_mesh_blended`]).
    semi_ranges: [(u32, u32); 4],
    /// Per-prim blend-pass metadata, one entry per semi-transparent
    /// triangle in original mesh order (see [`psx_blend::SemiPrim`]). The
    /// PSX-faithful blend pass re-orders these per frame by projected
    /// depth, mirroring the retail ordering table.
    semi_prims: Vec<psx_blend::SemiPrim>,
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
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
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
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
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

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Quad pipeline (Phase 1 TIM viewer).
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    uniforms_buf: wgpu::Buffer,
    uniforms_bg: wgpu::BindGroup,
    /// Mesh pipeline (Phase 1 TMD viewer).
    mesh_pipeline: wgpu::RenderPipeline,
    mesh_uniforms_buf: wgpu::Buffer,
    mesh_uniforms_bg: wgpu::BindGroup,
    /// Textured-mesh pipeline (Phase 1 TMD viewer with paired TIM).
    /// Reuses [`Self::bind_group_layout`] for the per-mesh texture binding.
    textured_mesh_pipeline: wgpu::RenderPipeline,
    /// VRAM-mesh pipeline: per-vertex CBA/TSB + R16Uint VRAM texture lookup.
    vram_mesh_pipeline: wgpu::RenderPipeline,
    /// PSX semi-transparency blend pipelines for [`Self::vram_mesh_pipeline`],
    /// one per ABR mode 0..=3 (see [`psx_blend`]). Drawn as a second pass
    /// over each mesh's semi-transparent index tail when PSX-faithful mode
    /// is on ([`Self::set_psx_mode`]).
    vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    vram_bgl: wgpu::BindGroupLayout,
    /// Multi-actor "scene" VRAM-mesh pipeline. Identical to
    /// [`Self::vram_mesh_pipeline`] but binds [`Self::scene_uniforms_bgl`]
    /// at group 0 (with `has_dynamic_offset = true`) so a single render
    /// pass can draw N actors with one bind group + N dynamic offsets.
    scene_vram_mesh_pipeline: wgpu::RenderPipeline,
    /// Scene-layout twins of [`Self::vram_mesh_blend_pipelines`].
    scene_vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    /// Lines pipeline shadowing the scene path (uses the scene-uniforms
    /// dynamic-offset layout). Used by [`Self::render`] when a `Scene`
    /// carries `overlay_lines`.
    scene_lines_pipeline: wgpu::RenderPipeline,
    /// Untextured vertex-colour mesh pipeline shadowing the scene path (same
    /// scene-uniforms dynamic-offset layout, no VRAM group). Draws a `Scene`'s
    /// `color_draws` (`F*`/`G*` props).
    scene_color_mesh_pipeline: wgpu::RenderPipeline,
    /// PSX semi-transparency blend pipelines for
    /// [`Self::scene_color_mesh_pipeline`], one per ABR mode 0..=3 (see
    /// [`psx_blend`]). Unlike the textured blend pass there is no per-texel
    /// STP gate: an untextured ABE prim blends *all* its pixels, so the
    /// fragment entries just emit the interpolated vertex colour (mode 3
    /// pre-scaled by 0.25) and the fixed-function blend state does the rest.
    scene_color_mesh_blend_pipelines: [wgpu::RenderPipeline; 4],
    scene_uniforms_bgl: wgpu::BindGroupLayout,
    scene_uniforms_bg: std::cell::RefCell<wgpu::BindGroup>,
    scene_uniforms_buf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity (in `MeshUniforms` slots) of [`Self::scene_uniforms_buf`].
    scene_uniforms_capacity: std::cell::Cell<usize>,
    /// `min_uniform_buffer_offset_alignment` reported by the adapter.
    /// Per-actor uniform writes are padded up to this stride.
    uniform_offset_alignment: u32,
    /// Lines pipeline: LineList topology, per-vertex color, depth-tested.
    /// Used for wireframe rendering of stage geometry.
    lines_pipeline: wgpu::RenderPipeline,
    /// Text pipeline: 2D textured quads, alpha-blended, no depth. Group 0
    /// binds a sampled font atlas. Used for HUD / debug / dialog overlays.
    text_pipeline: wgpu::RenderPipeline,
    /// Bind-group layout for the font-atlas texture binding (group 0 of
    /// [`Self::text_pipeline`]). Reused when uploading new atlases.
    text_atlas_bgl: wgpu::BindGroupLayout,
    /// Sampler used by the text pipeline. Nearest-neighbour to keep PSX
    /// pixel-art glyphs crisp.
    text_sampler: wgpu::Sampler,
    /// Per-frame text vertex / index buffers (RefCell-borrowed from the
    /// non-mut `render` API; resized geometrically on demand).
    text_vbuf: std::cell::RefCell<wgpu::Buffer>,
    text_ibuf: std::cell::RefCell<wgpu::Buffer>,
    /// Capacity of [`Self::text_vbuf`] in vertex slots and
    /// [`Self::text_ibuf`] in index slots. Both grow together - one quad
    /// per `TextDraw` is 4 vertices and 6 indices.
    text_vertex_capacity: std::cell::Cell<u32>,
    text_index_capacity: std::cell::Cell<u32>,
    /// Per-overlay quad ranges from the most recent staging call -
    /// `[(base_quad, count), ...]` in the same order overlays were passed.
    /// Drained by the in-pass draw to issue one `draw_indexed` per overlay
    /// with the matching atlas bound.
    scene_quad_ranges: std::cell::RefCell<Vec<(u32, u32)>>,
    /// Per-frame blend-pass ordering list (see
    /// [`psx_blend::BlendListEntry`]). RefCell-borrowed from the non-mut
    /// `render` API; cleared and refilled each frame, capacity persists so
    /// the per-prim sort allocates nothing in steady state.
    blend_list: std::cell::RefCell<Vec<psx_blend::BlendListEntry>>,
    /// Depth target - recreated on resize.
    depth_view: wgpu::TextureView,
    /// PSX-faithful rendering mode. When `true`, the VRAM-mesh shader uses
    /// affine (linear-in-screen-space) UV interpolation instead of
    /// perspective-correct, and snaps clip-space x/y to integer pixel
    /// positions to reproduce the GTE's per-vertex sub-pixel-truncation
    /// "vertex jitter." Default `false` for clean smooth rendering.
    psx_mode: std::cell::Cell<bool>,
    /// Count of [`Self::upload_vram`] calls; stamps each [`UploadedVram`]
    /// with a strictly-increasing generation (see
    /// [`UploadedVram::generation`]).
    vram_upload_counter: std::cell::Cell<u64>,
    /// GP0(0xE2) "Texture Window setting" - `(mask_x, mask_y, off_x, off_y)`
    /// each in 8-pixel steps (0..=31). Applied per-fragment in the
    /// VRAM-mesh shader. Defaults to all-zero (no-op), which matches
    /// retail Legaia's typical state - the register only gets non-zero
    /// values from a handful of effect / scene-init scripts.
    tex_window: std::cell::Cell<[u32; 4]>,
}

impl Renderer {
    /// Constructs a renderer attached to a winit-style window. Caller passes
    /// an `Arc<Window>` so the Surface can outlive the borrow.
    pub fn new<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: wgpu::WindowHandle + 'static,
    {
        pollster::block_on(Self::new_async(window, width, height))
    }

    async fn new_async<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: wgpu::WindowHandle + 'static,
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window)
            .context("create wgpu surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("legaia engine device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .context("request device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia textured quad shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniforms_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniforms bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniforms_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniforms"),
            contents: bytemuck::cast_slice(&[Uniforms {
                scale: [1.0, 1.0, 0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniforms bg"),
            layout: &uniforms_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia pipeline layout"),
            bind_group_layouts: &[&bind_group_layout, &uniforms_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia textured quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Mesh pipeline: 3D triangle list, depth-tested, single directional light.
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(MESH_SHADER_SRC).into()),
        });
        let mesh_uniforms_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mesh uniforms bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let mesh_uniforms_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh uniforms"),
            contents: bytemuck::cast_slice(&[MeshUniforms {
                mvp: Mat4::IDENTITY.to_cols_array_2d(),
                light_dir: [0.4, -0.8, 0.4, 0.0],
                psx_params: [width as f32, height as f32, 0.0, 0.0],
                tex_window: [0; 4],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let mesh_uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh uniforms bg"),
            layout: &mesh_uniforms_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: mesh_uniforms_buf.as_entire_binding(),
            }],
        });
        let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl],
            push_constant_ranges: &[],
        });
        // Vertex layout: 3 floats position. Normals are computed in the shader
        // from screen-space derivatives - no per-vertex normal needed, which
        // keeps the upload format dead-simple for the source TMDs (which only
        // store per-object normals, not per-vertex).
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        };
        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia mesh pipeline"),
            layout: Some(&mesh_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Textured-mesh pipeline: same depth + MVP path as the flat mesh
        // pipeline, but with a per-vertex UV attribute and a fragment shader
        // that samples a bound texture. Reuses `bind_group_layout` (the
        // texture+sampler layout from the quad pipeline) at group 1.
        let textured_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia textured mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(TEXTURED_MESH_SHADER_SRC).into()),
        });
        let textured_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia textured mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl, &bind_group_layout],
            push_constant_ranges: &[],
        });
        let textured_vertex_layout = wgpu::VertexBufferLayout {
            // 3 floats position + 2 floats UV = 20 bytes.
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        let textured_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia textured mesh pipeline"),
                layout: Some(&textured_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &textured_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[textured_vertex_layout],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &textured_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // VRAM-mesh pipeline: per-vertex (UV, CBA, TSB) + a 1024×512 R16Uint
        // texture holding the whole PSX VRAM. The fragment shader does its
        // own page+CLUT lookup so a single mesh can sample multiple texture
        // pages and palettes correctly.
        let vram_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("psx vram bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Uint,
                },
                count: None,
            }],
        });
        let vram_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia vram mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(VRAM_MESH_SHADER_SRC).into()),
        });
        let vram_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia vram mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl, &vram_bgl],
            push_constant_ranges: &[],
        });
        // 12 (pos) + 4 (uv as Uint8x4) + 4 (cba/tsb as Uint16x2) + 12
        // (normal as Float32x3) = 32 bytes
        let vram_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint8x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Uint16x2,
                },
                wgpu::VertexAttribute {
                    offset: 20,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        };
        let vram_mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia vram mesh pipeline"),
            layout: Some(&vram_mesh_layout),
            vertex: wgpu::VertexState {
                module: &vram_mesh_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&vram_vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &vram_mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wireframe lines pipeline: LineList topology, per-vertex color,
        // depth-tested. Reuses `mesh_uniforms_bgl` for the MVP. Per-vertex
        // layout = 12 (position) + 4 (color as Uint8x4) = 16 bytes.
        let lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia lines shader"),
            source: wgpu::ShaderSource::Wgsl(LINES_SHADER_SRC.into()),
        });
        let lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia lines pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl],
            push_constant_ranges: &[],
        });
        let lines_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Unorm8x4,
                },
            ],
        };
        let lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia lines pipeline"),
            layout: Some(&lines_layout),
            vertex: wgpu::VertexState {
                module: &lines_shader,
                entry_point: Some("vs_main"),
                buffers: &[lines_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &lines_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Scene-uniforms layout: a single dynamic-offset uniform buffer
        // holding N `MeshUniforms` slots, each `uniform_offset_alignment`
        // bytes apart. Reused for the multi-actor VRAM-mesh and lines
        // pipelines below.
        let uniform_offset_alignment = device.limits().min_uniform_buffer_offset_alignment.max(256);
        let scene_uniforms_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene mesh uniforms bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(
                            std::mem::size_of::<MeshUniforms>() as u64,
                        ),
                    },
                    count: None,
                }],
            });
        // Initial capacity: one slot. Grown on demand by render_scene.
        let initial_scene_capacity: usize = 1;
        let scene_uniforms_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene mesh uniforms"),
            size: (initial_scene_capacity * uniform_offset_alignment as usize) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene mesh uniforms bg"),
            layout: &scene_uniforms_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &scene_uniforms_buf,
                    offset: 0,
                    size: std::num::NonZeroU64::new(std::mem::size_of::<MeshUniforms>() as u64),
                }),
            }],
        });

        let scene_vram_mesh_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("legaia scene vram mesh pipeline layout"),
                bind_group_layouts: &[&scene_uniforms_bgl, &vram_bgl],
                push_constant_ranges: &[],
            });
        let scene_vram_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia scene vram mesh pipeline"),
                layout: Some(&scene_vram_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &vram_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                            wgpu::VertexAttribute {
                                offset: 12,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Uint8x4,
                            },
                            wgpu::VertexAttribute {
                                offset: 16,
                                shader_location: 2,
                                format: wgpu::VertexFormat::Uint16x2,
                            },
                            wgpu::VertexAttribute {
                                offset: 20,
                                shader_location: 3,
                                format: wgpu::VertexFormat::Float32x3,
                            },
                        ],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &vram_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // PSX semi-transparency blend pipelines: one per ABR mode, for both
        // the single-mesh layout and the scene (dynamic-offset) layout. Same
        // shader module + vertex state as the opaque VRAM-mesh pipelines;
        // the blend-pass fragment entry keeps only STP texels and the
        // per-mode fixed-function [`psx_blend::blend_state`] applies the PSX
        // equation (mode 3 pre-scales F by 0.25 via its own entry point).
        // Depth: test against the opaque pass but don't write (the PSX has
        // no depth buffer and blended fragments must not occlude later
        // draws); LessEqual so decal prims coplanar with already-drawn
        // geometry aren't z-rejected.
        let make_blend_pipeline = |label: &'static str,
                                   layout: &wgpu::PipelineLayout,
                                   mode: u8|
         -> wgpu::RenderPipeline {
            let entry = if psx_blend::src_shader_scale(mode) == 1.0 {
                "fs_blend"
            } else {
                "fs_blend_quarter"
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &vram_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&vram_vertex_layout),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &vram_mesh_shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(psx_blend::blend_state(mode)),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
            make_blend_pipeline(
                "legaia vram mesh blend pipeline",
                &vram_mesh_layout,
                m as u8,
            )
        });
        let scene_vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
            make_blend_pipeline(
                "legaia scene vram mesh blend pipeline",
                &scene_vram_mesh_layout,
                m as u8,
            )
        });

        let scene_lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia scene lines pipeline layout"),
            bind_group_layouts: &[&scene_uniforms_bgl],
            push_constant_ranges: &[],
        });
        let scene_lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia scene lines pipeline"),
            layout: Some(&scene_lines_layout),
            vertex: wgpu::VertexState {
                module: &lines_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x3,
                        },
                        wgpu::VertexAttribute {
                            offset: 12,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Unorm8x4,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &lines_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Vertex-colour mesh pipeline (untextured F*/G* props): same scene-
        // uniforms dynamic-offset layout as the lines pipeline (group 0 only,
        // no VRAM), TriangleList, position(12) + Unorm8x4 colour(4) +
        // Uint32 blend word(4) = 20 bytes. The blend word carries the prim's
        // ABE/ABR state in the low 16 bits ([`psx_blend::pack_blend_word`]).
        let color_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia color mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(COLOR_MESH_SHADER_SRC).into()),
        });
        let scene_color_mesh_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("legaia scene color mesh pipeline layout"),
                bind_group_layouts: &[&scene_uniforms_bgl],
                push_constant_ranges: &[],
            });
        let color_mesh_attributes = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Unorm8x4,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Uint32,
            },
        ];
        let color_mesh_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 20,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &color_mesh_attributes,
        };
        let scene_color_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia scene color mesh pipeline"),
                layout: Some(&scene_color_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &color_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&color_mesh_vertex_layout),
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &color_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        // PSX semi-transparency blend pipelines for the colour-mesh path,
        // one per ABR mode. Same shader module + vertex layout as the opaque
        // colour pipeline; the blend-pass fragment entries emit the prim
        // colour (mode 3 pre-scales by 0.25) and the per-mode fixed-function
        // [`psx_blend::blend_state`] applies the PSX equation. Unlike the
        // VRAM-mesh blend pass there is no STP discard - an untextured ABE
        // prim blends every pixel. Depth: LessEqual without writing, like
        // the textured blend pass.
        let scene_color_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] =
            std::array::from_fn(|m| {
                let mode = m as u8;
                let entry = if psx_blend::src_shader_scale(mode) == 1.0 {
                    "fs_blend"
                } else {
                    "fs_blend_quarter"
                };
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("legaia scene color mesh blend pipeline"),
                    layout: Some(&scene_color_mesh_layout),
                    vertex: wgpu::VertexState {
                        module: &color_mesh_shader,
                        entry_point: Some("vs_main"),
                        buffers: std::slice::from_ref(&color_mesh_vertex_layout),
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &color_mesh_shader,
                        entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: config.format,
                            blend: Some(psx_blend::blend_state(mode)),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: false,
                        depth_compare: wgpu::CompareFunction::LessEqual,
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                })
            });

        // Text pipeline: 2D textured quads in NDC, alpha blended, no depth.
        // Vertex layout = 8 (pos: Float32x2) + 8 (uv: Float32x2) +
        // 16 (color: Float32x4) = 32 bytes.
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia text shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER_SRC.into()),
        });
        let text_atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("text atlas bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let text_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text atlas sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia text pipeline layout"),
            bind_group_layouts: &[&text_atlas_bgl],
            push_constant_ranges: &[],
        });
        let text_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia text pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_main"),
                buffers: &[text_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Scene render pass binds a depth attachment; every pipeline used
            // in that pass must declare a matching depth-stencil format.
            // Text never reads or writes depth - `Always` + write disabled
            // keeps it a pure overlay pass.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let initial_text_quads: u32 = 64;
        let text_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text vertex buffer"),
            size: (initial_text_quads as u64) * 4 * 32,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let text_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("text index buffer"),
            size: (initial_text_quads as u64) * 6 * 4,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let depth_view = create_depth_view(&device, config.width, config.height);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            sampler,
            bind_group_layout,
            uniforms_buf,
            uniforms_bg,
            mesh_pipeline,
            mesh_uniforms_buf,
            mesh_uniforms_bg,
            textured_mesh_pipeline,
            vram_mesh_pipeline,
            vram_mesh_blend_pipelines,
            vram_bgl,
            scene_vram_mesh_pipeline,
            scene_vram_mesh_blend_pipelines,
            scene_lines_pipeline,
            scene_color_mesh_pipeline,
            scene_color_mesh_blend_pipelines,
            scene_uniforms_bgl,
            scene_uniforms_bg: std::cell::RefCell::new(scene_uniforms_bg),
            scene_uniforms_buf: std::cell::RefCell::new(scene_uniforms_buf),
            scene_uniforms_capacity: std::cell::Cell::new(initial_scene_capacity),
            uniform_offset_alignment,
            lines_pipeline,
            text_pipeline,
            text_atlas_bgl,
            text_sampler,
            text_vbuf: std::cell::RefCell::new(text_vbuf),
            text_ibuf: std::cell::RefCell::new(text_ibuf),
            text_vertex_capacity: std::cell::Cell::new(initial_text_quads * 4),
            text_index_capacity: std::cell::Cell::new(initial_text_quads * 6),
            scene_quad_ranges: std::cell::RefCell::new(Vec::new()),
            blend_list: std::cell::RefCell::new(Vec::new()),
            depth_view,
            psx_mode: std::cell::Cell::new(false),
            vram_upload_counter: std::cell::Cell::new(0),
            tex_window: std::cell::Cell::new([0; 4]),
        })
    }

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

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, self.config.width, self.config.height);
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn upload_mesh(&self, positions: &[[f32; 3]], indices: &[u32]) -> Result<UploadedMesh> {
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!("mesh index count {} is not a multiple of 3", indices.len());
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!("mesh index {} >= vertex count {}", max_idx, positions.len());
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh vertex buffer"),
                contents: bytemuck::cast_slice(positions),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a textured mesh: positions + UVs (paired by index, same length)
    /// plus triangle indices. Vertex+UV data is interleaved as `[x,y,z,u,v]`
    /// so it matches the textured-mesh pipeline's vertex layout in one buffer.
    pub fn upload_textured_mesh(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[f32; 2]],
        indices: &[u32],
    ) -> Result<UploadedTexturedMesh> {
        if positions.len() != uvs.len() {
            anyhow::bail!(
                "textured mesh: positions ({}) and uvs ({}) length mismatch",
                positions.len(),
                uvs.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "textured mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "textured mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        // Interleave: [x,y,z,u,v] per vertex (5 f32 = 20 bytes, matches the
        // pipeline's 20-byte stride).
        let mut interleaved = Vec::with_capacity(positions.len() * 5);
        for (p, uv) in positions.iter().zip(uvs.iter()) {
            interleaved.push(p[0]);
            interleaved.push(p[1]);
            interleaved.push(p[2]);
            interleaved.push(uv[0]);
            interleaved.push(uv[1]);
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("textured mesh vertex buffer"),
                contents: bytemuck::cast_slice(&interleaved),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("textured mesh index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedTexturedMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a CPU-side [`Vram`] as a 1024×512 R16Uint texture. The fragment
    /// shader reads from it via `textureLoad` (no sampler - Uint textures
    /// aren't filterable on most backends, and PSX texture lookup is
    /// integer-exact anyway).
    pub fn upload_vram(&self, vram: &Vram) -> Result<UploadedVram> {
        let size = wgpu::Extent3d {
            width: VRAM_WIDTH as u32,
            height: VRAM_HEIGHT as u32,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("psx vram"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            vram.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(VRAM_WIDTH as u32 * 2),
                rows_per_image: Some(VRAM_HEIGHT as u32),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("psx vram bg"),
            layout: &self.vram_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let generation = self.vram_upload_counter.get() + 1;
        self.vram_upload_counter.set(generation);
        Ok(UploadedVram {
            bind_group,
            generation,
        })
    }

    /// Upload a VRAM mesh: position + per-vertex `(u, v)` (each 0..255) +
    /// per-vertex `(cba, tsb)` PSX VRAM addresses, plus triangle indices.
    /// Vertex layout matches the VRAM-mesh pipeline's 20-byte stride.
    pub fn upload_vram_mesh(
        &self,
        positions: &[[f32; 3]],
        uvs: &[[u8; 2]],
        cba_tsb: &[[u16; 2]],
        normals: &[[f32; 3]],
        indices: &[u32],
    ) -> Result<UploadedVramMesh> {
        if positions.len() != uvs.len()
            || positions.len() != cba_tsb.len()
            || positions.len() != normals.len()
        {
            anyhow::bail!(
                "vram mesh attribute length mismatch: pos={} uvs={} cba_tsb={} normals={}",
                positions.len(),
                uvs.len(),
                cba_tsb.len(),
                normals.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "vram mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "vram mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        let mut bytes = Vec::with_capacity(positions.len() * 32);
        for (((pos, uv), ct), n) in positions
            .iter()
            .zip(uvs.iter())
            .zip(cba_tsb.iter())
            .zip(normals.iter())
        {
            bytes.extend_from_slice(bytemuck::cast_slice(pos));
            // UV padded to 4 bytes (Uint8x4 - extra bytes ignored by shader).
            bytes.push(uv[0]);
            bytes.push(uv[1]);
            bytes.push(0);
            bytes.push(0);
            bytes.extend_from_slice(&ct[0].to_le_bytes());
            bytes.extend_from_slice(&ct[1].to_le_bytes());
            bytes.extend_from_slice(bytemuck::cast_slice(n));
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vram mesh vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Append the per-ABR-mode semi-transparent tail for the PSX-faithful
        // blend pass. The opaque pass still draws `0..indices.len()`
        // (`index_count` below), so the default path is unchanged.
        let (indices_with_tail, semi_ranges, semi_prims) =
            psx_blend::append_semi_tail(indices, cba_tsb, positions);
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vram mesh index buffer"),
                contents: bytemuck::cast_slice(&indices_with_tail),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedVramMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
            semi_ranges,
            semi_prims,
        })
    }

    /// Upload an untextured vertex-colour triangle mesh: position + per-vertex
    /// `[r, g, b]` (each 0..255, alpha forced opaque) + triangle indices. The
    /// inverse of [`Self::upload_vram_mesh`] for the `F*`/`G*` props that carry
    /// colours instead of UVs ([`legaia_tmd::mesh::ColorMesh`]). Every prim is
    /// treated as opaque; use [`Self::upload_color_mesh_blended`] when the
    /// source prims carry semi-transparency (ABE) state.
    pub fn upload_color_mesh(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 3]],
        indices: &[u32],
    ) -> Result<UploadedColorMesh> {
        self.upload_color_mesh_blended(positions, colors, indices, &[])
    }

    /// [`Self::upload_color_mesh`] plus per-vertex PSX semi-transparency
    /// state. `blend` is index-aligned with `positions`: each entry is a
    /// blend word packing the prim's ABE enable into bit 15 and its ABR
    /// blend mode into bits 5..=6 ([`psx_blend::pack_blend_word`] - the
    /// same packing the textured path rides on the TSB attribute). All
    /// corners of a triangle must share one word (the mesh builders emit
    /// fresh per-corner vertices per prim). An empty slice means "all
    /// opaque" and is what [`Self::upload_color_mesh`] passes.
    ///
    /// Semi-transparent triangles are duplicated into a per-ABR-mode index
    /// tail ([`psx_blend::append_semi_tail_words`]) drawn by the
    /// PSX-faithful blend pass; on real hardware an untextured ABE prim
    /// blends **all** its pixels (there is no per-texel STP gate), so in
    /// PSX mode the opaque pass discards those prims entirely and the
    /// blend pass owns them. The default (non-PSX) path still draws
    /// everything opaque, unchanged.
    pub fn upload_color_mesh_blended(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 3]],
        indices: &[u32],
        blend: &[u16],
    ) -> Result<UploadedColorMesh> {
        if positions.len() != colors.len() {
            anyhow::bail!(
                "color mesh: positions ({}) and colors ({}) length mismatch",
                positions.len(),
                colors.len()
            );
        }
        if !blend.is_empty() && blend.len() != positions.len() {
            anyhow::bail!(
                "color mesh: positions ({}) and blend words ({}) length mismatch",
                positions.len(),
                blend.len()
            );
        }
        if !indices.len().is_multiple_of(3) {
            anyhow::bail!(
                "color mesh: index count {} is not a multiple of 3",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "color mesh index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        let mut bytes = Vec::with_capacity(positions.len() * 20);
        for (i, (pos, c)) in positions.iter().zip(colors.iter()).enumerate() {
            bytes.extend_from_slice(bytemuck::cast_slice(pos));
            bytes.push(c[0]);
            bytes.push(c[1]);
            bytes.push(c[2]);
            bytes.push(0xFF); // opaque alpha (Unorm8x4)
            let word = blend.get(i).copied().unwrap_or(0) as u32;
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("color mesh vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        // Append the per-ABR-mode semi-transparent tail for the PSX-faithful
        // blend pass. The opaque pass still draws `0..indices.len()`
        // (`index_count` below), so the default path is unchanged.
        let (indices_with_tail, semi_ranges, semi_prims) = if blend.is_empty() {
            (indices.to_vec(), [(0u32, 0u32); 4], Vec::new())
        } else {
            psx_blend::append_semi_tail_words(indices, blend, positions)
        };
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("color mesh index buffer"),
                contents: bytemuck::cast_slice(&indices_with_tail),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedColorMesh {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
            semi_ranges,
            semi_prims,
        })
    }

    /// Upload a wireframe line mesh: position + per-vertex `[r, g, b, a]`
    /// (each 0..255), plus line indices. Indices form a `LineList`: every
    /// 2 indices = 1 segment.
    pub fn upload_lines(
        &self,
        positions: &[[f32; 3]],
        colors: &[[u8; 4]],
        indices: &[u32],
    ) -> Result<UploadedLines> {
        if positions.len() != colors.len() {
            anyhow::bail!(
                "lines: positions ({}) and colors ({}) length mismatch",
                positions.len(),
                colors.len()
            );
        }
        if !indices.len().is_multiple_of(2) {
            anyhow::bail!(
                "lines: index count {} is not a multiple of 2",
                indices.len()
            );
        }
        if let Some(&max_idx) = indices.iter().max()
            && (max_idx as usize) >= positions.len()
        {
            anyhow::bail!(
                "lines: index {} >= vertex count {}",
                max_idx,
                positions.len()
            );
        }
        // Interleave pos (12) + color (4) = 16 bytes/vertex.
        let mut bytes = Vec::with_capacity(positions.len() * 16);
        for (p, c) in positions.iter().zip(colors.iter()) {
            bytes.extend_from_slice(bytemuck::cast_slice(p));
            bytes.extend_from_slice(c);
        }
        let vertex_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines vertex buffer"),
                contents: &bytes,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines index buffer"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Ok(UploadedLines {
            vertex_buf,
            index_buf,
            index_count: indices.len() as u32,
        })
    }

    /// Upload a [`legaia_font::Font`]'s atlas to the GPU. Convenience wrapper
    /// around [`Self::upload_font_atlas`] that pulls dimensions and pixels
    /// from the parsed font directly. Use this when the caller is loading
    /// the dialog font; use the lower-level `upload_font_atlas` for custom
    /// atlases (debug fonts, sprite glyph sheets, etc).
    pub fn upload_font(&self, font: &legaia_font::Font) -> Result<UploadedFontAtlas> {
        let (w, h) = font.atlas_dimensions();
        self.upload_font_atlas(font.atlas_rgba(), w, h)
    }

    /// Upload a sprite atlas. Alias of [`Self::upload_font_atlas`] - sprites
    /// and font glyphs share the textured-quad pipeline (see [`SpriteDraw`]).
    pub fn upload_sprite_atlas(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<UploadedSpriteAtlas> {
        self.upload_font_atlas(rgba, width, height)
    }

    /// Upload a font atlas. Used by the 2D text pipeline; one atlas can back
    /// many [`TextOverlay`] batches.
    pub fn upload_font_atlas(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<UploadedFontAtlas> {
        if rgba.len() as u32 != width * height * 4 {
            anyhow::bail!(
                "font atlas RGBA length {} doesn't match {}x{} (expected {})",
                rgba.len(),
                width,
                height,
                width * height * 4
            );
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("font atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("font atlas bg"),
            layout: &self.text_atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.text_sampler),
                },
            ],
        });
        Ok(UploadedFontAtlas {
            bind_group,
            width,
            height,
        })
    }

    pub fn upload_texture(&self, rgba: &[u8], width: u32, height: u32) -> Result<UploadedTexture> {
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            anyhow::bail!(
                "rgba length mismatch: got {}, expected {}",
                rgba.len(),
                expected
            );
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uploaded texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Ok(UploadedTexture {
            bind_group,
            width,
            height,
        })
    }

    /// Render the scene. Dispatches by [`RenderTarget`]:
    /// * `Clear` - clear to dark gray, no draws.
    /// * `Texture(t)` - letterboxed quad (Phase 1 TIM viewer).
    /// * `Mesh { mesh, mvp }` - depth-tested 3D mesh draw (Phase 1 TMD viewer).
    pub fn render(&self, target: RenderTarget<'_>) -> Result<()> {
        // Stage uniform writes before begin_render_pass.
        match &target {
            RenderTarget::Texture(t) => {
                let (sx, sy) =
                    letterbox_scale(self.config.width, self.config.height, t.width, t.height);
                self.queue.write_buffer(
                    &self.uniforms_buf,
                    0,
                    bytemuck::cast_slice(&[Uniforms {
                        scale: [sx, sy, 0.0, 0.0],
                    }]),
                );
            }
            RenderTarget::Mesh { mvp, .. }
            | RenderTarget::TexturedMesh { mvp, .. }
            | RenderTarget::VramMesh { mvp, .. }
            | RenderTarget::Lines { mvp, .. } => {
                let snap = if self.psx_mode.get() { 1.0f32 } else { 0.0 };
                self.queue.write_buffer(
                    &self.mesh_uniforms_buf,
                    0,
                    bytemuck::cast_slice(&[MeshUniforms {
                        mvp: mvp.to_cols_array_2d(),
                        // Light coming from upper-back-left in world space.
                        light_dir: normalize3([0.4, -0.8, 0.4]),
                        psx_params: [
                            self.config.width as f32,
                            self.config.height as f32,
                            snap,
                            snap, // .w = dither_enable (shares the psx_mode flag)
                        ],
                        tex_window: self.tex_window.get(),
                    }]),
                );
            }
            RenderTarget::Scene(scene) => {
                self.stage_scene_uniforms(scene);
                let mut overlays: Vec<&TextOverlay<'_>> = Vec::with_capacity(3);
                if let Some(s) = scene.overlay_sprites {
                    overlays.push(s);
                }
                if let Some(s) = scene.overlay_sprites_2 {
                    overlays.push(s);
                }
                if let Some(t) = scene.overlay_text {
                    overlays.push(t);
                }
                if !overlays.is_empty() {
                    self.scene_quad_ranges
                        .borrow_mut()
                        .clone_from(&self.stage_quad_overlays(&overlays));
                } else {
                    self.scene_quad_ranges.borrow_mut().clear();
                }
            }
            RenderTarget::TextOnly(overlay) => {
                self.scene_quad_ranges
                    .borrow_mut()
                    .clone_from(&self.stage_quad_overlays(&[overlay]));
            }
            RenderTarget::Clear => {}
        }

        let frame = self
            .surface
            .get_current_texture()
            .context("get current swapchain texture")?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        {
            // Mesh paths use the depth attachment; texture/clear paths skip it
            // (it would just sit unused, but keeping the depth-stencil-attachment
            // optional avoids needing wgpu to validate it for 2D-only frames).
            let depth_attachment = matches!(
                target,
                RenderTarget::Mesh { .. }
                    | RenderTarget::TexturedMesh { .. }
                    | RenderTarget::VramMesh { .. }
                    | RenderTarget::Lines { .. }
                    | RenderTarget::Scene(_)
                    | RenderTarget::TextOnly(_)
            )
            .then(|| wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            });
            let clear_rgba = match &target {
                RenderTarget::Scene(s) => s
                    .clear_color
                    .map(|c| wgpu::Color {
                        r: c[0] as f64,
                        g: c[1] as f64,
                        b: c[2] as f64,
                        a: c[3] as f64,
                    })
                    .unwrap_or(wgpu::Color {
                        r: 0.05,
                        g: 0.05,
                        b: 0.07,
                        a: 1.0,
                    }),
                _ => wgpu::Color {
                    r: 0.05,
                    g: 0.05,
                    b: 0.07,
                    a: 1.0,
                },
            };
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("legaia frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_rgba),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_attachment,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            match target {
                RenderTarget::Clear => {}
                RenderTarget::Texture(t) => {
                    rp.set_pipeline(&self.pipeline);
                    rp.set_bind_group(0, &t.bind_group, &[]);
                    rp.set_bind_group(1, &self.uniforms_bg, &[]);
                    rp.draw(0..4, 0..1);
                }
                RenderTarget::Mesh { mesh, .. } => {
                    rp.set_pipeline(&self.mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::TexturedMesh { mesh, texture, .. } => {
                    rp.set_pipeline(&self.textured_mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_bind_group(1, &texture.bind_group, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::VramMesh { mesh, vram, mvp } => {
                    rp.set_pipeline(&self.vram_mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_bind_group(1, &vram.bind_group, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                    // PSX-faithful semi-transparency blend pass (see
                    // [`psx_blend`]): re-draw the semi prims back-to-front
                    // by per-prim depth (the retail ordering-table walk),
                    // selecting the matching ABR blend pipeline per run.
                    // Gated like the rest of the faithful extras.
                    if self.psx_mode.get() && mesh.has_semi_prims() {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        let mut list = self.blend_list.borrow_mut();
                        list.clear();
                        psx_blend::push_draw_prims(&mut list, false, 0, &mvp, mesh.semi_prims());
                        psx_blend::sort_blend_list(&mut list);
                        let mut bound_mode: Option<u8> = None;
                        psx_blend::coalesce_sorted(&list, |head, start, count| {
                            if bound_mode != Some(head.mode) {
                                rp.set_pipeline(
                                    &self.vram_mesh_blend_pipelines[head.mode as usize],
                                );
                                bound_mode = Some(head.mode);
                            }
                            rp.draw_indexed(start..start + count, 0, 0..1);
                        });
                    }
                }
                RenderTarget::Lines { mesh, .. } => {
                    rp.set_pipeline(&self.lines_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::Scene(scene) => {
                    let bg_borrow = self.scene_uniforms_bg.borrow();
                    let bg: &wgpu::BindGroup = &bg_borrow;
                    rp.set_pipeline(&self.scene_vram_mesh_pipeline);
                    rp.set_bind_group(1, &scene.vram.bind_group, &[]);
                    let stride = self.uniform_offset_alignment;
                    for (i, draw) in scene.draws.iter().enumerate() {
                        let off = (i as u32) * stride;
                        rp.set_bind_group(0, bg, &[off]);
                        rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                        rp.set_index_buffer(
                            draw.mesh.index_buf.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                    }
                    if let Some((lines, _mvp)) = scene.overlay_lines {
                        // Overlay-lines uniforms live at slot N (one past
                        // the last actor), staged by `stage_scene_uniforms`.
                        let off = (scene.draws.len() as u32) * stride;
                        rp.set_pipeline(&self.scene_lines_pipeline);
                        rp.set_bind_group(0, bg, &[off]);
                        rp.set_vertex_buffer(0, lines.vertex_buf.slice(..));
                        rp.set_index_buffer(lines.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..lines.index_count, 0, 0..1);
                    }
                    if !scene.color_draws.is_empty() {
                        // Untextured F*/G* props: slots follow the draws + the
                        // optional overlay-lines slot (see stage_scene_uniforms).
                        let color_base =
                            scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                        rp.set_pipeline(&self.scene_color_mesh_pipeline);
                        for (i, draw) in scene.color_draws.iter().enumerate() {
                            let off = (color_base + i as u32) * stride;
                            rp.set_bind_group(0, bg, &[off]);
                            rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                            rp.set_index_buffer(
                                draw.mesh.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                        }
                    }
                    // PSX-faithful semi-transparency blend pass (see
                    // [`psx_blend`]): after every opaque draw, re-draw the
                    // semi-transparent prims with the matching blend
                    // pipelines. Runs last among the 3D draws so blended
                    // fragments (which don't write depth) can't be
                    // overwritten by later opaque geometry.
                    //
                    // Ordering: retail inserts each semi prim into the
                    // depth-bucketed ordering table and blends back-to-front,
                    // interleaved across actors. The engine mirrors that at
                    // per-PRIMITIVE granularity - every semi prim of every
                    // semi-carrying draw (textured + untextured alike) is
                    // keyed by its centroid's clip-space `w` under the
                    // draw's MVP (= the average of its vertices' clip `w`,
                    // the GTE avg-Z the OT bins on) and the whole list is
                    // blended far-to-near, regardless of draw boundaries.
                    // Equal keys (one OT bucket) draw later-submitted-first,
                    // the retail LIFO bucket order (`AddPrim` prepends).
                    let any_semi = scene.draws.iter().any(|d| d.mesh.has_semi_prims())
                        || scene.color_draws.iter().any(|d| d.mesh.has_semi_prims());
                    if self.psx_mode.get() && any_semi {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        let color_base =
                            scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                        let mut list = self.blend_list.borrow_mut();
                        list.clear();
                        for (i, draw) in scene.draws.iter().enumerate() {
                            psx_blend::push_draw_prims(
                                &mut list,
                                false,
                                i as u32,
                                &draw.mvp,
                                draw.mesh.semi_prims(),
                            );
                        }
                        for (i, draw) in scene.color_draws.iter().enumerate() {
                            psx_blend::push_draw_prims(
                                &mut list,
                                true,
                                i as u32,
                                &draw.mvp,
                                draw.mesh.semi_prims(),
                            );
                        }
                        psx_blend::sort_blend_list(&mut list);
                        // Emit with state caching: rebind buffers + uniform
                        // offset only when the owning draw changes, switch
                        // pipelines only when the (path, ABR mode) changes;
                        // contiguous tail runs merge into one draw call
                        // (`coalesce_sorted`).
                        let mut bound_draw: Option<(bool, u32)> = None;
                        let mut bound_pipe: Option<(bool, u8)> = None;
                        psx_blend::coalesce_sorted(&list, |head, start, count| {
                            let draw_key = (head.untextured, head.draw_index);
                            if bound_draw != Some(draw_key) {
                                let (vbuf, ibuf, off) = if head.untextured {
                                    let m = scene.color_draws[head.draw_index as usize].mesh;
                                    (
                                        &m.vertex_buf,
                                        &m.index_buf,
                                        (color_base + head.draw_index) * stride,
                                    )
                                } else {
                                    let m = scene.draws[head.draw_index as usize].mesh;
                                    (&m.vertex_buf, &m.index_buf, head.draw_index * stride)
                                };
                                rp.set_vertex_buffer(0, vbuf.slice(..));
                                rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                                rp.set_bind_group(0, bg, &[off]);
                                bound_draw = Some(draw_key);
                            }
                            let pipe_key = (head.untextured, head.mode);
                            if bound_pipe != Some(pipe_key) {
                                let pipelines = if head.untextured {
                                    &self.scene_color_mesh_blend_pipelines
                                } else {
                                    &self.scene_vram_mesh_blend_pipelines
                                };
                                rp.set_pipeline(&pipelines[head.mode as usize]);
                                bound_pipe = Some(pipe_key);
                            }
                            rp.draw_indexed(start..start + count, 0, 0..1);
                        });
                    }
                    let mut overlays: Vec<&TextOverlay<'_>> = Vec::with_capacity(3);
                    if let Some(s) = scene.overlay_sprites {
                        overlays.push(s);
                    }
                    if let Some(s) = scene.overlay_sprites_2 {
                        overlays.push(s);
                    }
                    if let Some(t) = scene.overlay_text {
                        overlays.push(t);
                    }
                    if !overlays.is_empty() {
                        let ranges = self.scene_quad_ranges.borrow();
                        if !ranges.iter().all(|(_, n)| *n == 0) {
                            rp.set_pipeline(&self.text_pipeline);
                            let vbuf_borrow = self.text_vbuf.borrow();
                            let ibuf_borrow = self.text_ibuf.borrow();
                            rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                            rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                            for (overlay, (base_quad, count)) in overlays.iter().zip(ranges.iter())
                            {
                                if *count == 0 {
                                    continue;
                                }
                                rp.set_bind_group(0, &overlay.atlas.bind_group, &[]);
                                let start = base_quad * 6;
                                let end = (base_quad + count) * 6;
                                rp.draw_indexed(start..end, 0, 0..1);
                            }
                        }
                    }
                }
                RenderTarget::TextOnly(text) => {
                    let ranges = self.scene_quad_ranges.borrow();
                    if let Some(&(base_quad, count)) = ranges.first()
                        && count > 0
                    {
                        rp.set_pipeline(&self.text_pipeline);
                        rp.set_bind_group(0, &text.atlas.bind_group, &[]);
                        let vbuf_borrow = self.text_vbuf.borrow();
                        let ibuf_borrow = self.text_ibuf.borrow();
                        rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                        rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                        let start = base_quad * 6;
                        let end = (base_quad + count) * 6;
                        rp.draw_indexed(start..end, 0, 0..1);
                    }
                }
            }
        }
        self.queue.submit(std::iter::once(enc.finish()));
        frame.present();
        Ok(())
    }

    /// Resize the scene-uniforms buffer (and its bind group) to hold at
    /// least `slots` `MeshUniforms` entries, then write each entry.
    fn stage_scene_uniforms(&self, scene: &Scene<'_>) {
        let stride = self.uniform_offset_alignment as usize;
        let needed =
            scene.draws.len() + scene.overlay_lines.is_some() as usize + scene.color_draws.len();
        if needed == 0 {
            return;
        }
        let mut cap = self.scene_uniforms_capacity.get();
        if cap < needed {
            // Grow geometrically so we don't churn on small N.
            cap = needed.next_power_of_two().max(needed);
            let new_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene mesh uniforms (resized)"),
                size: (cap * stride) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let new_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene mesh uniforms bg (resized)"),
                layout: &self.scene_uniforms_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &new_buf,
                        offset: 0,
                        size: std::num::NonZeroU64::new(std::mem::size_of::<MeshUniforms>() as u64),
                    }),
                }],
            });
            *self.scene_uniforms_buf.borrow_mut() = new_buf;
            *self.scene_uniforms_bg.borrow_mut() = new_bg;
            self.scene_uniforms_capacity.set(cap);
        }
        // Build a flat byte buffer with one MeshUniforms entry per slot,
        // padded to `stride`. wgpu rejects overlapping writes, so we hand
        // the queue a single contiguous range.
        let total = needed * stride;
        let mut bytes = vec![0u8; total];
        let snap = if self.psx_mode.get() { 1.0f32 } else { 0.0 };
        let psx_params = [
            self.config.width as f32,
            self.config.height as f32,
            snap,
            snap, // .w = dither_enable (shares the psx_mode flag)
        ];
        let tex_window = self.tex_window.get();
        let push = |bytes: &mut [u8], slot: usize, mvp: Mat4| {
            let u = MeshUniforms {
                mvp: mvp.to_cols_array_2d(),
                light_dir: normalize3([0.4, -0.8, 0.4]),
                psx_params,
                tex_window,
            };
            let off = slot * stride;
            let n = std::mem::size_of::<MeshUniforms>();
            bytes[off..off + n].copy_from_slice(bytemuck::bytes_of(&u));
        };
        for (i, draw) in scene.draws.iter().enumerate() {
            push(&mut bytes, i, draw.mvp);
        }
        if let Some((_, mvp)) = scene.overlay_lines {
            push(&mut bytes, scene.draws.len(), mvp);
        }
        // Colour-mesh slots follow the draws + the optional overlay-lines slot.
        let color_base = scene.draws.len() + scene.overlay_lines.is_some() as usize;
        for (i, draw) in scene.color_draws.iter().enumerate() {
            push(&mut bytes, color_base + i, draw.mvp);
        }
        let buf_borrow = self.scene_uniforms_buf.borrow();
        let buf: &wgpu::Buffer = &buf_borrow;
        self.queue.write_buffer(buf, 0, &bytes);
    }
}

/// Number of quads in `text` capped at u32::MAX/6, or `None` if there's
/// nothing to draw. Pulled out so the pre-pass staging and the in-pass draw
/// agree on what counts as renderable.
fn text_quad_count(text: &TextOverlay<'_>) -> Option<u32> {
    let n = text.draws.len();
    if n == 0 {
        return None;
    }
    Some(n as u32)
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

impl Renderer {
    /// Build the per-frame text vertex/index buffers from one or more 2D
    /// quad overlays (sprite batches and text batches share the same
    /// pipeline; the only per-batch difference is the bound atlas). Quads
    /// are concatenated in input order; the returned `[(base_quad, count)]`
    /// pairs let the render pass issue one `draw_indexed` per overlay with
    /// the matching atlas bind group.
    ///
    /// Pixel coords are converted to NDC using the current surface size;
    /// atlas pixel coords are converted to `[0, 1]` UVs using each
    /// overlay's atlas size.
    fn stage_quad_overlays(&self, overlays: &[&TextOverlay<'_>]) -> Vec<(u32, u32)> {
        let mut total_quads: u32 = 0;
        let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(overlays.len());
        for o in overlays {
            let n = text_quad_count(o).unwrap_or(0);
            ranges.push((total_quads, n));
            total_quads = total_quads.saturating_add(n);
        }
        if total_quads == 0 {
            return ranges;
        }
        let needed_v = total_quads * 4;
        let needed_i = total_quads * 6;
        if needed_v > self.text_vertex_capacity.get() {
            let cap = needed_v.next_power_of_two().max(needed_v);
            *self.text_vbuf.borrow_mut() = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad2d vertex buffer (resized)"),
                size: (cap as u64) * 32,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_vertex_capacity.set(cap);
        }
        if needed_i > self.text_index_capacity.get() {
            let cap = needed_i.next_power_of_two().max(needed_i);
            *self.text_ibuf.borrow_mut() = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad2d index buffer (resized)"),
                size: (cap as u64) * 4,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_index_capacity.set(cap);
        }

        let surf_w = self.config.width.max(1) as f32;
        let surf_h = self.config.height.max(1) as f32;

        let mut verts: Vec<TextVertex> = Vec::with_capacity(needed_v as usize);
        let mut idxs: Vec<u32> = Vec::with_capacity(needed_i as usize);
        let mut quad_idx: u32 = 0;
        for overlay in overlays {
            let atlas_w = overlay.atlas.width.max(1) as f32;
            let atlas_h = overlay.atlas.height.max(1) as f32;
            for draw in overlay.draws {
                let (dx, dy, dw, dh) = draw.dst;
                let (sx, sy, sw, sh) = draw.src;
                let nx0 = (dx as f32 / surf_w) * 2.0 - 1.0;
                let ny0 = 1.0 - (dy as f32 / surf_h) * 2.0;
                let nx1 = ((dx + dw as i32) as f32 / surf_w) * 2.0 - 1.0;
                let ny1 = 1.0 - ((dy + dh as i32) as f32 / surf_h) * 2.0;
                let u0 = sx as f32 / atlas_w;
                let v0 = sy as f32 / atlas_h;
                let u1 = (sx + sw) as f32 / atlas_w;
                let v1 = (sy + sh) as f32 / atlas_h;
                let color = draw.color;
                let base = quad_idx * 4;
                verts.push(TextVertex {
                    pos: [nx0, ny0],
                    uv: [u0, v0],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx1, ny0],
                    uv: [u1, v0],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx0, ny1],
                    uv: [u0, v1],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx1, ny1],
                    uv: [u1, v1],
                    color,
                });
                idxs.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                quad_idx += 1;
            }
        }
        let vbuf_borrow = self.text_vbuf.borrow();
        let ibuf_borrow = self.text_ibuf.borrow();
        self.queue
            .write_buffer(&vbuf_borrow, 0, bytemuck::cast_slice(&verts));
        self.queue
            .write_buffer(&ibuf_borrow, 0, bytemuck::cast_slice(&idxs));
        ranges
    }
}

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn normalize3(v: [f32; 3]) -> [f32; 4] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len, 0.0]
}

pub(crate) fn letterbox_scale(win_w: u32, win_h: u32, tex_w: u32, tex_h: u32) -> (f32, f32) {
    let win_aspect = win_w as f32 / win_h.max(1) as f32;
    let tex_aspect = tex_w as f32 / tex_h.max(1) as f32;
    if win_aspect > tex_aspect {
        // Window wider than texture - pillarbox
        (tex_aspect / win_aspect, 1.0)
    } else {
        // Window taller than texture - letterbox
        (1.0, win_aspect / tex_aspect)
    }
}
