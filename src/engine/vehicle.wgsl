// Vehicle geometry stays in metres on the GPU. One instance matrix per body/wheel part.
struct VehicleTransform {
    @location(4) x: vec4<f32>, @location(5) y: vec4<f32>,
    @location(6) z: vec4<f32>, @location(7) translation: vec4<f32>,
};
@vertex fn vs_vehicle(v: Input, instance: VehicleTransform) -> Output {
    let model = mat4x4<f32>(instance.x, instance.y, instance.z, instance.translation);
    let world = (model * vec4<f32>(v.position, 1.0)).xyz;
    var o: Output;
    o.clip = camera.view_projection * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = (model * vec4<f32>(v.normal, 0.0)).xyz * 100.0;
    o.color = v.color; o.surface = v.surface; o.surface.y *= instance.translation.w; o.fence_uv = v.surface.yz;
    return o;
}
@vertex fn vs_vehicle_shadow(v: Input, instance: VehicleTransform) -> ShadowOutput {
    let model = mat4x4<f32>(instance.x, instance.y, instance.z, instance.translation);
    var o: ShadowOutput;
    o.clip = camera.light_view_projection * vec4<f32>((model * vec4<f32>(v.position, 1.0)).xyz, 1.0);
    o.fence_uv = v.surface.yz; o.kind_pattern = vec2<f32>(v.surface.x, v.surface.y * instance.translation.w);
    return o;
}
// Alpha-to-coverage retains depth-correct seats behind glass without sorting
// every triangle of each moving vehicle. Opaque parts still cover all samples.
@fragment fn fs_vehicle(v: Output, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return shade_surface(v, front, clamp(v.surface.y, 0.0, 1.0), prepare_surface(v));
}
@fragment fn fs_vehicle_shadow(v: ShadowOutput) {
    let pattern = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let pixel = vec2<u32>(v.clip.xy) % vec2<u32>(4u);
    let threshold = (pattern[pixel.y * 4u + pixel.x] + 0.5) / 16.0;
    if v.kind_pattern.y <= threshold { discard; }
}
