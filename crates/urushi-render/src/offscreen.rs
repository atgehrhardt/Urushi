//! Rendering scenes into memory: pixel tests and screenshots without a window.

use std::sync::mpsc;

use urushi_ui::{Color, Scene, TextSystem};

use crate::RenderError;
use crate::gpu::Gpu;
use crate::renderer::{FrameTarget, SceneRenderer};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Renders scenes to RGBA8 pixel buffers.
pub struct Offscreen {
    gpu: Gpu,
    renderer: SceneRenderer,
}

/// An RGBA8 image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA8 pixels.
    pub rgba: Vec<u8>,
}

impl Image {
    /// The pixel at `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

impl Offscreen {
    /// Opens a headless device. Fails where there is no GPU (or software adapter).
    pub fn new() -> Result<Self, RenderError> {
        let gpu = Gpu::headless()?;
        let renderer = SceneRenderer::new(&gpu.device, FORMAT);
        Ok(Self { gpu, renderer })
    }

    /// Renders `scene` at `width`×`height` physical pixels.
    pub fn render(
        &mut self,
        scene: &Scene,
        text: &mut TextSystem,
        width: u32,
        height: u32,
        scale: f32,
        clear: Color,
    ) -> Result<Image, RenderError> {
        let device = &self.gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let target = FrameTarget {
            view: &view,
            size: (width, height),
            scale,
            clear,
        };
        self.renderer
            .draw(device, &self.gpu.queue, &target, scene, text);

        let row = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Device(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Device(e.to_string()))?
            .map_err(|e| RenderError::Device(e.to_string()))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|e| RenderError::Device(e.to_string()))?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let start = (y * row) as usize;
            rgba.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(Image {
            width,
            height,
            rgba,
        })
    }
}
