//! GPU context: instance, adapter, device, queue.

use crate::RenderError;

/// The wgpu objects every renderer needs. Create one per process.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Creates an instance honouring `WGPU_BACKEND` / `WGPU_ADAPTER_NAME` and
/// friends from the environment (offscreen use, no window surfaces).
pub fn new_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// Like [`new_instance`], for creating window surfaces on `display` (the
/// windowing system connection, e.g. SDL's X11/Wayland display).
pub fn new_instance_for_display(
    display: Box<dyn wgpu::wgt::WgpuHasDisplayHandle>,
) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
        display,
    ))
}

impl Gpu {
    /// A device for offscreen rendering only (no window, no display needed).
    pub fn headless() -> Result<Self, RenderError> {
        Self::new(new_instance(), None)
    }

    /// A device on `instance`, compatible with `surface` if given.
    pub fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter =
            pollster::block_on(
                instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::from_env()
                        .unwrap_or(wgpu::PowerPreference::HighPerformance),
                    force_fallback_adapter: false,
                    compatible_surface: surface,
                    ..Default::default()
                }),
            )
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?}, {})", info.name, info.backend, info.driver);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tfp-render device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// Blocks until the GPU has finished all submitted work (timing).
    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Adapter name and backend, for logs and `--list-devices`.
    pub fn describe(&self) -> String {
        let i = self.adapter.get_info();
        format!(
            "{} ({:?}, {:?}, driver {} {})",
            i.name, i.backend, i.device_type, i.driver, i.driver_info
        )
    }
}
