use super::*;
use crate::engine::render::Renderer;
use serde_json::{json, Value};

fn export(disabled: bool) -> Value {
    let texture = |channel: &str| {
        let values: Vec<[f32; 4]> = (0..TEXELS)
            .map(|index| {
                let x = (index % TILE) as f32;
                let y = (index / TILE) as f32;
                [0.04 + x * 0.001, -0.08 + y * 0.002, 0.1, -0.03]
            })
            .collect();
        let bytes: Vec<_> = values
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        json!({"channel": channel, "data": STANDARD.encode(bytes)})
    };
    let rgb = RGB.map(texture);
    let scalar = texture("scalar");
    let hidden1 = if disabled { vec![] } else { rgb.to_vec() };
    let hidden2 = if disabled { vec![] } else { vec![scalar] };
    json!({
        "format": "mtl-jobines-backpropagation", "version": 1,
        "canvas": {"width": 73, "height": 41},
        "tiling": {"size": [64,64], "stride": [32,32], "phaseOffsets": PHASES,
            "window": "separable_half_sample_raised_cosine"},
        "textureLayout": {"width": 64, "height": 64, "format": "rgba32f",
            "rowOrder": "bottom_to_top", "tapOrder": "rgba", "offsetAxes": {"x": "right", "y": "up"},
            "encoding": "base64_f32_le", "inactiveTaps": "ignored"},
        "input": {"range": [0,1], "encoding": "srgb_code_values", "luminanceCoefficients": [0.2126,0.7152,0.0722]},
        "boundary": "zero_outside_frame",
        "output": {"mode": if disabled { "direct" } else { "correction" }, "addInputLuminance": true,
            "addInputRgb": !disabled, "clampAfterBlending": [0,1]},
        "layers": [
            {"name": "hidden1", "enabled": !disabled, "bypass": "input_luminance", "activation": "relu",
                "bias": 0.10, "offsets": [[0,2],[-2,0],[0,-2],[2,0]], "weightChannels": "input_rgb", "weights": hidden1},
            {"name": "hidden2", "enabled": !disabled, "bypass": "identity", "activation": "relu",
                "bias": 0.02, "offsets": [[0,0],[-2,0],[2,0]], "addInputLuminance": !disabled, "weights": hidden2},
            {"name": "readout", "enabled": true, "activation": "linear", "bias": 0,
                "offsets": [[0,1],[-1,0],[0,-1],[1,0]], "weightChannels": "output_rgb", "weights": rgb}
        ]
    })
}

fn decode(value: Value) -> Result<Model, String> {
    Model::from_export(serde_json::from_value(value).unwrap())
}

#[test]
fn decodes_weight_planes_and_bypasses_and_rejects_incompatible_exports() {
    let model = decode(export(false)).unwrap();
    assert_eq!(model.settings.counts, [4, 3, 4, 0]);
    assert_eq!(model.settings.hidden1[0], [0, 2, 0, 0]);
    assert_eq!(model.weights.len(), 7 * TEXELS);
    assert_eq!(model.weights[0], [0.04, -0.08, 0.1, -0.03]);
    assert_eq!(model.weights[TEXELS], model.weights[0]);
    let bypassed = decode(export(true)).unwrap();
    assert_eq!(bypassed.settings.counts, [0, 0, 4, 0]);
    assert_eq!(bypassed.weights[3 * TEXELS], [0.; 4]);
    assert_eq!(bypassed.weights[4 * TEXELS], model.weights[0]);
    let mut invalid = export(false);
    invalid["layers"][0]["weights"][0]["data"] = json!(STANDARD.encode([0; 16]));
    assert!(decode(invalid).err().unwrap().contains("wrong size"));
    let mut invalid = export(false);
    invalid["tiling"]["phaseOffsets"][0] = json!([1, 0]);
    assert!(decode(invalid).is_err());
}

fn cpu_output(model: &Model, input: &[[u8; 4]]) -> Vec<[u8; 4]> {
    let [width, height] = model.size.map(|size| size as usize);
    let settings = model.settings;
    let rgb_codes: Vec<_> = input
        .iter()
        .map(|pixel| pixel.map(|value| f32::from(value) / 255.))
        .collect();
    let luminance: Vec<f32> = rgb_codes
        .iter()
        .map(|rgb| (0..3).map(|i| rgb[i] * settings.luminance[i]).sum())
        .collect();
    let mut output = vec![[settings.biases[2]; 3]; input.len()];
    let sample = |x: usize, y: usize, [dx, dy, _, _]: [i32; 4]| {
        let x = x as i32 + dx;
        let y = y as i32 - dy;
        (x >= 0 && x < width as i32 && y >= 0 && y < height as i32)
            .then(|| y as usize * width + x as usize)
    };
    for [dx, dy] in PHASES {
        let location = |index: usize| {
            let x = (index % width + dx as usize) % TILE;
            let y = (height - 1 - index / width + dy as usize) % TILE;
            (x, y, y * TILE + x)
        };
        let mut first = luminance.clone();
        if settings.counts[0] != 0 {
            for (index, value) in first.iter_mut().enumerate() {
                let (_, _, texel) = location(index);
                let mut sum = settings.biases[0];
                for tap in 0..settings.counts[0] as usize {
                    if let Some(neighbor) =
                        sample(index % width, index / width, settings.hidden1[tap])
                    {
                        for channel in 0..3 {
                            sum += rgb_codes[neighbor][channel]
                                * model.weights[channel * TEXELS + texel][tap];
                        }
                    }
                }
                *value = sum.max(0.);
            }
        }
        let mut second = first.clone();
        if settings.counts[1] != 0 {
            for (index, value) in second.iter_mut().enumerate() {
                let (_, _, texel) = location(index);
                let mut sum = settings.biases[1];
                for tap in 0..settings.counts[1] as usize {
                    if let Some(neighbor) =
                        sample(index % width, index / width, settings.hidden2[tap])
                    {
                        sum += first[neighbor] * model.weights[3 * TEXELS + texel][tap];
                    }
                }
                *value = sum.max(0.);
                if settings.skips[0] != 0 {
                    *value += luminance[index];
                }
            }
        }
        let axis = |position: usize| {
            ((position as f32 + 0.5) * std::f32::consts::PI / 64.)
                .sin()
                .powi(2)
        };
        for (index, out) in output.iter_mut().enumerate() {
            let (x, y, texel) = location(index);
            let window = axis(x) * axis(y);
            for tap in 0..settings.counts[2] as usize {
                if let Some(neighbor) = sample(index % width, index / width, settings.readout[tap])
                {
                    for (channel, value) in out.iter_mut().enumerate() {
                        *value += window
                            * second[neighbor]
                            * model.weights[(4 + channel) * TEXELS + texel][tap];
                    }
                }
            }
        }
    }
    let bias = (settings.skips[2] != 0)
        .then(|| calibration::flat_field_bias(model))
        .flatten();
    output
        .iter()
        .enumerate()
        .map(|(index, out)| {
            let mut pixel = [255; 4];
            for channel in 0..3 {
                let mut value = out[channel];
                if settings.skips[1] != 0 {
                    value += luminance[index];
                }
                if model.add_input_rgb {
                    value += rgb_codes[index][channel];
                }
                if let Some(bias) = &bias {
                    let cell = rgb_codes[index].map(|code| code * (CALIBRATION_LEVELS - 1) as f32);
                    let low = cell.map(|coordinate| coordinate.floor() as usize);
                    let high = low.map(|coordinate| (coordinate + 1).min(CALIBRATION_LEVELS - 1));
                    let position =
                        (height - 1 - index / width) % PERIOD * PERIOD + index % width % PERIOD;
                    for corner in 0..8 {
                        let mut probe = 0;
                        let mut blend = 1.;
                        for component in 0..3 {
                            let fraction = cell[component].fract();
                            probe *= CALIBRATION_LEVELS;
                            if corner & (1 << component) != 0 {
                                probe += high[component];
                                blend *= fraction;
                            } else {
                                probe += low[component];
                                blend *= 1. - fraction;
                            }
                        }
                        value -= bias[probe * PERIOD * PERIOD + position][channel] * blend;
                    }
                }
                pixel[channel] = (value.clamp(0., 1.) * 255.).round() as u8;
            }
            pixel
        })
        .collect()
}

fn gpu_output(renderer: &Renderer, model: Model, input: &[[u8; 4]]) -> Vec<u8> {
    let device = &renderer.device;
    let queue = &renderer.queue;
    let size = model.size;
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let model = LayerModel::new(device, queue, format, size, model);
    let make = |usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer model parity"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let source = make(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    queue.write_texture(
        source.as_image_copy(),
        bytemuck::cast_slice(input),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: None,
        },
        extent,
    );
    let present = make(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let seed_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer model parity source"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../passthrough.wgsl").into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[layout_entry(0, texture_binding(false))],
    });
    let seed_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let seed_pipeline = pipeline(device, &seed_shader, &seed_layout, "fs", format);
    let seed_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&source.create_view(&Default::default())),
        }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    pass(
        &mut encoder,
        &seed_pipeline,
        &seed_group,
        &model.scene,
        size,
    );
    model.draw(
        queue,
        &mut encoder,
        &present.create_view(&Default::default()),
        size,
    );
    queue.submit(Some(encoder.finish()));
    renderer.rgba_pixels(&present).unwrap()
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_matches_cpu_for_color_codes_offsets_overlap_and_bypasses() {
    let renderer = pollster::block_on(Renderer::new(None, 73, 41)).unwrap();
    let mut models: Vec<_> = [false, true]
        .map(|disabled| decode(export(disabled)).unwrap())
        .into();
    models.push(Model::from_bytes(bundled_export()).unwrap());
    for model in models {
        let [width, height] = model.size;
        let input: Vec<[u8; 4]> = (0..width * height)
            .map(|i| {
                let x = i % width;
                let y = i / width;
                [
                    (x * 13 % 211) as u8,
                    (y * 31 % 163) as u8,
                    ((x + y * 3) * 7 % 197) as u8,
                    255,
                ]
            })
            .collect();
        let expected = cpu_output(&model, &input);
        let gpu = gpu_output(&renderer, model, &input);
        assert_eq!(gpu.len(), expected.len() * 4);
        for (index, (actual, expected)) in gpu.chunks_exact(4).zip(expected).enumerate() {
            assert!(
                actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
                "pixel {index} at {width} × {height}: {actual:?} versus {expected:?}"
            );
        }
    }
}

#[test]
fn calibration_keeps_each_channel_mean_and_flattens_a_gray_patch() {
    let mut export = export(true);
    let bytes: Vec<_> = (0..TEXELS)
        .flat_map(|index| {
            let weight = if index % PERIOD < PERIOD / 2 {
                0.5_f32
            } else {
                -0.25
            };
            [weight, 0., 0., 0.].into_iter().flat_map(f32::to_le_bytes)
        })
        .collect();
    export["layers"][2]["weights"][0]["data"] = json!(STANDARD.encode(bytes));
    let corrected = decode(export.clone()).unwrap();
    export["flatFieldCorrection"] = json!(false);
    let uncorrected = decode(export).unwrap();
    let [width, height] = corrected.size.map(|size| size as usize);
    let gray = vec![[64, 64, 64, 255]; width * height];
    let before = cpu_output(&uncorrected, &gray);
    let after = cpu_output(&corrected, &gray);
    let patch = |pixels: &[[u8; 4]]| {
        let mut min = 255;
        let mut max = 0;
        let mut sum = 0;
        for y in 4..36 {
            for x in 4..36 {
                let value = pixels[y * width + x][0];
                min = min.min(value);
                max = max.max(value);
                sum += u32::from(value);
            }
        }
        (max - min, sum)
    };
    let (range_before, sum_before) = patch(&before);
    let (range_after, sum_after) = patch(&after);
    assert!(
        range_before > 2,
        "fixture must contain a visible flat-field pattern"
    );
    assert!(range_after <= 1);
    assert!(sum_before.abs_diff(sum_after) <= 32 * 32);
}

#[test]
fn bundled_model_calibration_flattens_a_colored_sky_patch() {
    let corrected = Model::from_bytes(bundled_export()).unwrap();
    let mut uncorrected = Model::from_bytes(bundled_export()).unwrap();
    uncorrected.settings.skips[2] = 0;
    let [width, height] = corrected.size.map(|size| size as usize);
    let blue = vec![[64, 107, 173, 255]; width * height];
    let range = |model| {
        let pixels = cpu_output(model, &blue);
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for y in 32..64 {
            for x in 32..64 {
                let [r, g, b, _] = pixels[y * width + x].map(f32::from);
                let value = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                min = min.min(value);
                max = max.max(value);
            }
        }
        max - min
    };
    let before = range(&uncorrected);
    let after = range(&corrected);
    assert!(before > 10.);
    assert!(
        after < before * 0.1,
        "sky variation: {before:.2} → {after:.2}"
    );
}
