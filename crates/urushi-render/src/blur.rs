//! Frosted glass: blurring what's already drawn under a [`urushi_ui::Backdrop`].
//!
//! A frame with backdrops is drawn into an offscreen texture instead of straight into
//! the target. At each backdrop the scene pass ends, the texture is blurred with a
//! dual-Kawase chain (repeated halving, then doubling back to half resolution), and a
//! new pass draws the blurred copy inside the backdrop's rounded rectangle before
//! carrying on. At the end the texture is copied to the target. Frames without
//! backdrops never come here and stay a single pass.

use bytemuck::{Pod, Zeroable};

/// The most halvings a blur may use; each roughly doubles its reach.
pub(crate) const MAX_LEVELS: usize = 5;

/// One backdrop, in physical pixels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct BackdropInstance {
    /// x, y, width, height.
    pub(crate) rect: [f32; 4],
    /// Corner radius, then unused.
    pub(crate) params: [f32; 4],
}

/// How many halvings blur `physical` pixels: about one per doubling of the reach.
pub(crate) fn levels_for(physical: f32) -> usize {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 1..=MAX_LEVELS"
    )]
    let levels = (physical / 2.0).max(1.0).log2().round().clamp(1.0, 5.0) as usize;
    levels.min(MAX_LEVELS)
}

/// A texture that's both drawn into and sampled.
struct Layer {
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

/// The scene texture and the blur chain, sized for one target size.
struct Layers {
    size: (u32, u32),
    scene: Layer,
    /// Half, quarter, … resolution.
    levels: Vec<Layer>,
}

/// Pipelines and textures for backdrops.
pub(crate) struct Blur {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    instances: wgpu::Buffer,
    layers: Option<Layers>,
}

impl Blur {
    /// Creates the pipelines for targets of `format`. `globals` is the scene's
    /// screen-size uniform layout.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        globals: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("urushi blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur source"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let full = |fs, blend| {
            pipeline(
                device,
                &shader,
                format,
                &[&layout],
                ("full_vs", fs),
                (None, blend),
            )
        };
        let down = full("down_fs", None);
        let up = full("up_fs", None);
        let blit = full("blit_fs", None);
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let composite = pipeline(
            device,
            &shader,
            format,
            &[&layout, globals],
            ("composite_vs", "composite_fs"),
            (
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BackdropInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                }),
                Some(wgpu::BlendState::ALPHA_BLENDING),
            ),
        );
        Self {
            format,
            layout,
            sampler,
            down,
            up,
            blit,
            composite,
            instances: instance_buffer(device, 16),
            layers: None,
        }
    }

    /// Makes sure the textures match a target of `size` physical pixels, and uploads
    /// this frame's backdrops.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        backdrops: &[BackdropInstance],
    ) {
        if self.layers.as_ref().is_none_or(|l| l.size != size) {
            let scene = self.layer(device, "frosted scene", size);
            let mut levels = Vec::with_capacity(MAX_LEVELS);
            let mut level = size;
            for _ in 0..MAX_LEVELS {
                level = (level.0.div_ceil(2).max(1), level.1.div_ceil(2).max(1));
                levels.push(self.layer(device, "blur level", level));
            }
            self.layers = Some(Layers {
                size,
                scene,
                levels,
            });
        }
        let bytes: &[u8] = bytemuck::cast_slice(backdrops);
        if self.instances.size() < bytes.len() as u64 {
            self.instances = instance_buffer(device, backdrops.len().next_power_of_two());
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, bytes);
        }
    }

    fn layer(&self, device: &wgpu::Device, label: &str, (width, height): (u32, u32)) -> Layer {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
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
        Layer { view, group }
    }

    /// The texture the frame is drawn into (after [`Self::prepare`]).
    pub(crate) fn scene_view(&self) -> Option<&wgpu::TextureView> {
        self.layers.as_ref().map(|l| &l.scene.view)
    }

    /// Blurs what's drawn so far into the half-resolution level, halving `levels` times.
    pub(crate) fn frost(&self, encoder: &mut wgpu::CommandEncoder, levels: usize) {
        let Some(layers) = &self.layers else { return };
        let levels = levels.clamp(1, layers.levels.len());
        let mut source = &layers.scene;
        for target in &layers.levels[..levels] {
            full_pass(encoder, &self.down, source, &target.view);
            source = target;
        }
        for target in layers.levels[..levels - 1].iter().rev() {
            full_pass(encoder, &self.up, source, &target.view);
            source = target;
        }
    }

    /// Draws backdrop `index` (of those passed to [`Self::prepare`]) from the last
    /// [`Self::frost`]. Leaves the pass's bind groups changed.
    pub(crate) fn composite(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        globals: &wgpu::BindGroup,
        index: u32,
    ) {
        let Some(blurred) = self.layers.as_ref().and_then(|l| l.levels.first()) else {
            return;
        };
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &blurred.group, &[]);
        pass.set_bind_group(1, globals, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, index..index + 1);
    }

    /// Copies the finished frame to `target`.
    pub(crate) fn blit(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        if let Some(layers) = &self.layers {
            full_pass(encoder, &self.blit, &layers.scene, target);
        }
    }
}

/// Draws one full-target triangle sampling `source`.
fn full_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    source: &Layer,
    target: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blur"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &source.group, &[]);
    pass.draw(0..3, 0..1);
}

fn instance_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("backdrops"),
        size: (count.max(16) * std::mem::size_of::<BackdropInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    groups: &[&wgpu::BindGroupLayout],
    (vs, fs): (&str, &str),
    (buffer, blend): (
        Option<wgpu::VertexBufferLayout<'_>>,
        Option<wgpu::BlendState>,
    ),
) -> wgpu::RenderPipeline {
    let groups: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(|g| Some(*g)).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(fs),
        bind_group_layouts: &groups,
        immediate_size: 0,
    });
    let buffers: Vec<Option<wgpu::VertexBufferLayout<'_>>> = buffer.into_iter().map(Some).collect();
    let topology = if buffers.is_empty() {
        wgpu::PrimitiveTopology::TriangleList
    } else {
        wgpu::PrimitiveTopology::TriangleStrip
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wider_blurs_use_more_halvings() {
        assert_eq!(levels_for(1.0), 1);
        assert_eq!(levels_for(16.0), 3);
        assert_eq!(levels_for(48.0), 5);
        assert_eq!(levels_for(10_000.0), MAX_LEVELS);
    }
}
