@group(0) @binding(0) var scene: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 3>(vec2<f32>(-1.0,-1.0), vec2<f32>(3.0,-1.0), vec2<f32>(-1.0,3.0));
    return vec4<f32>(points[index], 0.0, 1.0);
}
@fragment fn fs(@builtin(position) screen: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(scene, vec2<i32>(screen.xy), 0).rgb, 1.0);
}
