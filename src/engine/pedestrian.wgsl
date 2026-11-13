// Linear blend skinning in source metres, followed by the explicit game axis map.
@group(0) @binding(2) var<storage,read> pedestrian_poses: array<mat4x4<f32>>;
@group(0) @binding(3) var pedestrian_albedo: texture_2d_array<f32>;
@group(0) @binding(4) var pedestrian_sampler: sampler;
struct SkinInput {
    @location(8) joints: vec4<u32>,
    @location(9) weights: vec4<f32>,
};
fn pedestrian_skin(skin: SkinInput, base: u32) -> mat4x4<f32> {
    return pedestrian_poses[base+skin.joints.x]*skin.weights.x
        + pedestrian_poses[base+skin.joints.y]*skin.weights.y
        + pedestrian_poses[base+skin.joints.z]*skin.weights.z
        + pedestrian_poses[base+skin.joints.w]*skin.weights.w;
}
@vertex fn vs_pedestrian(v: Input, instance: VehicleTransform, skin: SkinInput) -> Output {
    let bones=pedestrian_skin(skin,u32(instance.z.w));
    let model=mat4x4<f32>(instance.x,instance.y,instance.z,instance.translation);
    let world=(model*bones*vec4<f32>(v.position,1.0)).xyz;
    var o: Output;
    o.clip=camera.view_projection*vec4<f32>(world,1.0);
    o.world=world;
    o.normal=(model*bones*vec4<f32>(v.normal,0.0)).xyz*100.0;
    o.color=v.color;
    o.surface=vec4<f32>(7.0,instance.translation.w,0.0,instance.x.w);
    o.fence_uv=v.surface.zw;
    o.snow_control=vec2<f32>(0.0,1.0);
    return o;
}
@vertex fn vs_pedestrian_shadow(v: Input, instance: VehicleTransform, skin: SkinInput) -> ShadowOutput {
    let bones=pedestrian_skin(skin,u32(instance.z.w));
    let model=mat4x4<f32>(instance.x,instance.y,instance.z,instance.translation);
    var o: ShadowOutput;
    o.clip=camera.light_view_projection*vec4<f32>((model*bones*vec4<f32>(v.position,1.0)).xyz,1.0);
    o.fence_uv=vec2<f32>(0.0);
    o.kind_pattern=vec2<f32>(7.0,instance.translation.w);
    return o;
}
@fragment fn fs_pedestrian(v: Output, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var material=v;
    let albedo=textureSample(pedestrian_albedo,pedestrian_sampler,v.fence_uv,i32(v.surface.w));
    material.color*=albedo.rgb;
    material.surface.w=0.0;
    return shade_local_surface(material,front,clamp(material.surface.y,0.0,1.0),prepare_surface(material));
}
