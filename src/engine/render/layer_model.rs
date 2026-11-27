//! Inference for the local WebGL prototype's versioned layer export.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytemuck::{Pod, Zeroable};
use serde::Deserialize;
use wgpu::util::DeviceExt;

fn bundled_export() -> &'static [u8] {
    crate::web_asset!(
        "../../../assets/effects/backpropagation-64-layers.json",
        "assets/effects/backpropagation-64-layers.json"
    )
}
const MAX_EXPORT_BYTES: u64 = 1024 * 1024;
const TILE: usize = 64;
const TEXELS: usize = TILE * TILE;
const PERIOD: usize = TILE / 2;
const CALIBRATION_LEVELS: usize = 5;
const CALIBRATION_COLUMNS: usize = 8;
const CALIBRATION_PROBES: usize = CALIBRATION_LEVELS.pow(3);
mod calibration;
const PHASES: [[u32; 2]; 4] = [[0, 0], [32, 0], [0, 32], [32, 32]];
const HIDDEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const RGB: [&str; 3] = ["r", "g", "b"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    format: String,
    version: u32,
    canvas: Canvas,
    tiling: Tiling,
    texture_layout: TextureLayout,
    input: Input,
    boundary: String,
    output: Output,
    flat_field_correction: Option<bool>,
    layers: [Layer; 3],
}

#[derive(Deserialize)]
struct Canvas {
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tiling {
    size: [usize; 2],
    stride: [usize; 2],
    phase_offsets: [[u32; 2]; 4],
    window: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextureLayout {
    width: usize,
    height: usize,
    format: String,
    row_order: String,
    tap_order: String,
    offset_axes: OffsetAxes,
    encoding: String,
    inactive_taps: String,
}

#[derive(Deserialize)]
struct OffsetAxes {
    x: String,
    y: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    range: [f32; 2],
    encoding: String,
    luminance_coefficients: [f32; 3],
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutputMode {
    Correction,
    Direct,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    mode: OutputMode,
    add_input_luminance: bool,
    add_input_rgb: bool,
    clamp_after_blending: [f32; 2],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Layer {
    name: String,
    enabled: bool,
    bypass: Option<String>,
    activation: String,
    bias: f32,
    offsets: Vec<[i32; 2]>,
    weight_channels: Option<String>,
    weights: Vec<Texture>,
    #[serde(default)]
    add_input_luminance: bool,
}

#[derive(Deserialize)]
struct Texture {
    channel: String,
    data: String,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ModelSettings {
    hidden1: [[i32; 4]; 4],
    hidden2: [[i32; 4]; 4],
    readout: [[i32; 4]; 4],
    luminance: [f32; 4],
    biases: [f32; 4],
    counts: [u32; 4],
    skips: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Presentation {
    viewport: [f32; 2],
    model_size: [f32; 2],
    add_input_rgb: u32,
    padding: [u32; 3],
}

struct Model {
    size: [u32; 2],
    settings: ModelSettings,
    weights: Vec<[f32; 4]>,
    add_input_rgb: bool,
}

impl Layer {
    fn taps(&self) -> [[i32; 4]; 4] {
        let mut result = [[0; 4]; 4];
        for (tap, &[x, y]) in result.iter_mut().zip(&self.offsets) {
            *tap = [x, y, 0, 0];
        }
        result
    }

    fn tap_count(&self) -> u32 {
        if self.enabled {
            self.offsets.len() as u32
        } else {
            0
        }
    }

    fn decode_weights(&self, channels: &[&str], output: &mut Vec<[f32; 4]>) -> Result<(), String> {
        if !self.enabled && self.weights.is_empty() {
            output.resize(output.len() + channels.len() * TEXELS, [0.; 4]);
            return Ok(());
        }
        if self.weights.len() != channels.len() {
            return Err(format!("{} has an unexpected texture count", self.name));
        }
        for (texture, channel) in self.weights.iter().zip(channels) {
            if texture.channel != *channel {
                return Err(format!("{} is missing the {channel} texture", self.name));
            }
            let bytes = STANDARD.decode(&texture.data).map_err(|e| e.to_string())?;
            if bytes.len() != TEXELS * 16 {
                return Err(format!(
                    "{} {channel} texture has the wrong size",
                    self.name
                ));
            }
            for texel in bytes.chunks_exact(16) {
                let values = std::array::from_fn(|i| {
                    f32::from_le_bytes(texel[i * 4..i * 4 + 4].try_into().unwrap())
                });
                if !values.iter().all(|value| value.is_finite()) {
                    return Err(format!("{} {channel} has a non-finite weight", self.name));
                }
                output.push(values);
            }
        }
        Ok(())
    }
}

impl Model {
    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_EXPORT_BYTES {
            return Err("layer export is larger than 1 MiB".into());
        }
        let export: Export = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        Self::from_export(export)
    }

    fn from_export(export: Export) -> Result<Self, String> {
        let layout = export.texture_layout;
        let tiling = export.tiling;
        let [hidden1, hidden2, readout] = export.layers;
        let add_input_rgb = matches!(export.output.mode, OutputMode::Correction);
        if export.format != "mtl-jobines-backpropagation"
            || export.version != 1
            || tiling.size != [TILE; 2]
            || tiling.stride != [TILE / 2; 2]
            || tiling.phase_offsets != PHASES
            || tiling.window != "separable_half_sample_raised_cosine"
            || [layout.width, layout.height] != [TILE; 2]
            || layout.format != "rgba32f"
            || layout.row_order != "bottom_to_top"
            || layout.tap_order != "rgba"
            || layout.offset_axes.x != "right"
            || layout.offset_axes.y != "up"
            || layout.encoding != "base64_f32_le"
            || layout.inactive_taps != "ignored"
            || export.boundary != "zero_outside_frame"
            || export.input.encoding != "srgb_code_values"
            || export.input.range != [0., 1.]
            || export.output.clamp_after_blending != [0., 1.]
            || export.output.add_input_rgb != add_input_rgb
        {
            return Err("unsupported layer export format or settings".into());
        }
        let size = [export.canvas.width, export.canvas.height];
        if size.iter().any(|dimension| !(1..=2048).contains(dimension)) {
            return Err("unsupported layer export canvas size".into());
        }
        for (layer, name, activation, bypass, channels) in [
            (
                &hidden1,
                "hidden1",
                "relu",
                Some("input_luminance"),
                Some("input_rgb"),
            ),
            (&hidden2, "hidden2", "relu", Some("identity"), None),
            (&readout, "readout", "linear", None, Some("output_rgb")),
        ] {
            if layer.name != name
                || layer.activation != activation
                || layer.bypass.as_deref() != bypass
                || layer.weight_channels.as_deref() != channels
                || !layer.bias.is_finite()
                || !(1..=4).contains(&layer.offsets.len())
                || layer
                    .offsets
                    .iter()
                    .flatten()
                    .any(|offset| offset.unsigned_abs() > 8)
            {
                return Err(format!("unsupported {name} layer settings"));
            }
        }
        if !readout.enabled {
            return Err("the RGB readout must be enabled".into());
        }
        let [r, g, b] = export.input.luminance_coefficients;
        if ![r, g, b].iter().all(|value| value.is_finite()) {
            return Err("non-finite luminance coefficients".into());
        }
        let mut weights = Vec::with_capacity(7 * TEXELS);
        hidden1.decode_weights(&RGB, &mut weights)?;
        hidden2.decode_weights(&["scalar"], &mut weights)?;
        readout.decode_weights(&RGB, &mut weights)?;
        Ok(Self {
            size,
            add_input_rgb,
            weights,
            settings: ModelSettings {
                hidden1: hidden1.taps(),
                hidden2: hidden2.taps(),
                readout: readout.taps(),
                luminance: [r, g, b, 0.],
                biases: [hidden1.bias, hidden2.bias, readout.bias, 0.],
                counts: [
                    hidden1.tap_count(),
                    hidden2.tap_count(),
                    readout.tap_count(),
                    0,
                ],
                skips: [
                    u32::from(hidden2.add_input_luminance),
                    u32::from(export.output.add_input_luminance),
                    u32::from(export.flat_field_correction.unwrap_or(true)),
                    0,
                ],
            },
        })
    }
}

pub(super) struct LayerModel {
    format: wgpu::TextureFormat,
    size: [u32; 2],
    add_input_rgb: bool,
    pub scene: wgpu::TextureView,
    scene_group: wgpu::BindGroup,
    scene_layout: wgpu::BindGroupLayout,
    presentation: wgpu::Buffer,
    input: wgpu::TextureView,
    hidden1: wgpu::TextureView,
    hidden2: wgpu::TextureView,
    result: wgpu::TextureView,
    model_groups: [wgpu::BindGroup; 3],
    model_pipelines: [wgpu::RenderPipeline; 3],
    resample: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    [width, height]: [u32; 2],
    format: wgpu::TextureFormat,
    view_formats: &[wgpu::TextureFormat],
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats,
    })
}

fn upload_float_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    [width, height]: [u32; 2],
    values: &[[f32; 4]],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HIDDEN_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(values),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 16),
            rows_per_image: None,
        },
        size,
    );
    texture.create_view(&Default::default())
}

fn layout_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    }
}

fn texture_binding(filterable: bool) -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

fn buffer_binding(ty: wgpu::BufferBindingType) -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    destination: &wgpu::TextureView,
    [width, height]: [u32; 2],
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("layer model"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: destination,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_viewport(0., 0., width as f32, height as f32, 0., 1.);
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

fn render_source(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    texture(device, "layer model scene", size, format, &[]).create_view(&Default::default())
}

fn scene_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    presentation: &wgpu::Buffer,
    codes: &wgpu::TextureView,
    result: &wgpu::TextureView,
) -> wgpu::BindGroup {
    use wgpu::BindingResource::TextureView;
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer model presentation"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: presentation.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: TextureView(codes),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: TextureView(result),
            },
        ],
    })
}

impl LayerModel {
    fn supported(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> bool {
        matches!(
            format,
            wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb
        ) && adapter
            .get_texture_format_features(HIDDEN_FORMAT)
            .allowed_usages
            .contains(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT)
    }

    pub fn bundled(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        viewport: [u32; 2],
    ) -> Option<Self> {
        if !Self::supported(adapter, format) {
            return None;
        }
        match Model::from_bytes(bundled_export()) {
            Ok(model) => {
                let model = Self::new(device, queue, format, viewport, model);
                #[cfg(not(target_arch = "wasm32"))]
                eprintln!("Bundled layer model active");
                #[cfg(target_arch = "wasm32")]
                web_sys::console::info_1(&"Bundled layer model active".into());
                Some(model)
            }
            Err(error) => {
                #[cfg(not(target_arch = "wasm32"))]
                eprintln!("Skipping bundled layer model: {error}");
                #[cfg(target_arch = "wasm32")]
                web_sys::console::warn_1(&format!("Skipping bundled layer model: {error}").into());
                None
            }
        }
    }

    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        viewport: [u32; 2],
        mut model: Model,
    ) -> Self {
        let size = model.size;
        let scene = render_source(device, viewport, format);
        let input = texture(
            device,
            "layer model input codes",
            size,
            wgpu::TextureFormat::Rgba8Unorm,
            &[],
        )
        .create_view(&Default::default());
        let hidden1 = texture(device, "layer model hidden 1", size, HIDDEN_FORMAT, &[])
            .create_view(&Default::default());
        let hidden2 = texture(device, "layer model hidden 2", size, HIDDEN_FORMAT, &[])
            .create_view(&Default::default());
        let result = texture(device, "layer model output", size, HIDDEN_FORMAT, &[])
            .create_view(&Default::default());

        let weights = upload_float_texture(
            device,
            queue,
            "layer model weights",
            [TILE as u32, (7 * TILE) as u32],
            &model.weights,
        );
        let bias_size = [
            (PERIOD * CALIBRATION_COLUMNS) as u32,
            (PERIOD * CALIBRATION_PROBES.div_ceil(CALIBRATION_COLUMNS)) as u32,
        ];
        let measured_bias = (model.settings.skips[2] != 0)
            .then(|| calibration::flat_field_bias(&model))
            .flatten();
        model.settings.skips[2] = u32::from(measured_bias.is_some());
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("layer model settings"),
            contents: bytemuck::bytes_of(&model.settings),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bias = match measured_bias {
            Some(bias) => {
                let stride = bias_size[0] as usize;
                let mut atlas = vec![[0.; 4]; (bias_size[0] * bias_size[1]) as usize];
                for (probe, tile) in bias.chunks_exact(PERIOD * PERIOD).enumerate() {
                    let origin = (probe / CALIBRATION_COLUMNS) * PERIOD * stride
                        + (probe % CALIBRATION_COLUMNS) * PERIOD;
                    for (row, source) in tile.chunks_exact(PERIOD).enumerate() {
                        let start = origin + row * stride;
                        atlas[start..start + PERIOD].copy_from_slice(source);
                    }
                }
                upload_float_texture(
                    device,
                    queue,
                    "layer model flat-field bias",
                    bias_size,
                    &atlas,
                )
            }
            None => {
                upload_float_texture(device, queue, "layer model zero bias", [1, 1], &[[0.; 4]])
            }
        };
        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer model inference"),
            entries: &[
                layout_entry(0, buffer_binding(wgpu::BufferBindingType::Uniform)),
                layout_entry(1, texture_binding(false)),
                layout_entry(2, texture_binding(false)),
                layout_entry(3, texture_binding(false)),
                layout_entry(4, texture_binding(false)),
            ],
        });
        let model_groups = [&input, &hidden1, &hidden2].map(|previous| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("layer model inference"),
                layout: &model_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: settings.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&weights),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(previous),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&bias),
                    },
                ],
            })
        });
        let model_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("layer model inference"),
                bind_group_layouts: &[Some(&model_layout)],
                immediate_size: 0,
            });
        let model_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer model inference"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layer_model.wgsl").into()),
        });
        let model_pipelines = ["hidden1", "hidden2", "readout"].map(|entry| {
            pipeline(
                device,
                &model_shader,
                &model_pipeline_layout,
                entry,
                HIDDEN_FORMAT,
            )
        });

        let presentation = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer model presentation"),
            size: std::mem::size_of::<Presentation>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer model presentation"),
            entries: &[
                layout_entry(0, buffer_binding(wgpu::BufferBindingType::Uniform)),
                layout_entry(1, texture_binding(false)),
                layout_entry(2, texture_binding(false)),
            ],
        });
        let scene_group = scene_group(device, &scene_layout, &presentation, &scene, &result);
        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("layer model presentation"),
                bind_group_layouts: &[Some(&scene_layout)],
                immediate_size: 0,
            });
        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer model presentation"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layer_model_present.wgsl").into()),
        });
        Self {
            format,
            size,
            add_input_rgb: model.add_input_rgb,
            scene,
            scene_group,
            scene_layout,
            presentation,
            input,
            hidden1,
            hidden2,
            result,
            model_groups,
            model_pipelines,
            resample: pipeline(
                device,
                &scene_shader,
                &scene_pipeline_layout,
                "resample",
                wgpu::TextureFormat::Rgba8Unorm,
            ),
            present: pipeline(
                device,
                &scene_shader,
                &scene_pipeline_layout,
                "present",
                format,
            ),
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        self.scene = render_source(device, size, self.format);
        self.scene_group = scene_group(
            device,
            &self.scene_layout,
            &self.presentation,
            &self.scene,
            &self.result,
        );
    }

    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: [u32; 2],
    ) {
        let settings = Presentation {
            viewport: viewport.map(|dimension| dimension as f32),
            model_size: self.size.map(|dimension| dimension as f32),
            add_input_rgb: u32::from(self.add_input_rgb),
            padding: [0; 3],
        };
        queue.write_buffer(&self.presentation, 0, bytemuck::bytes_of(&settings));
        pass(
            encoder,
            &self.resample,
            &self.scene_group,
            &self.input,
            self.size,
        );
        for ((pipeline, group), destination) in self
            .model_pipelines
            .iter()
            .zip(&self.model_groups)
            .zip([&self.hidden1, &self.hidden2, &self.result])
        {
            pass(encoder, pipeline, group, destination, self.size);
        }
        pass(encoder, &self.present, &self.scene_group, target, viewport);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
