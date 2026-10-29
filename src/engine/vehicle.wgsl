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
    o.color = v.color; o.surface = v.surface; o.fence_uv = v.surface.yz;
    return o;
}
@vertex fn vs_vehicle_shadow(v: Input, instance: VehicleTransform) -> ShadowOutput {
    let model = mat4x4<f32>(instance.x, instance.y, instance.z, instance.translation);
    var o: ShadowOutput;
    o.clip = camera.light_view_projection * model * vec4<f32>(v.position, 1.0);
    o.fence_uv = v.surface.yz; o.kind_pattern = v.surface.xw;
    return o;
}
