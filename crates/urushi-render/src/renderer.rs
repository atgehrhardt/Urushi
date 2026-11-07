//! Turns a [`Scene`] into GPU draw calls.
//!
//! Primitives are batched in paint order: consecutive quads (or glyphs) that share a
//! clip become one instanced draw. Text is shaped at physical pixel size (crisp on
//! high-DPI screens), and the shaped result is cached per (text, font, size).

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use cosmic_text::CacheKey;
use urushi_ui::{Color, Font, Primitive, Rect, Scene, Text, TextSystem};

use crate::atlas::{ATLAS_SIZE, Atlas, AtlasFull};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadInstance {
    rect: [f32; 4],
    color: [f32; 4],
    border_color: [f32; 4],
    params: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlyphInstance {
    rect: [f32; 4],
    uv: [f32; 4],
    color: [f32; 4],
    is_color: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Quads,
    Glyphs,
}

struct Batch {
    kind: Kind,
    scissor: [u32; 4],
    start: u32,
    end: u32,
}

/// Instances and batches for one frame.
#[derive(Default)]
struct FrameData {
    quads: Vec<QuadInstance>,
    glyphs: Vec<GlyphInstance>,
    batches: Vec<Batch>,
}

impl FrameData {
    fn push_batch(&mut self, kind: Kind, scissor: [u32; 4], index: usize) {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        match self.batches.last_mut() {
            Some(b) if b.kind == kind && b.scissor == scissor && b.end == index => {
                b.end = index + 1;
            }
            _ => self.batches.push(Batch {
                kind,
                scissor,
                start: index,
                end: index + 1,
            }),
        }
    }
}

/// A shaped line: glyphs with pen offsets (physical pixels) from the line's top-left.
struct Shaped {
    glyphs: Vec<(CacheKey, f32, f32)>,
}

/// Where and how to draw one frame.
pub struct FrameTarget<'a> {
    /// Texture view to draw into.
    pub view: &'a wgpu::TextureView,
    /// Its size in physical pixels.
    pub size: (u32, u32),
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// Background colour.
    pub clear: Color,
}

/// The GPU resources needed to draw scenes: pipelines, atlas texture, instance buffers.
pub struct SceneRenderer {
    quad_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    atlas_texture: wgpu::Texture,
    atlas_group: wgpu::BindGroup,
    atlas: Atlas,
    quad_buffer: wgpu::Buffer,
    glyph_buffer: wgpu::Buffer,
    shaped: HashMap<(String, Font, u32), Shaped>,
}

fn rgba(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

impl SceneRenderer {
    /// Creates pipelines that draw into textures of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("urushi shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let (globals, globals_layout, globals_group) = create_globals(device);
        let (atlas_texture, atlas_layout, atlas_group) = create_atlas(device);
        let quad_attrs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2];
        let glyph_attrs =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32];
        let quad_pipeline = create_pipeline(
            device,
            &shader,
            format,
            &[&globals_layout],
            ("quad_vs", "quad_fs"),
            (std::mem::size_of::<QuadInstance>() as u64, &quad_attrs),
        );
        let glyph_pipeline = create_pipeline(
            device,
            &shader,
            format,
            &[&globals_layout, &atlas_layout],
            ("glyph_vs", "glyph_fs"),
            (std::mem::size_of::<GlyphInstance>() as u64, &glyph_attrs),
        );
        Self {
            quad_pipeline,
            glyph_pipeline,
            globals,
            globals_group,
            atlas_texture,
            atlas_group,
            atlas: Atlas::new(),
            quad_buffer: instance_buffer(
                device,
                "quads",
                1024 * std::mem::size_of::<QuadInstance>() as u64,
            ),
            glyph_buffer: instance_buffer(
                device,
                "glyphs",
                4096 * std::mem::size_of::<GlyphInstance>() as u64,
            ),
            shaped: HashMap::new(),
        }
    }

    /// Draws `scene` into `target`.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &FrameTarget<'_>,
        scene: &Scene,
        text: &mut TextSystem,
    ) {
        let frame = if let Ok(frame) = self.build(scene, text, target) {
            frame
        } else {
            // The atlas filled up: start over with an empty one.
            self.atlas.clear();
            self.shaped.clear();
            self.build(scene, text, target).unwrap_or_default()
        };
        self.upload(device, queue, target, &frame);
        self.encode(device, queue, target, &frame);
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &FrameTarget<'_>,
        frame: &FrameData,
    ) {
        for upload in self.atlas.pending.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.x,
                        y: upload.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload.width * 4),
                    rows_per_image: Some(upload.height),
                },
                wgpu::Extent3d {
                    width: upload.width,
                    height: upload.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "surface sizes are far below 2^24"
        )]
        let screen = [target.size.0 as f32, target.size.1 as f32, 0.0, 0.0];
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&screen));
        ensure_capacity(
            device,
            &mut self.quad_buffer,
            "quads",
            std::mem::size_of_val(frame.quads.as_slice()) as u64,
        );
        ensure_capacity(
            device,
            &mut self.glyph_buffer,
            "glyphs",
            std::mem::size_of_val(frame.glyphs.as_slice()) as u64,
        );
        if !frame.quads.is_empty() {
            queue.write_buffer(&self.quad_buffer, 0, bytemuck::cast_slice(&frame.quads));
        }
        if !frame.glyphs.is_empty() {
            queue.write_buffer(&self.glyph_buffer, 0, bytemuck::cast_slice(&frame.glyphs));
        }
    }

    fn encode(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &FrameTarget<'_>,
        frame: &FrameData,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        {
            let clear_color = target.clear;
            let clear = wgpu::Color {
                r: f64::from(clear_color.r),
                g: f64::from(clear_color.g),
                b: f64::from(clear_color.b),
                a: f64::from(clear_color.a),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.atlas_group, &[]);
            for batch in &frame.batches {
                let [x, y, w, h] = batch.scissor;
                if w == 0 || h == 0 {
                    continue;
                }
                pass.set_scissor_rect(x, y, w, h);
                let (pipeline, buffer) = match batch.kind {
                    Kind::Quads => (&self.quad_pipeline, &self.quad_buffer),
                    Kind::Glyphs => (&self.glyph_pipeline, &self.glyph_buffer),
                };
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..4, batch.start..batch.end);
            }
        }
        queue.submit([encoder.finish()]);
    }

    fn build(
        &mut self,
        scene: &Scene,
        text: &mut TextSystem,
        target: &FrameTarget<'_>,
    ) -> Result<FrameData, AtlasFull> {
        let scale = target.scale;
        let mut frame = FrameData::default();
        #[allow(
            clippy::cast_precision_loss,
            reason = "surface sizes are far below 2^24"
        )]
        let full = Rect::new(0.0, 0.0, target.size.0 as f32, target.size.1 as f32);
        let mut clips = vec![full];
        for primitive in &scene.primitives {
            let clip = *clips.last().unwrap_or(&full);
            let scissor = scissor_of(clip);
            match primitive {
                Primitive::PushClip(rect) => {
                    let physical = Rect::new(
                        rect.x * scale,
                        rect.y * scale,
                        rect.width * scale,
                        rect.height * scale,
                    );
                    clips.push(physical.intersect(&clip));
                }
                Primitive::PopClip => {
                    if clips.len() > 1 {
                        clips.pop();
                    }
                }
                Primitive::Quad(q) => {
                    let r = q.rect;
                    frame.quads.push(QuadInstance {
                        rect: [r.x * scale, r.y * scale, r.width * scale, r.height * scale],
                        color: rgba(q.color),
                        border_color: rgba(q.border_color),
                        params: [q.radius * scale, q.border_width * scale],
                    });
                    frame.push_batch(Kind::Quads, scissor, frame.quads.len() - 1);
                }
                Primitive::Text(t) => self.push_text(t, (scale, scissor), text, &mut frame)?,
            }
        }
        Ok(frame)
    }

    fn push_text(
        &mut self,
        t: &Text,
        (scale, scissor): (f32, [u32; 4]),
        text: &mut TextSystem,
        frame: &mut FrameData,
    ) -> Result<(), AtlasFull> {
        let key = (t.text.clone(), t.font, (t.size * scale).to_bits());
        if !self.shaped.contains_key(&key) {
            if self.shaped.len() > 20_000 {
                self.shaped.clear();
            }
            let shaped = shape(text, &t.text, t.font, t.size * scale);
            self.shaped.insert(key.clone(), shaped);
        }
        let Some(shaped) = self.shaped.get(&key) else {
            return Ok(());
        };
        let ox = (t.origin.x * scale).round();
        let oy = (t.origin.y * scale).round();
        #[allow(
            clippy::cast_precision_loss,
            reason = "atlas size is a small power of two"
        )]
        let atlas = ATLAS_SIZE as f32;
        for &(cache_key, gx, gy) in &shaped.glyphs {
            let Some(g) = self.atlas.get(text, cache_key)? else {
                continue;
            };
            #[allow(
                clippy::cast_precision_loss,
                reason = "glyph metrics and atlas coordinates are small integers"
            )]
            let (rect, uv) = (
                [
                    ox + gx + g.left as f32,
                    oy + gy - g.top as f32,
                    g.width as f32,
                    g.height as f32,
                ],
                [
                    g.x as f32 / atlas,
                    g.y as f32 / atlas,
                    (g.x + g.width) as f32 / atlas,
                    (g.y + g.height) as f32 / atlas,
                ],
            );
            frame.glyphs.push(GlyphInstance {
                rect,
                uv,
                color: rgba(t.color),
                is_color: if g.is_color { 1.0 } else { 0.0 },
            });
            frame.push_batch(Kind::Glyphs, scissor, frame.glyphs.len() - 1);
        }
        Ok(())
    }
}

fn create_globals(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::BindGroupLayout, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("globals"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals"),
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
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (buffer, layout, group)
}

fn create_atlas(device: &wgpu::Device) -> (wgpu::Texture, wgpu::BindGroupLayout, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glyph atlas"),
        size: wgpu::Extent3d {
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("atlas"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas"),
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
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("atlas"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (texture, layout, group)
}

fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    groups: &[&wgpu::BindGroupLayout],
    (vs, fs): (&str, &str),
    (stride, attributes): (u64, &[wgpu::VertexAttribute]),
) -> wgpu::RenderPipeline {
    let groups: Vec<Option<&wgpu::BindGroupLayout>> = groups.iter().map(|g| Some(*g)).collect();
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(vs),
        bind_group_layouts: &groups,
        immediate_size: 0,
    });
    let target = wgpu::ColorTargetState {
        format,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_mask: wgpu::ColorWrites::ALL,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vs),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn shape(text: &mut TextSystem, s: &str, font: Font, physical_size: f32) -> Shaped {
    let buffer = text.shape(s, font, physical_size);
    let mut glyphs = Vec::new();
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            #[allow(
                clippy::cast_precision_loss,
                reason = "pixel offsets are small integers"
            )]
            glyphs.push((
                physical.cache_key,
                physical.x as f32,
                physical.y as f32 + run.line_y.round(),
            ));
        }
    }
    Shaped { glyphs }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to non-negative pixel coordinates"
)]
fn scissor_of(r: Rect) -> [u32; 4] {
    let x = r.x.max(0.0).floor();
    let y = r.y.max(0.0).floor();
    let right = r.right().max(0.0).ceil();
    let bottom = r.bottom().max(0.0).ceil();
    [
        x as u32,
        y as u32,
        (right - x).max(0.0) as u32,
        (bottom - y).max(0.0) as u32,
    ]
}

fn instance_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size.max(256),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn ensure_capacity(device: &wgpu::Device, buffer: &mut wgpu::Buffer, label: &str, needed: u64) {
    if buffer.size() < needed {
        *buffer = instance_buffer(device, label, needed.next_power_of_two());
    }
}
