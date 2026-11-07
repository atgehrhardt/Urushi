//! Rendering scenes into a window.

use urushi_ui::{Color, Scene, TextSystem};

use crate::RenderError;
use crate::gpu::Gpu;
use crate::renderer::{FrameTarget, SceneRenderer};

/// Draws scenes into a window surface.
pub struct SurfaceRenderer {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: SceneRenderer,
    scale: f32,
}

impl SurfaceRenderer {
    /// Creates a renderer for `window` with a surface of `width`×`height` physical pixels.
    pub fn new(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Self, RenderError> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        if cfg!(target_os = "android") {
            // Some Android Vulkan drivers (including the emulator's) crash in
            // vkSetDebugUtilsObjectNameEXT, so never hand them debug labels.
            descriptor.flags = (descriptor.flags | wgpu::InstanceFlags::DISCARD_HAL_LABELS)
                - wgpu::InstanceFlags::DEBUG
                - wgpu::InstanceFlags::VALIDATION;
        }
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(window)
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        let gpu = Gpu::new(instance, Some(&surface))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        // Blend in sRGB space (like most UI toolkits), so text weight matches design tools.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| RenderError::Surface("surface supports no formats".into()))?;
        let mut config = surface
            .get_default_config(&gpu.adapter, width.max(1), height.max(1))
            .ok_or_else(|| {
                RenderError::Surface("surface is not supported by the adapter".into())
            })?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        let renderer = SceneRenderer::new(&gpu.device, format);
        Ok(Self {
            gpu,
            surface,
            config,
            renderer,
            scale,
        })
    }

    /// Call when the window's size or scale factor changes.
    pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
        self.scale = scale;
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
    }

    /// Physical pixels per logical pixel.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Draws a frame. Transient surface problems (minimized, resized mid-frame) skip the
    /// frame instead of failing.
    pub fn render(
        &mut self,
        scene: &Scene,
        text: &mut TextSystem,
        clear: Color,
    ) -> Result<(), RenderError> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(RenderError::Surface("surface validation error".into()));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let target = FrameTarget {
            view: &view,
            size: (self.config.width, self.config.height),
            scale: self.scale,
            clear,
        };
        self.renderer
            .draw(&self.gpu.device, &self.gpu.queue, &target, scene, text);
        self.gpu.queue.present(frame);
        Ok(())
    }
}
