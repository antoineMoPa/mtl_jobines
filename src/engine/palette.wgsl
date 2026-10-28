struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};
@vertex fn vs(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> Varying {
    return Varying(vec4<f32>(position, 0., 1.), uv, color);
}
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@fragment fn fs(v: Varying) -> @location(0) vec4<f32> {
    let color = v.color * textureSample(atlas, atlas_sampler, v.uv);
    // egui blends premultiplied gamma colors; the target is an sRGB view.
    let linear = select(color.rgb / 12.92, pow((color.rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), color.rgb > vec3<f32>(0.04045));
    return vec4<f32>(linear, color.a);
}
