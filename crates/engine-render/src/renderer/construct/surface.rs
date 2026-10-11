//! The window surface and its device: instance, adapter, device + queue,
//! swapchain format and the configured surface.

use super::*;

/// The configured surface and the device it presents from.
pub(super) struct SurfaceParts {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    /// The format the frame is viewed (and rendered) as; see
    /// [`choose_surface_format`].
    pub(super) view_format: wgpu::TextureFormat,
}

/// Create the surface for `window`, request a device for it and configure
/// the swapchain at `width` x `height`.
pub(super) async fn open_surface<W>(window: Arc<W>, width: u32, height: u32) -> Result<SurfaceParts>
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
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .context("request device")?;

    let info = adapter.get_info();
    log::info!(
        "wgpu adapter: {} ({:?}, {:?} backend, driver {})",
        info.name,
        info.device_type,
        info.backend,
        info.driver
    );

    let caps = surface.get_capabilities(&adapter);
    let (format, view_format) = choose_surface_format(&caps.formats);
    // `LEGAIA_VSYNC=off` uncaps the present rate so a profiling run reads the
    // engine's own frame cost rather than the display's refresh interval.
    // Default stays vsync'd (tear-free, and the sim is fixed-timestep anyway).
    let present_mode = if crate::profile::no_vsync() {
        wgpu::PresentMode::AutoNoVsync
    } else {
        wgpu::PresentMode::AutoVsync
    };
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: width.max(1),
        height: height.max(1),
        present_mode,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: if view_format == format {
            vec![]
        } else {
            vec![view_format]
        },
    };
    surface.configure(&device, &config);
    Ok(SurfaceParts {
        surface,
        device,
        queue,
        config,
        view_format,
    })
}
