//! wgpu-based GPU rendering state for the Doom frame buffer.
//!
//! [`GpuState`] encapsulates all wgpu resources needed to upload and display
//! the 640 × 400 pixel Doom frame buffer produced by the engine's
//! `I_FinishUpdate` function.
//!
//! ## Rendering pipeline
//!
//! Each frame, the workflow is:
//!
//! 1. **Upload** – `DG_ScreenBuffer` (raw BGRA8 pixels) is written into a
//!    wgpu [`Texture`] via [`Queue::write_texture`].
//! 2. **Render** – A single fullscreen-quad draw call samples the texture and
//!    outputs to the swapchain surface.
//!
//! The WGSL shaders are embedded at compile time as string literals so the
//! binary is self-contained (no external shader files required).
//!
//! ## Pixel format
//!
//! The Doom engine's `I_FinishUpdate` function (in `i_video.c`) writes pixels
//! in BGRA byte order:
//!
//! | Byte offset | Channel |
//! |-------------|---------|
//! | 0 | Blue |
//! | 1 | Green |
//! | 2 | Red |
//! | 3 | Alpha (unused, always 0) |
//!
//! The intermediate [`Texture`] uses [`TextureFormat::Bgra8Unorm`] to match
//! this layout exactly, avoiding any CPU-side byte-swapping.
//!
//! [`Texture`]: wgpu::Texture
//! [`TextureFormat::Bgra8Unorm`]: wgpu::TextureFormat::Bgra8Unorm
//! [`Queue::write_texture`]: wgpu::Queue::write_texture

use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use room::doom::doomgeneric::{DOOMGENERIC_RESX, DOOMGENERIC_RESY};

// ---------------------------------------------------------------------------
// WGSL shaders (embedded)
// ---------------------------------------------------------------------------

/// WGSL source for the fullscreen-quad vertex and fragment shaders.
///
/// The vertex shader generates a full-screen quad from the vertex index
/// without needing a vertex buffer.  The fragment shader samples the Doom
/// frame texture and writes it to the render target.
const SHADER_SOURCE: &str = r#"
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

/// Generate a fullscreen quad (two triangles) from the vertex index.
///
/// Vertices are laid out as a counter-clockwise pair of triangles covering
/// clip space from (-1,-1) to (1,1).
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    // NDC positions for a fullscreen quad (two triangles, 6 vertices).
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
    );

    // UV coordinates: Doom's origin is top-left; wgpu NDC bottom-left.
    // Flip the V axis so the image is not upside-down.
    var tex_coords = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );

    var out: VertexOutput;
    out.clip_position = vec4<f32>(positions[vi], 0.0, 1.0);
    out.tex_coords = tex_coords[vi];
    return out;
}

@group(0) @binding(0) var doom_texture: texture_2d<f32>;
@group(0) @binding(1) var doom_sampler: sampler;

/// Sample the Doom frame texture and write the result to the render target.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(doom_texture, doom_sampler, in.tex_coords);
}
"#;

// ---------------------------------------------------------------------------
// GpuState
// ---------------------------------------------------------------------------

/// All wgpu resources required to render the Doom frame buffer to a window.
pub struct GpuState {
    /// The wgpu surface tied to the OS window.
    surface: wgpu::Surface<'static>,
    /// The logical GPU device.
    device: wgpu::Device,
    /// The command queue used to submit GPU work.
    queue: wgpu::Queue,
    /// Current swapchain configuration (updated on resize).
    config: wgpu::SurfaceConfiguration,
    /// Intermediate texture that receives the Doom screen pixels each frame.
    doom_texture: wgpu::Texture,
    /// Bind group binding `doom_texture` and its sampler to the shader.
    bind_group: wgpu::BindGroup,
    /// The render pipeline executing the fullscreen-quad blit.
    render_pipeline: wgpu::RenderPipeline,
}

/// Constructors and frame-time operations for [`GpuState`].
impl GpuState {
    /// Create a new [`GpuState`] for the given window.
    ///
    /// This performs the following wgpu setup:
    ///
    /// 1. Creates a [`wgpu::Instance`].
    /// 2. Creates a [`wgpu::Surface`] from the window.
    /// 3. Requests a [`wgpu::Adapter`] and [`wgpu::Device`] / [`wgpu::Queue`].
    /// 4. Configures the surface.
    /// 5. Creates the intermediate Doom-frame [`wgpu::Texture`].
    /// 6. Creates the render pipeline with the embedded WGSL shaders.
    ///
    /// # Errors
    ///
    /// Returns an error string if any wgpu initialisation step fails.
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        // Use the default backends (Vulkan, Metal, DX12, OpenGL).
        // The `new_without_display_handle` constructor is used here for simplicity;
        // a more complete implementation would pass the display handle from winit
        // to enable Wayland GLES compositing.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        // `Surface<'static>` requires the window to outlive the surface.
        // `Arc<Window>` is `'static`, so this is sound.
        let surface = instance
            .create_surface(window)
            .map_err(|e| format!("create_surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("no suitable GPU adapter found: {e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("doom_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            // Disable experimental features and API tracing for production use.
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("request_device: {e}"))?;
        let surface_caps = surface.get_capabilities(&adapter);
        // Prefer a non-sRGB format so colours match the palette exactly.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        // Created with BGRA8Unorm to match the pixel format written by the
        // engine's I_FinishUpdate function.
        let doom_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("doom_frame_texture"),
            size: wgpu::Extent3d {
                width: DOOMGENERIC_RESX as u32,
                height: DOOMGENERIC_RESY as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // BGRA8Unorm matches the byte layout produced by I_FinishUpdate.
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let doom_texture_view = doom_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let doom_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("doom_sampler"),
            // Use nearest-neighbour filtering to preserve pixel sharpness.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("doom_bind_group_layout"),
            entries: &[
                // Binding 0: the Doom frame texture.
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                // Binding 1: the texture sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("doom_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&doom_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&doom_sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("doom_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("doom_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("doom_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[], // No vertex buffer; positions come from vertex index.
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            doom_texture,
            bind_group,
            render_pipeline,
        })
    }

    /// Reconfigure the surface after a window resize.
    ///
    /// This must be called whenever the window's inner size changes, before
    /// the next call to [`render`].
    ///
    /// [`render`]: Self::render
    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Upload `pixels` to the Doom frame texture and blit it to the surface.
    ///
    /// `pixels` must be exactly `DOOMGENERIC_RESX * DOOMGENERIC_RESY * 4`
    /// bytes in BGRA8 format.
    ///
    /// # Errors
    ///
    /// Returns an error string if the swapchain texture cannot be acquired
    /// (e.g. the surface is lost or the window is minimised).
    pub fn render(&self, pixels: &[u8]) -> Result<(), String> {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.doom_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((DOOMGENERIC_RESX * 4) as u32),
                rows_per_image: Some(DOOMGENERIC_RESY as u32),
            },
            wgpu::Extent3d {
                width: DOOMGENERIC_RESX as u32,
                height: DOOMGENERIC_RESY as u32,
                depth_or_array_layers: 1,
            },
        );
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Surface is suboptimal but still usable; present this frame.
                t
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Err("surface timeout".to_owned());
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                // Window is minimised or hidden; skip this frame.
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return Err("surface lost or outdated".to_owned());
            }
            // Handle future variants without breaking the build.
            _ => {
                return Err("surface error: unknown status".to_owned());
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("doom_frame_encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("doom_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            // Draw 6 vertices (2 triangles) without a vertex buffer.
            render_pass.draw(0..6, 0..1);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}
