struct Model {
    hidden1: array<vec4<i32>, 4>,
    hidden2: array<vec4<i32>, 4>,
    readout: array<vec4<i32>, 4>,
    luminance: vec4<f32>,
    biases: vec4<f32>,
    counts: vec4<u32>,
    skips: vec4<u32>,
}

@group(0) @binding(0) var<uniform> model: Model;
@group(0) @binding(1) var weights: texture_2d<f32>;
@group(0) @binding(2) var input_image: texture_2d<f32>;
@group(0) @binding(3) var hidden_image: texture_2d<f32>;
@group(0) @binding(4) var flat_field_bias: texture_2d<f32>;

const TILE: u32 = 64u;
const STRIDE: u32 = 32u;
const PHASES = array<vec2<u32>, 4>(
    vec2<u32>(0u, 0u), vec2<u32>(STRIDE, 0u),
    vec2<u32>(0u, STRIDE), vec2<u32>(STRIDE, STRIDE)
);

@vertex fn vs(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(points[vertex], 0.0, 1.0);
}

fn in_frame(pixel: vec2<i32>, size: vec2<i32>) -> bool {
    return all(pixel >= vec2<i32>(0)) && all(pixel < size);
}

fn weight_location(pixel: vec2<i32>, height: i32, phase: u32) -> vec2<u32> {
    // WebGL exports both the tile phase and its texture rows in bottom-up coordinates.
    let logical = vec2<u32>(u32(pixel.x), u32(height - 1 - pixel.y));
    return (logical + PHASES[phase]) % TILE;
}

fn weight_texel(plane: u32, location: vec2<u32>) -> vec4<f32> {
    return textureLoad(weights, vec2<i32>(i32(location.x), i32(plane * TILE + location.y)), 0);
}

fn tap_pixel(pixel: vec2<i32>, offset: vec2<i32>) -> vec2<i32> {
    return pixel + vec2<i32>(offset.x, -offset.y);
}

@fragment fn hidden1(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(input_image));
    if model.counts.x == 0u {
        return vec4<f32>(dot(textureLoad(input_image, pixel, 0).rgb, model.luminance.xyz));
    }

    var red = vec4<f32>(0.0);
    var green = vec4<f32>(0.0);
    var blue = vec4<f32>(0.0);
    for (var tap = 0u; tap < model.counts.x; tap++) {
        let source = tap_pixel(pixel, model.hidden1[tap].xy);
        if in_frame(source, size) {
            let rgb = textureLoad(input_image, source, 0).rgb;
            red[tap] = rgb.r;
            green[tap] = rgb.g;
            blue[tap] = rgb.b;
        }
    }
    var output = vec4<f32>(0.0);
    for (var phase = 0u; phase < 4u; phase++) {
        let location = weight_location(pixel, size.y, phase);
        let activation = model.biases.x + dot(weight_texel(0u, location), red)
            + dot(weight_texel(1u, location), green) + dot(weight_texel(2u, location), blue);
        output[phase] = max(activation, 0.0);
    }
    return output;
}

@fragment fn hidden2(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(hidden_image));
    if model.counts.y == 0u {
        return textureLoad(hidden_image, pixel, 0);
    }

    var neighbor: array<vec4<f32>, 4>;
    for (var tap = 0u; tap < model.counts.y; tap++) {
        let source = tap_pixel(pixel, model.hidden2[tap].xy);
        if in_frame(source, size) {
            neighbor[tap] = textureLoad(hidden_image, source, 0);
        }
    }
    var center = 0.0;
    if model.skips.x != 0u {
        center = dot(textureLoad(input_image, pixel, 0).rgb, model.luminance.xyz);
    }
    var output = vec4<f32>(0.0);
    for (var phase = 0u; phase < 4u; phase++) {
        let location = weight_location(pixel, size.y, phase);
        let taps = vec4<f32>(neighbor[0][phase], neighbor[1][phase], neighbor[2][phase], neighbor[3][phase]);
        output[phase] = center + max(model.biases.y + dot(weight_texel(3u, location), taps), 0.0);
    }
    return output;
}

fn axis_weight(position: u32) -> f32 {
    // The complementary half-block windows sum to one at every pixel center.
    let sine = sin((f32(position % STRIDE) + 0.5) * (3.141592653589793 / f32(TILE)));
    let weight = sine * sine;
    return select(1.0 - weight, weight, position < STRIDE);
}

fn bias_at_probe(tile: vec2<i32>, probe: vec3<i32>) -> vec3<f32> {
    let index = (probe.r * 5 + probe.g) * 5 + probe.b;
    let origin = vec2<i32>(index % 8, index / 8) * i32(STRIDE);
    return textureLoad(flat_field_bias, tile + origin, 0).rgb;
}

fn bias_at_color(tile: vec2<i32>, rgb: vec3<f32>) -> vec3<f32> {
    let cell = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 4.0;
    let low = vec3<i32>(floor(cell));
    let high = min(low + 1, vec3<i32>(4));
    let f = fract(cell);
    let blue00 = mix(bias_at_probe(tile, low), bias_at_probe(tile, vec3<i32>(low.rg, high.b)), f.b);
    let blue01 = mix(bias_at_probe(tile, vec3<i32>(low.r, high.g, low.b)), bias_at_probe(tile, vec3<i32>(low.r, high.gb)), f.b);
    let blue10 = mix(bias_at_probe(tile, vec3<i32>(high.r, low.gb)), bias_at_probe(tile, vec3<i32>(high.r, low.g, high.b)), f.b);
    let blue11 = mix(bias_at_probe(tile, vec3<i32>(high.rg, low.b)), bias_at_probe(tile, high), f.b);
    return mix(mix(blue00, blue01, f.g), mix(blue10, blue11, f.g), f.r);
}

@fragment fn readout(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let size = vec2<i32>(textureDimensions(hidden_image));
    var neighbor: array<vec4<f32>, 4>;
    for (var tap = 0u; tap < model.counts.z; tap++) {
        let source = tap_pixel(pixel, model.readout[tap].xy);
        if in_frame(source, size) {
            neighbor[tap] = textureLoad(hidden_image, source, 0);
        }
    }

    var output = vec3<f32>(model.biases.z);
    if model.skips.y != 0u {
        output += vec3<f32>(dot(textureLoad(input_image, pixel, 0).rgb, model.luminance.xyz));
    }
    for (var phase = 0u; phase < 4u; phase++) {
        let location = weight_location(pixel, size.y, phase);
        let taps = vec4<f32>(neighbor[0][phase], neighbor[1][phase], neighbor[2][phase], neighbor[3][phase]);
        let rgb = vec3<f32>(
            dot(weight_texel(4u, location), taps),
            dot(weight_texel(5u, location), taps),
            dot(weight_texel(6u, location), taps)
        );
        output += axis_weight(location.x) * axis_weight(location.y) * rgb;
    }
    if model.skips.z != 0u {
        let tile = vec2<i32>(weight_location(pixel, size.y, 0u) % STRIDE);
        output -= bias_at_color(tile, textureLoad(input_image, pixel, 0).rgb);
    }
    return vec4<f32>(output, 1.0);
}
