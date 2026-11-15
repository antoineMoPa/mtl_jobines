@group(0) @binding(0) var<uniform> view_projection: mat4x4<f32>;

struct Output {
    @builtin(position) clip: vec4<f32>,
    @location(0) alpha: f32,
};

@vertex fn vs(@location(0) vertex: vec4<f32>) -> Output {
    var output: Output;
    output.clip = view_projection * vec4<f32>(vertex.xyz, 1.0);
    output.alpha = vertex.w;
    return output;
}

@fragment fn fs(input: Output) -> @location(0) vec4<f32> {
    return vec4<f32>(0.08, 0.48, 1.0, input.alpha);
}
