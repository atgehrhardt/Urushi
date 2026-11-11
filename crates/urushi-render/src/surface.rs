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
    pub fn new<W>(window: &W, width: u32, height: u32, scale: f32) -> Result<Self, RenderError>
    where
        W: Into<wgpu::SurfaceTarget<'static>> + Clone,
    {
        let android = cfg!(target_os = "android");
        let descriptor = || {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            if android {
                // Some Android Vulkan drivers (including the emulator's) crash in
                // vkSetDebugUtilsObjectNameEXT, so never hand them debug labels.
                descriptor.flags = (descriptor.flags | wgpu::InstanceFlags::DISCARD_HAL_LABELS)
                    - wgpu::InstanceFlags::DEBUG
                    - wgpu::InstanceFlags::VALIDATION;
            }
            descriptor
        };
        let mut last_error = RenderError::NoAdapter("no graphics backend enabled".into());
        for backends in backend_attempts(android, descriptor().backends) {
            let mut attempt = descriptor();
            attempt.backends = backends;
            let instance = wgpu::Instance::new(attempt);
            let surface = match instance.create_surface(window.clone()) {
                Ok(surface) => surface,
                Err(e) => {
                    last_error = RenderError::Surface(e.to_string());
                    continue;
                }
            };
            match Gpu::new(instance, Some(&surface)) {
                Ok(gpu) => return Self::configure(gpu, surface, width, height, scale),
                Err(e) => {
                    tracing::info!(?backends, error = %e, "no usable adapter; trying the next backend");
                    last_error = e;
                }
            }
        }
        Err(last_error)
    }

    fn configure(
        gpu: Gpu,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Self, RenderError> {
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

/// The backends to try, one instance per entry, in order.
///
/// On Android each attempt gets a single backend. With Vulkan and GL in one instance, the
/// Android emulator (with a host GPU) picked its GL adapter, and the EGL surface (created
/// at `configure`) failed because the window was already connected to another graphics
/// API (`EGL_BAD_ALLOC`). wgpu then panicked in `Surface::configure`, closing the app at
/// start. Vulkan goes first; GL is the fallback for devices without a usable Vulkan
/// driver. Elsewhere one instance with every configured backend is fine.
fn backend_attempts(android: bool, configured: wgpu::Backends) -> Vec<wgpu::Backends> {
    if !android {
        return vec![configured];
    }
    [wgpu::Backends::VULKAN, wgpu::Backends::GL]
        .into_iter()
        .filter(|b| configured.contains(*b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::backend_attempts;

    #[test]
    fn android_tries_vulkan_then_gl_one_at_a_time() {
        assert_eq!(
            backend_attempts(true, wgpu::Backends::all()),
            vec![wgpu::Backends::VULKAN, wgpu::Backends::GL]
        );
    }

    #[test]
    fn android_respects_a_backend_forced_from_the_environment() {
        assert_eq!(
            backend_attempts(true, wgpu::Backends::GL),
            vec![wgpu::Backends::GL]
        );
    }

    #[test]
    fn other_platforms_use_one_instance_with_every_backend() {
        assert_eq!(
            backend_attempts(false, wgpu::Backends::all()),
            vec![wgpu::Backends::all()]
        );
    }
}
