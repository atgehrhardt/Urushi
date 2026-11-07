//! Device setup, for a window surface or headless (tests and screenshots).

use crate::RenderError;

/// A wgpu device and queue.
pub struct Gpu {
    /// The instance (needed to create surfaces).
    pub instance: wgpu::Instance,
    /// The adapter.
    pub adapter: wgpu::Adapter,
    /// The device.
    pub device: wgpu::Device,
    /// Its queue.
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Picks an adapter (compatible with `surface`, if given) and opens a device. Blocks.
    pub fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: surface,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("urushi"),
            required_limits:
                wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
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

    /// A device with no window, e.g. for rendering tests.
    pub fn headless() -> Result<Self, RenderError> {
        Self::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()),
            None,
        )
    }
}
