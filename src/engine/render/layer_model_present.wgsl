struct Presentation {
    viewport: vec2<f32>,
    model_size: vec2<f32>,
    add_input_rgb: u32,
    padding1: u32,
    padding2: u32,
    padding3: u32,
}

@group(0) @binding(0) var<uniform> presentation: Presentation;
@group(0) @binding(1) var scene_color: texture_2d<f32>;
@group(0) @binding(2) var model_result: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(points[vertex], 0.0, 1.0);
}

fn scene_code_at(pixel: vec2<i32>) -> vec3<f32> {
    let linear = textureLoad(scene_color, pixel, 0).rgb;
    return select(1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * linear, linear <= vec3<f32>(0.0031308));
}

@fragment fn resample(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let scene_pixel = position.xy * presentation.viewport / presentation.model_size;
    let bounded = clamp(scene_pixel - 0.5, vec2<f32>(0.0), presentation.viewport - 1.0);
    let origin = vec2<i32>(floor(bounded));
    let next = min(origin + 1, vec2<i32>(presentation.viewport) - 1);
    let fraction = fract(bounded);
    // Match the prototype's interpolation in sRGB code values.
    let top = mix(scene_code_at(origin), scene_code_at(vec2<i32>(next.x, origin.y)), fraction.x);
    let bottom = mix(scene_code_at(vec2<i32>(origin.x, next.y)), scene_code_at(next), fraction.x);
    return vec4<f32>(mix(top, bottom, fraction.y), 1.0);
}

fn result_at(pixel: vec2<i32>) -> vec3<f32> {
    let max_pixel = vec2<i32>(textureDimensions(model_result)) - 1;
    return textureLoad(model_result, clamp(pixel, vec2<i32>(0), max_pixel), 0).rgb;
}

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return select(pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb / 12.92, rgb <= vec3<f32>(0.04045));
}

@fragment fn present(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample_pixel = position.xy * presentation.model_size / presentation.viewport - 0.5;
    let origin = vec2<i32>(floor(sample_pixel));
    let fraction = fract(sample_pixel);
    let top = mix(result_at(origin), result_at(origin + vec2<i32>(1, 0)), fraction.x);
    let bottom = mix(result_at(origin + vec2<i32>(0, 1)), result_at(origin + vec2<i32>(1, 1)), fraction.x);
    var rgb = mix(top, bottom, fraction.y);
    if presentation.add_input_rgb != 0u {
        // Keep native-resolution texture and motion in the RGB correction mode.
        rgb += scene_code_at(vec2<i32>(position.xy));
    }
    return vec4<f32>(srgb_to_linear(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}
