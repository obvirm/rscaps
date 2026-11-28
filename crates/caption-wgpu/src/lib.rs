//! `wgpu + lyon` caption raster backend (experimental).
//!
//! SVG tetap sumber kebenaran: `usvgr` mem-parse + layout teks, `lyon`
//! men-tessellate path, `wgpu` meraster ke texture GPU. Efek lanjutan
//! (filter/mask/blend/glass/occlusion) belum dipetakan penuh; backend ini
//! sengaja di balik feature sampai parity gate 41 template lulus.

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use lyon::math::point;
use lyon::path::Path as LyonPath;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexBuffers,
};
use std::collections::HashMap;
use std::sync::Arc;
use svgr::usvgr::{
    self, filter, fontdb::Database, Color, Group, ImageKind, Mask, Node, Paint, Path as SvgPath,
    PreloadedImageData, Visibility,
};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ImageVertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

#[derive(Clone)]
enum MaskSource {
    Image(Arc<PreloadedImageData>),
    Paths { vertices: Vec<Vertex>, indices: Vec<u32> },
}

struct ImageDraw {
    data: Arc<PreloadedImageData>,
    mask: Option<MaskSource>,
    vertices: Vec<ImageVertex>,
    indices: Vec<u32>,
}

struct LayerDraw {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    mask: MaskSource,
}

pub struct WgpuLyonRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    image_pipeline: wgpu::RenderPipeline,
    image_layout: wgpu::BindGroupLayout,
    masked_pipeline: wgpu::RenderPipeline,
    masked_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    width: u32,
    height: u32,
    fontdb: Database,
}

impl WgpuLyonRenderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let mut fontdb = Database::new();
        fontdb.load_system_fonts();
        Self::with_fontdb(width, height, &fontdb)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_fontdb(width: u32, height: u32, fontdb: &Database) -> Result<Self> {
        pollster::block_on(Self::new_async(width, height, fontdb.clone()))
    }

    pub async fn new_async(width: u32, height: u32, fontdb: Database) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .context("wgpu: tidak ada adapter GPU (WebGPU/Vulkan/DX12)")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rscaps-caption-wgpu"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("wgpu: request_device gagal")?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rscaps-caption-wgsl"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rscaps-caption-pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rscaps-caption-image-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        let image_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rscaps-caption-image-pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rscaps-caption-image-pipeline-layout"),
                bind_group_layouts: &[Some(&image_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImageVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_image"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let masked_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rscaps-caption-masked-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let masked_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rscaps-caption-masked-pipeline"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rscaps-caption-masked-pipeline-layout"),
                bind_group_layouts: &[Some(&masked_layout)],
                immediate_size: 0,
            })),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImageVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_image_mask"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rscaps-caption-image-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            image_pipeline,
            image_layout,
            masked_pipeline,
            masked_layout,
            sampler,
            width: width.max(1),
            height: height.max(1),
            fontdb,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_svg(&mut self, svg: &str) -> Result<Vec<u8>> {
        self.render_svg_with_images(svg, &HashMap::new())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_svg_with_images(
        &mut self,
        svg: &str,
        images: &HashMap<String, Arc<PreloadedImageData>>,
    ) -> Result<Vec<u8>> {
        pollster::block_on(self.render_svg_async(svg, images))
    }

    pub async fn render_svg_async(
        &mut self,
        svg: &str,
        images: &HashMap<String, Arc<PreloadedImageData>>,
    ) -> Result<Vec<u8>> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut image_draws = Vec::new();
        let mut layers = Vec::new();
        let opt = usvgr::Options {
            image_data: Some(images),
            ..Default::default()
        };
        let tree = usvgr::Tree::from_str(svg, &opt, &self.fontdb)
            .map_err(|err| anyhow::anyhow!("parse svg: {err}"))?;
        walk_group(
            tree.root(),
            1.0,
            &mut vertices,
            &mut indices,
            &mut image_draws,
            &mut layers,
            self.width,
            self.height,
        )?;

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rscaps-caption-target"),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
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

        let vertex_bytes = bytemuck::cast_slice(&vertices);
        let vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rscaps-caption-vertices"),
            size: vertex_bytes.len().max(4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !vertex_bytes.is_empty() {
            self.queue.write_buffer(&vertex_buffer, 0, vertex_bytes);
        }
        let index_bytes = bytemuck::cast_slice(&indices);
        let index_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rscaps-caption-indices"),
            size: index_bytes.len().max(4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !index_bytes.is_empty() {
            self.queue.write_buffer(&index_buffer, 0, index_bytes);
        }

        let mut image_resources = Vec::with_capacity(image_draws.len());
        for draw in &image_draws {
            let data = &draw.data;
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("rscaps-caption-image"),
                size: wgpu::Extent3d {
                    width: data.width.max(1),
                    height: data.height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
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
                &data.data.as_ref(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(data.width.max(1) * 4),
                    rows_per_image: Some(data.height.max(1)),
                },
                wgpu::Extent3d {
                    width: data.width.max(1),
                    height: data.height.max(1),
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mask_view = match &draw.mask {
                Some(MaskSource::Image(mask)) => {
                    let mask_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("rscaps-caption-mask"),
                        size: wgpu::Extent3d {
                            width: mask.width.max(1),
                            height: mask.height.max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: FORMAT,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    self.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &mask_texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &mask.data.as_ref(),
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(mask.width.max(1) * 4),
                            rows_per_image: Some(mask.height.max(1)),
                        },
                        wgpu::Extent3d {
                            width: mask.width.max(1),
                            height: mask.height.max(1),
                            depth_or_array_layers: 1,
                        },
                    );
                    Some(mask_texture.create_view(&wgpu::TextureViewDescriptor::default()))
                }
                Some(MaskSource::Paths { vertices, indices }) => {
                    let mask_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("rscaps-caption-mask-paths"),
                        size: wgpu::Extent3d {
                            width: self.width,
                            height: self.height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    let mask_view =
                        mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let vbytes = bytemuck::cast_slice(vertices);
                    let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("rscaps-caption-mask-vertices"),
                        size: vbytes.len().max(4) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue.write_buffer(&vbuf, 0, vbytes);
                    let ibytes = bytemuck::cast_slice(indices);
                    let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("rscaps-caption-mask-indices"),
                        size: ibytes.len().max(4) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue.write_buffer(&ibuf, 0, ibytes);
                    let mut mask_encoder =
                        self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("rscaps-caption-mask-encoder"),
                        });
                    {
                        let mut pass = mask_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("rscaps-caption-mask-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &mask_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pass.set_pipeline(&self.pipeline);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
                    }
                    self.queue.submit(Some(mask_encoder.finish()));
                    Some(mask_view)
                }
                None => None,
            };
            let (bind_group, masked) = if let Some(mask_view) = mask_view {
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rscaps-caption-masked-bind"),
                    layout: &self.masked_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&mask_view),
                        },
                    ],
                });
                (bind_group, true)
            } else {
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("rscaps-caption-image-bind"),
                    layout: &self.image_layout,
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
                (bind_group, false)
            };
            let vbytes = bytemuck::cast_slice(&draw.vertices);
            let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rscaps-caption-image-vertices"),
                size: vbytes.len().max(4) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if !vbytes.is_empty() {
                self.queue.write_buffer(&vbuf, 0, vbytes);
            }
            let ibytes = bytemuck::cast_slice(&draw.indices);
            let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rscaps-caption-image-indices"),
                size: ibytes.len().max(4) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if !ibytes.is_empty() {
                self.queue.write_buffer(&ibuf, 0, ibytes);
            }
            image_resources.push((bind_group, vbuf, ibuf, draw.indices.len(), masked));
        }

        for layer in &layers {
            if layer.indices.is_empty() {
                continue;
            }
            let layer_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("rscaps-caption-layer"),
                size: wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let layer_view = layer_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let lvbytes = bytemuck::cast_slice(&layer.vertices);
            let lvbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rscaps-caption-layer-vertices"),
                size: lvbytes.len().max(4) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&lvbuf, 0, lvbytes);
            let libytes = bytemuck::cast_slice(&layer.indices);
            let libuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rscaps-caption-layer-indices"),
                size: libytes.len().max(4) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&libuf, 0, libytes);
            let mut layer_encoder =
                self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rscaps-caption-layer-encoder"),
                });
            {
                let mut pass = layer_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("rscaps-caption-layer-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &layer_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, lvbuf.slice(..));
                pass.set_index_buffer(libuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..layer.indices.len() as u32, 0, 0..1);
            }
            self.queue.submit(Some(layer_encoder.finish()));

            let mask_view = match &layer.mask {
                MaskSource::Image(mask) => {
                    let mask_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("rscaps-caption-layer-mask"),
                        size: wgpu::Extent3d {
                            width: mask.width.max(1),
                            height: mask.height.max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: FORMAT,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    self.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &mask_texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &mask.data.as_ref(),
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(mask.width.max(1) * 4),
                            rows_per_image: Some(mask.height.max(1)),
                        },
                        wgpu::Extent3d {
                            width: mask.width.max(1),
                            height: mask.height.max(1),
                            depth_or_array_layers: 1,
                        },
                    );
                    mask_texture.create_view(&wgpu::TextureViewDescriptor::default())
                }
                MaskSource::Paths { vertices, indices } => {
                    let mask_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("rscaps-caption-layer-mask-paths"),
                        size: wgpu::Extent3d {
                            width: self.width,
                            height: self.height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                    let view = mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let vbytes = bytemuck::cast_slice(vertices);
                    let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("rscaps-caption-layer-mask-vertices"),
                        size: vbytes.len().max(4) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue.write_buffer(&vbuf, 0, vbytes);
                    let ibytes = bytemuck::cast_slice(indices);
                    let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("rscaps-caption-layer-mask-indices"),
                        size: ibytes.len().max(4) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    self.queue.write_buffer(&ibuf, 0, ibytes);
                    let mut mask_encoder =
                        self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("rscaps-caption-layer-mask-encoder"),
                        });
                    {
                        let mut pass = mask_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("rscaps-caption-layer-mask-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pass.set_pipeline(&self.pipeline);
                        pass.set_vertex_buffer(0, vbuf.slice(..));
                        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
                    }
                    self.queue.submit(Some(mask_encoder.finish()));
                    view
                }
            };
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("rscaps-caption-layer-bind"),
                layout: &self.masked_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&layer_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&mask_view),
                    },
                ],
            });
            let quad = vec![
                ImageVertex { pos: [-1.0, -1.0], uv: [0.0, 1.0] },
                ImageVertex { pos: [1.0, -1.0], uv: [1.0, 1.0] },
                ImageVertex { pos: [1.0, 1.0], uv: [1.0, 0.0] },
                ImageVertex { pos: [-1.0, 1.0], uv: [0.0, 0.0] },
            ];
            let qbytes = bytemuck::cast_slice(&quad);
            let qbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rscaps-caption-layer-quad"),
                size: qbytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&qbuf, 0, qbytes);
            let qidx: [u32; 6] = [0, 1, 2, 0, 2, 3];
            let qibytes = bytemuck::cast_slice(&qidx);
            let qibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rscaps-caption-layer-quad-indices"),
                size: qibytes.len() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&qibuf, 0, qibytes);
            image_resources.push((bind_group, qbuf, qibuf, 6, true));
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rscaps-caption-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rscaps-caption-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !indices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
            }
            for (bind_group, vbuf, ibuf, count, masked) in &image_resources {
                if *count == 0 {
                    continue;
                }
                pass.set_pipeline(if *masked {
                    &self.masked_pipeline
                } else {
                    &self.image_pipeline
                });
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, vbuf.slice(..));
                pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count as u32, 0, 0..1);
            }
        }

        let unpadded = self.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rscaps-caption-readback"),
            size: (padded * self.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| anyhow::anyhow!("wgpu poll: {err}"))?;
        rx.recv()
            .map_err(|err| anyhow::anyhow!("wgpu map recv: {err}"))?
            .map_err(|err| anyhow::anyhow!("wgpu map: {err}"))?;
        let mapped = slice
            .get_mapped_range()
            .map_err(|err| anyhow::anyhow!("wgpu mapped range: {err}"))?;
        let mut out = Vec::with_capacity((unpadded * self.height) as usize);
        for row in 0..self.height {
            let start = (row * padded) as usize;
            out.extend_from_slice(&mapped[start..start + unpadded as usize]);
        }
        drop(mapped);
        readback.unmap();
        Ok(out)
    }
}

#[derive(Clone, Copy)]
struct Shadow {
    dx: f32,
    dy: f32,
    blur: f32,
    color: [f32; 4],
}

fn shadows_for(filters: &[std::sync::Arc<filter::Filter>]) -> Vec<Shadow> {
    let mut out = Vec::new();
    for filter in filters {
        let mut flood = ([0.0, 0.0, 0.0, 1.0], 1.0f32);
        let mut offset = (0.0f32, 0.0f32);
        for prim in filter.primitives() {
            match prim.kind() {
                filter::Kind::Flood(f) => {
                    flood = (rgba(f.color()), f.opacity().get());
                }
                filter::Kind::Offset(o) => {
                    offset = (o.dx(), o.dy());
                }
                filter::Kind::DropShadow(s) => {
                    let c = rgba(s.color());
                    let blur = s.std_dev_x().get().max(s.std_dev_y().get());
                    out.push(Shadow {
                        dx: s.dx(),
                        dy: s.dy(),
                        blur,
                        color: [c[0], c[1], c[2], c[3] * s.opacity().get()],
                    });
                }
                filter::Kind::GaussianBlur(b) => {
                    let blur = b.std_dev_x().get().max(b.std_dev_y().get());
                    out.push(Shadow {
                        dx: offset.0,
                        dy: offset.1,
                        blur,
                        color: [flood.0[0], flood.0[1], flood.0[2], flood.0[3] * flood.1],
                    });
                }
                _ => {}
            }
        }
        // Filter outline (Flood + Composite + Merge) tanpa blur: salinan
        // berwarna di belakang source tidak setara outline CPU; lewati.
    }
    out
}

fn rgba(c: Color) -> [f32; 4] {
    [
        c.red as f32 / 255.0,
        c.green as f32 / 255.0,
        c.blue as f32 / 255.0,
        1.0,
    ]
}

fn walk_group(
    group: &Group,
    opacity: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    image_draws: &mut Vec<ImageDraw>,
    layers: &mut Vec<LayerDraw>,
    width: u32,
    height: u32,
) -> Result<()> {
    let opacity = opacity * group.opacity().get();
    let shadows = shadows_for(group.filters());
    if !shadows.is_empty() {
        for shadow in shadows {
            walk_children(
                group, opacity, vertices, indices, image_draws, layers, width, height, Some(shadow), None,
            )?;
        }
    }
    walk_children(
        group, opacity, vertices, indices, image_draws, layers, width, height, None, None,
    )
}

fn walk_children(
    group: &Group,
    opacity: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    image_draws: &mut Vec<ImageDraw>,
    layers: &mut Vec<LayerDraw>,
    width: u32,
    height: u32,
    shadow: Option<Shadow>,
    mask: Option<MaskSource>,
) -> Result<()> {
    for node in group.children() {
        match node {
            Node::Group(g) => {
                let opacity = opacity * g.opacity().get();
                let group_mask = mask_source(g.mask(), width, height)
                    .or_else(|| clip_source(g.clip_path(), width, height));
                if shadow.is_none() && group_mask.is_some() && !has_image(g) {
                    let mut layer_vertices = Vec::new();
                    let mut layer_indices = Vec::new();
                    let mut layer_images = Vec::new();
                    walk_children(
                        g,
                        opacity,
                        &mut layer_vertices,
                        &mut layer_indices,
                        &mut layer_images,
                        &mut Vec::new(),
                        width,
                        height,
                        None,
                        None,
                    )?;
                    if !layer_indices.is_empty() {
                        layers.push(LayerDraw {
                            vertices: layer_vertices,
                            indices: layer_indices,
                            mask: group_mask.unwrap(),
                        });
                        continue;
                    }
                }
                let mask = group_mask.or_else(|| mask.clone());
                if shadow.is_none() {
                    let nested = shadows_for(g.filters());
                    if !nested.is_empty() {
                        for s in nested {
                            walk_children(
                                g, opacity, vertices, indices, image_draws, layers, width, height,
                                Some(s), mask.clone(),
                            )?;
                        }
                    }
                }
                walk_children(
                    g, opacity, vertices, indices, image_draws, layers, width, height, shadow,
                    mask,
                )?;
            }
            Node::Path(p) => {
                add_path(p, opacity, vertices, indices, width, height, shadow)?
            }
            Node::FastShape(s) => {
                add_path(s.path(), opacity, vertices, indices, width, height, shadow)?
            }
            Node::Text(t) => walk_children(
                t.flattened(), opacity, vertices, indices, image_draws, layers, width, height, shadow,
                mask.clone(),
            )?,
            Node::Image(img) => {
                if shadow.is_none() {
                    add_image(img, opacity, image_draws, width, height, mask.clone())?;
                }
            }
        }
    }
    Ok(())
}

fn has_image(group: &Group) -> bool {
    group.children().iter().any(|node| match node {
        Node::Image(_) => true,
        Node::Group(g) => has_image(g),
        Node::Text(t) => has_image(t.flattened()),
        _ => false,
    })
}

fn mask_source(
    mask: Option<&Mask>,
    width: u32,
    height: u32,
) -> Option<MaskSource> {
    mask.and_then(|m| source_from_group(m.root(), width, height))
}

fn clip_source(
    clip: Option<&usvgr::ClipPath>,
    width: u32,
    height: u32,
) -> Option<MaskSource> {
    clip.and_then(|c| source_from_group(c.root(), width, height))
}

fn source_from_group(group: &Group, width: u32, height: u32) -> Option<MaskSource> {
    fn first_data_image(group: &Group) -> Option<Arc<PreloadedImageData>> {
        for node in group.children() {
            match node {
                Node::Image(img) => {
                    if let ImageKind::DATA(data) = img.kind() {
                        return Some(Arc::clone(data));
                    }
                }
                Node::Group(g) => {
                    if let Some(data) = first_data_image(g) {
                        return Some(data);
                    }
                }
                _ => {}
            }
        }
        None
    }
    if let Some(data) = first_data_image(group) {
        return Some(MaskSource::Image(data));
    }
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    collect_mask_paths(group, &mut vertices, &mut indices, width, height);
    if indices.is_empty() {
        None
    } else {
        Some(MaskSource::Paths { vertices, indices })
    }
}

fn collect_mask_paths(
    group: &Group,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    width: u32,
    height: u32,
) {
    for node in group.children() {
        match node {
            Node::Group(g) => collect_mask_paths(g, vertices, indices, width, height),
            Node::Path(p) => {
                let _ = add_path(p, 1.0, vertices, indices, width, height, None);
            }
            Node::FastShape(s) => {
                let _ = add_path(s.path(), 1.0, vertices, indices, width, height, None);
            }
            Node::Text(t) => collect_mask_paths(t.flattened(), vertices, indices, width, height),
            Node::Image(_) => {}
        }
    }
}

fn add_image(
    img: &usvgr::Image,
    opacity: f32,
    image_draws: &mut Vec<ImageDraw>,
    width: u32,
    height: u32,
    mask: Option<MaskSource>,
) -> Result<()> {
    if img.visibility() != Visibility::Visible {
        return Ok(());
    }
    let ImageKind::DATA(data) = img.kind() else {
        return Ok(());
    };
    let rect = img.abs_bounding_box();
    // Hanya gambar full-frame (kaca selene / occlusion pastor) yang aman
    // digambar tanpa mask/clip. Gambar mask/brush iris butuh mask graph.
    if rect.width() < width as f32 * 0.9 || rect.height() < height as f32 * 0.9 {
        return Ok(());
    }
    let x0 = rect.x();
    let y0 = rect.y();
    let x1 = x0 + rect.width();
    let y1 = y0 + rect.height();
    let _ = opacity;
    let verts = vec![
        ImageVertex { pos: ndc(x0, y0, width, height), uv: [0.0, 0.0] },
        ImageVertex { pos: ndc(x1, y0, width, height), uv: [1.0, 0.0] },
        ImageVertex { pos: ndc(x1, y1, width, height), uv: [1.0, 1.0] },
        ImageVertex { pos: ndc(x0, y1, width, height), uv: [0.0, 1.0] },
    ];
    image_draws.push(ImageDraw {
        data: Arc::clone(data),
        mask,
        vertices: verts,
        indices: vec![0, 1, 2, 0, 2, 3],
    });
    Ok(())
}

fn ndc(x: f32, y: f32, width: u32, height: u32) -> [f32; 2] {
    [
        (x / width as f32) * 2.0 - 1.0,
        1.0 - (y / height as f32) * 2.0,
    ]
}

fn add_path(
    path: &SvgPath,
    opacity: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    width: u32,
    height: u32,
    shadow: Option<Shadow>,
) -> Result<()> {
    if path.visibility() != Visibility::Visible {
        return Ok(());
    }
    let Some(lyon_path) = to_lyon_path(path)? else {
        return Ok(());
    };
    if let Some(fill) = path.fill() {
        let color = paint_color(fill.paint(), fill.opacity().get() * opacity);
        if let Some(shadow) = shadow {
            add_shadow_geometry(&lyon_path, color, shadow, vertices, indices, width, height)?;
        } else {
            tessellate_fill(&lyon_path, color, vertices, indices, width, height)?;
        }
    }
    if let Some(stroke) = path.stroke() {
        let color = paint_color(stroke.paint(), stroke.opacity().get() * opacity);
        if shadow.is_none() {
            tessellate_stroke(
                &lyon_path,
                color,
                stroke.width().get(),
                vertices,
                indices,
                width,
                height,
            )?;
        }
    }
    Ok(())
}

fn tessellate_fill(
    path: &LyonPath,
    color: [f32; 4],
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    width: u32,
    height: u32,
) -> Result<()> {
    let mut tess = FillTessellator::new();
    let mut geometry: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    tess.tessellate_path(
        path,
        &FillOptions::default(),
        &mut BuffersBuilder::new(&mut geometry, |v: FillVertex| {
            vertex(v.position(), color, width, height, 0.0, 0.0)
        }),
    )
    .map_err(|err| anyhow::anyhow!("lyon fill: {err}"))?;
    append_geometry(geometry, vertices, indices);
    Ok(())
}

fn tessellate_stroke(
    path: &LyonPath,
    color: [f32; 4],
    line_width: f32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    width: u32,
    height: u32,
) -> Result<()> {
    let mut tess = StrokeTessellator::new();
    let mut geometry: VertexBuffers<Vertex, u32> = VertexBuffers::new();
    tess.tessellate_path(
        path,
        &StrokeOptions::default().with_line_width(line_width),
        &mut BuffersBuilder::new(&mut geometry, |v: StrokeVertex| {
            vertex(v.position(), color, width, height, 0.0, 0.0)
        }),
    )
    .map_err(|err| anyhow::anyhow!("lyon stroke: {err}"))?;
    append_geometry(geometry, vertices, indices);
    Ok(())
}

fn add_shadow_geometry(
    path: &LyonPath,
    source: [f32; 4],
    shadow: Shadow,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    width: u32,
    height: u32,
) -> Result<()> {
    let color = [
        shadow.color[0],
        shadow.color[1],
        shadow.color[2],
        shadow.color[3] * source[3],
    ];
    // Gaussian blur didekati dengan beberapa salinan offset. Cukup untuk
    // membuat tepi bayangan lunak tanpa pass texture tambahan.
    let radius = shadow.blur.max(0.0);
    let mut samples: Vec<(f32, f32, f32)> = Vec::new();
    if radius <= 6.0 {
        let ring = [
            (0.0, 0.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (0.7, 0.7),
            (-0.7, 0.7),
            (0.7, -0.7),
            (-0.7, -0.7),
        ];
        let weight = 1.0 / ring.len() as f32;
        for (ox, oy) in ring {
            samples.push((ox * radius, oy * radius, weight));
        }
    } else {
        let sigma = (radius * 0.5).max(0.001);
        let step = radius * 0.5;
        let mut total = 0.0f32;
        for gy in -2..=2 {
            for gx in -2..=2 {
                let ox = gx as f32 * step;
                let oy = gy as f32 * step;
                let w = (-(ox * ox + oy * oy) / (2.0 * sigma * sigma)).exp();
                samples.push((ox, oy, w));
                total += w;
            }
        }
        if total > 0.0 {
            for sample in &mut samples {
                sample.2 /= total;
            }
        }
    }
    for &(ox, oy, weight) in &samples {
        let dx = shadow.dx + ox;
        let dy = shadow.dy + oy;
        let c = [color[0], color[1], color[2], color[3] * weight];
        let mut tess = FillTessellator::new();
        let mut geometry: VertexBuffers<Vertex, u32> = VertexBuffers::new();
        tess.tessellate_path(
            path,
            &FillOptions::default(),
            &mut BuffersBuilder::new(&mut geometry, |v: FillVertex| {
                vertex(v.position(), c, width, height, dx, dy)
            }),
        )
        .map_err(|err| anyhow::anyhow!("lyon shadow: {err}"))?;
        append_geometry(geometry, vertices, indices);
    }
    Ok(())
}

fn append_geometry(geometry: VertexBuffers<Vertex, u32>, vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>) {
    let base = vertices.len() as u32;
    vertices.extend_from_slice(&geometry.vertices);
    indices.extend(geometry.indices.iter().map(|i| i + base));
}

fn vertex(
    p: lyon::math::Point,
    color: [f32; 4],
    width: u32,
    height: u32,
    dx: f32,
    dy: f32,
) -> Vertex {
    Vertex {
        pos: [
            ((p.x + dx) / width as f32) * 2.0 - 1.0,
            1.0 - ((p.y + dy) / height as f32) * 2.0,
        ],
        color,
    }
}

fn to_lyon_path(path: &SvgPath) -> Result<Option<LyonPath>> {
    use svgr::usvgr::tiny_skia_path::{PathSegment, Point};
    let mut builder = LyonPath::builder();
    let mut started = false;
    let mut open = false;
    for seg in path.data().segments() {
        let map = |p: &Point| {
            let mut p = *p;
            path.abs_transform().map_point(&mut p);
            point(p.x, p.y)
        };
        match seg {
            PathSegment::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(map(&p));
                started = true;
                open = true;
            }
            PathSegment::LineTo(p) => {
                builder.line_to(map(&p));
            }
            PathSegment::QuadTo(c, p) => {
                builder.quadratic_bezier_to(map(&c), map(&p));
            }
            PathSegment::CubicTo(c1, c2, p) => {
                builder.cubic_bezier_to(map(&c1), map(&c2), map(&p));
            }
            PathSegment::Close => {
                if open {
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if !started {
        return Ok(None);
    }
    if open {
        builder.end(false);
    }
    Ok(Some(builder.build()))
}

fn paint_color(paint: &Paint, opacity: f32) -> [f32; 4] {
    let (color, alpha) = match paint {
        Paint::Color(c) => (*c, 1.0),
        Paint::LinearGradient(g) => g
            .stops()
            .first()
            .map(|s| (s.color(), s.opacity().get()))
            .unwrap_or((Color::black(), 1.0)),
        Paint::RadialGradient(g) => g
            .stops()
            .first()
            .map(|s| (s.color(), s.opacity().get()))
            .unwrap_or((Color::black(), 1.0)),
        Paint::Pattern(_) => (Color::black(), 1.0),
    };
    [
        color.red as f32 / 255.0,
        color.green as f32 / 255.0,
        color.blue as f32 / 255.0,
        opacity * alpha,
    ]
}

const SHADER: &str = r#"
struct In {
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
};
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};
@vertex
fn vs_main(in: In) -> Out {
    var out: Out;
    out.pos = vec4<f32>(in.pos, 0.0, 1.0);
    out.color = in.color;
    return out;
}
@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return in.color;
}

struct ImageIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
};
struct ImageOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var mask_tex: texture_2d<f32>;

@vertex
fn vs_image(in: ImageIn) -> ImageOut {
    var out: ImageOut;
    out.pos = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}

@fragment
fn fs_image_mask(in: ImageOut) -> @location(0) vec4<f32> {
    let color = textureSample(tex, samp, in.uv);
    let mask = textureSample(mask_tex, samp, in.uv);
    return vec4<f32>(color.rgb, color.a * mask.a);
}
"#;
