//! The [`Renderer`] constructor. [`Renderer::new_async`] is the ordered list
//! of what a renderer is built from; each line is one builder in the
//! submodule of its resource family, returning the pieces the renderer
//! keeps and the ones a later builder needs.
//!
//! The order is the order the wgpu objects are created in, and a builder
//! takes what an earlier one made by reference, so a new resource goes in at
//! its place in the list.

use super::*;

mod blend;
mod lighting;
mod mesh;
mod quad;
mod scene;
mod screen_overlay;
mod surface;
mod text;

/// Builds a static-offset uniform bind group: a one-entry `BindGroupLayout`,
/// a `create_buffer_init` buffer, and the matching one-entry bind group.
/// Shared by the quad (`uniforms_*`) and mesh (`mesh_uniforms_*`) setups,
/// which are identical apart from the label, shader `visibility`, and the
/// buffer `contents`. Labels reproduce the original `"{base} bgl"` /
/// `"{base}"` / `"{base} bg"` naming exactly. The dynamic-offset
/// `scene_uniforms` bind group is deliberately NOT built here (different BGL:
/// `has_dynamic_offset: true` + a non-None `min_binding_size`).
fn make_uniform_bind_group(
    device: &wgpu::Device,
    label: &str,
    contents: &[u8],
    visibility: wgpu::ShaderStages,
) -> (wgpu::BindGroupLayout, wgpu::Buffer, wgpu::BindGroup) {
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{label} bgl")),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{label} bg")),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buf.as_entire_binding(),
        }],
    });
    (bgl, buf, bg)
}

/// Pick the swapchain format to configure, and the format the frame is
/// *viewed* (and so rendered) as.
///
/// The shaders emit **PSX framebuffer values**, not linear light. The final
/// stage of every 3D shader is [`psx_dither`](crate::psx_dither), which
/// quantises each channel to 5 bits and expands it back with
/// `(c5 << 3) | (c5 >> 2)` - exactly the 8-bit value the console clocks out to
/// the display. Those values are already display-referred, so the attachment
/// must not re-encode them: an sRGB view treats the shader's output as linear
/// and applies the linear->sRGB transfer on store, which lifts every midtone
/// (a 5-bit `16`, retail byte `132`, would present as `192`) and rounds the
/// 5-bit quantisation the dither exists to model straight back out. It also
/// forces the PSX semi-transparency blends ([`psx_blend`](crate::psx_blend))
/// to happen in linear space, where retail blends the raw 5-bit values.
///
/// So the frame is always viewed as UNORM: what a shader writes is what the
/// display gets. The two formats differ only when the surface offers nothing
/// but sRGB formats, in which case the sRGB surface texture is viewed as its
/// UNORM twin (declared in `view_formats`).
pub(crate) fn choose_surface_format(
    formats: &[wgpu::TextureFormat],
) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
    let format = formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(formats[0]);
    (format, format.remove_srgb_suffix())
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
        let surface::SurfaceParts {
            surface,
            device,
            queue,
            config,
            view_format,
        } = surface::open_surface(window, width, height).await?;

        let quad = quad::build_quad(&device, view_format);

        // Depth convention: **reversed-Z** across every 3D pipeline. The MVP
        // upload sites post-multiply [`reverse_z`] (clip z' = w - z), the pass
        // clears depth to 0.0, and the compare functions below are the
        // mirrored Greater / GreaterEqual. With the Depth32Float attachment
        // this makes depth resolution proportional to view depth (~z * 2^-23)
        // instead of collapsing quadratically toward the far plane, which is
        // what lets sub-unit coplanar-separation nudges (see
        // `legaia_tmd::mesh::coplanar`) hold at every camera distance.

        let flat_mesh = mesh::build_flat_mesh(&device, view_format, width, height);
        let textured_mesh_pipeline = mesh::build_textured_mesh(
            &device,
            view_format,
            &flat_mesh.uniforms_bgl,
            &quad.bind_group_layout,
        );
        let vram_vertex_attributes = mesh::vram_vertex_attributes();
        let vram_vertex_layout = mesh::vram_vertex_layout(&vram_vertex_attributes);
        let vram_mesh = mesh::build_vram_mesh(
            &device,
            view_format,
            &flat_mesh.uniforms_bgl,
            &vram_vertex_layout,
        );
        let lines = mesh::build_wireframe(&device, view_format, &flat_mesh.uniforms_bgl);

        let scene_uniforms = scene::build_scene_uniforms(&device);
        let light_layer = lighting::build_light_layer(&device);
        let shadow_pass = lighting::build_shadow_pass(&device, scene_uniforms.offset_alignment);
        let glow = lighting::build_glow_sprites(&device, view_format);

        let scene_vram = scene::build_scene_vram_mesh(
            &device,
            view_format,
            &scene_uniforms.bgl,
            &vram_mesh.bgl,
            &light_layer.bgl,
            &vram_vertex_layout,
        );
        let vram_mesh_blend_pipelines = blend::blend_pipelines(
            &device,
            view_format,
            "legaia vram mesh blend pipeline",
            &vram_mesh.layout,
            &vram_mesh.shader,
            &vram_vertex_layout,
        );
        let scene_vram_mesh_blend_pipelines = blend::blend_pipelines(
            &device,
            view_format,
            "legaia scene vram mesh blend pipeline",
            &scene_vram.layout,
            &scene_vram.shader,
            &vram_vertex_layout,
        );
        let scene_lines_pipeline =
            scene::build_scene_wireframe(&device, view_format, &scene_uniforms.bgl, &lines.shader);
        let color_mesh = scene::build_scene_color_mesh(
            &device,
            view_format,
            &scene_uniforms.bgl,
            &vram_mesh.bgl,
            &light_layer.bgl,
        );

        let text = text::build_text_overlay(&device, view_format);
        let overlay = screen_overlay::build_screen_overlay(&device, view_format, &vram_mesh.bgl);
        let (viewport_fill_vbuf, viewport_fill_ibuf) = screen_overlay::build_viewport_fill(&device);

        let depth_view = create_depth_view(&device, config.width, config.height);

        Ok(Self {
            fog_volume_pass: std::cell::RefCell::new(None),
            fog_volume_active: std::cell::Cell::new(false),
            bloom_pass: std::cell::RefCell::new(None),
            bloom_on: std::cell::Cell::new(false),
            surface,
            device,
            queue,
            config,
            view_format,
            pipeline: quad.pipeline,
            sampler: quad.sampler,
            bind_group_layout: quad.bind_group_layout,
            uniforms_buf: quad.uniforms_buf,
            uniforms_bg: quad.uniforms_bg,
            mesh_pipeline: flat_mesh.pipeline,
            mesh_uniforms_buf: flat_mesh.uniforms_buf,
            mesh_uniforms_bg: flat_mesh.uniforms_bg,
            textured_mesh_pipeline,
            vram_mesh_pipeline: vram_mesh.pipeline,
            vram_mesh_blend_pipelines,
            vram_bgl: vram_mesh.bgl,
            scene_vram_mesh_pipeline: scene_vram.pipeline,
            scene_vram_mesh_blend_pipelines,
            scene_lines_pipeline,
            scene_color_mesh_pipeline: color_mesh.pipeline,
            scene_color_mesh_blend_pipelines: color_mesh.blend_pipelines,
            scene_uniforms_bgl: scene_uniforms.bgl,
            scene_uniforms_bg: std::cell::RefCell::new(scene_uniforms.bg),
            scene_uniforms_buf: std::cell::RefCell::new(scene_uniforms.buf),
            scene_uniforms_capacity: std::cell::Cell::new(scene_uniforms.initial_capacity),
            uniform_offset_alignment: scene_uniforms.offset_alignment,
            lines_pipeline: lines.pipeline,
            text_pipeline: text.pipeline,
            text_blend_pipelines: text.blend_pipelines,
            text_atlas_bgl: text.atlas_bgl,
            text_sampler: text.sampler,
            text_vbuf: std::cell::RefCell::new(text.vbuf),
            text_ibuf: std::cell::RefCell::new(text.ibuf),
            text_vertex_capacity: std::cell::Cell::new(text.initial_quads * 4),
            text_index_capacity: std::cell::Cell::new(text.initial_quads * 6),
            scene_quad_ranges: std::cell::RefCell::new(Vec::new()),
            blend_list: std::cell::RefCell::new(Vec::new()),
            depth_view,
            psx_mode: std::cell::Cell::new(false),
            ui_psx_tint: std::cell::Cell::new(true),
            vram_upload_counter: std::cell::Cell::new(0),
            tex_window: std::cell::Cell::new([0; 4]),
            draw_clips: std::cell::RefCell::new((Vec::new(), Vec::new())),
            draw_nclip: std::cell::RefCell::new(Vec::new()),
            color_grade: std::cell::Cell::new([1.0, 1.0, 1.0, 0.0]),
            depth_cue: std::cell::Cell::new([0.0, 0.0, 0.0, 0.0]),
            cue_ramp: std::cell::Cell::new([0.0, 0.0, 0.0, 0.0]),
            palette_grade: std::cell::Cell::new([0.0, 0.0, 0.0, 0.0]),
            backface_cull: std::cell::Cell::new(0.0),
            overworld_curve: std::cell::Cell::new(0.0),
            overworld_curve_bias: std::cell::Cell::new(0.0),
            prim_near: std::cell::Cell::new([0.0; 4]),
            // Semi-transparency (ABE) blending on by default: retail's GPU
            // always blends ABE prims, so field water / glass / effects should
            // composite in the clean render, not just under LEGAIA_PSX_RENDER.
            semi_blend: std::cell::Cell::new(true),
            scene_viewport: std::cell::Cell::new(None),
            dyn_lighting: std::cell::Cell::new(false),
            // Shadow sub-toggle defaults ON - it only bites while dynamic
            // lighting is enabled and lights are staged.
            dyn_shadows: std::cell::Cell::new(true),
            lighting_mood: std::cell::Cell::new(
                legaia_render_kernels::scene_lighting::LightingMood::DAY,
            ),
            glow_sprites: std::cell::RefCell::new(Vec::new()),
            // Camera-occlusion fade off by default: parity oracles and every
            // non-play-window consumer stay on the faithful render; the play
            // window opts in explicitly.
            occl_fade: std::cell::Cell::new(false),
            occl_focus: std::cell::Cell::new(None),
            occl_env_counts: std::cell::Cell::new((usize::MAX, usize::MAX)),
            scene_lights: std::cell::RefCell::new(Vec::new()),
            scene_view_proj: std::cell::Cell::new(None),
            scene_lights_buf: light_layer.buf,
            scene_lights_bg: light_layer.bg,
            shadow_layer_views: light_layer.shadow_layer_views,
            shadow_vram_pipeline: shadow_pass.vram_pipeline,
            shadow_color_pipeline: shadow_pass.color_pipeline,
            glow_pipeline: glow.pipeline,
            glow_uniforms_buf: glow.uniforms_buf,
            glow_uniforms_bg: glow.uniforms_bg,
            shadow_uniforms_bgl: shadow_pass.uniforms_bgl,
            shadow_uniforms_buf: std::cell::RefCell::new(shadow_pass.uniforms_buf),
            shadow_uniforms_bg: std::cell::RefCell::new(shadow_pass.uniforms_bg),
            shadow_uniforms_capacity: std::cell::Cell::new(shadow_pass.initial_capacity),
            screen_overlay_pipeline: overlay.pipeline,
            screen_overlay_semi_opaque_pipeline: overlay.semi_opaque_pipeline,
            screen_overlay_blend_pipelines: overlay.blend_pipelines,
            screen_overlay_vbuf: std::cell::RefCell::new(overlay.vbuf),
            screen_overlay_ibuf: std::cell::RefCell::new(overlay.ibuf),
            screen_overlay_vcap: std::cell::Cell::new(overlay.initial_quads * 4),
            screen_overlay_icap: std::cell::Cell::new(overlay.initial_quads * 6),
            screen_overlay_runs: std::cell::RefCell::new(Vec::new()),
            screen_overlay_under_runs: std::cell::Cell::new(0),
            screen_overlay_backdrop_runs: std::cell::Cell::new(0),
            viewport_fill_vbuf,
            viewport_fill_ibuf,
        })
    }
}
