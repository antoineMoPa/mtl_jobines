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
// The player rigs have wrists but no finger joints. Curl the T-pose fingers
// around the knuckles before skinning, for both the mesh and its shadow.
fn combat_position(pos: vec3<f32>, actor_state: f32) -> vec3<f32> {
    let span = abs(pos.x);
    let thumb = pos.z > 0.056;
    let knuckle = select(0.785, 0.767, thumb);
    if actor_state >= 0.0 || span <= knuckle { return pos; }
    let radius = select(0.027, 0.024, thumb);
    let bend = min((span-knuckle)/radius, 3.6);
    let offset = radius + pos.y - 1.301;
    return vec3<f32>(sign(pos.x)*(knuckle+offset*sin(bend)), 1.301-radius+offset*cos(bend),
                     pos.z-select(0.0, 0.035*sin(bend*0.65), thumb));
}
// Loose edges move in source metres before skinning, including in the shadow pass.
fn dressed_position(v: Input, actor_state: f32) -> vec3<f32> {
    var p = combat_position(v.position, actor_state);
    let phase = camera.eye_time.w * 2.7 + p.x * 13.0;
    if v.surface.y == 2.0 {
        let tips = smoothstep(0.55, 1.0, v.surface.w) * (1.0 - smoothstep(1.38, 1.58, p.y));
        p.x += 0.003 * tips * sin(phase);
        p.z += 0.004 * tips * sin(phase * 0.77);
    } else if v.surface.y == 1.0 {
        let shirt = 1.0 - smoothstep(0.0, 0.10, abs(p.y - 0.93));
        let shorts = 1.0 - smoothstep(0.0, 0.07, abs(p.y - 0.56));
        p += v.normal * (0.0025 * max(shirt, shorts) * sin(phase + p.z * 16.0));
    }
    return p;
}
@vertex fn vs_pedestrian(v: Input, instance: VehicleTransform, skin: SkinInput) -> Output {
    let bones=pedestrian_skin(skin,u32(instance.z.w));
    let model=mat4x4<f32>(instance.x,instance.y,instance.z,instance.translation);
    let world=(model*bones*vec4<f32>(dressed_position(v,instance.y.w),1.0)).xyz;
    var o: Output;
    o.clip=camera.view_projection*vec4<f32>(world,1.0);
    o.world=world;
    o.normal=(model*bones*vec4<f32>(v.normal,0.0)).xyz*100.0;
    o.color=v.color;
    o.surface=vec4<f32>(7.0,instance.translation.w,instance.y.w,instance.x.w);
    o.fence_uv=v.surface.zw;
    o.snow_control=vec2<f32>(v.surface.y,1.0);
    return o;
}
@vertex fn vs_pedestrian_shadow(v: Input, instance: VehicleTransform, skin: SkinInput) -> ShadowOutput {
    let bones=pedestrian_skin(skin,u32(instance.z.w));
    let model=mat4x4<f32>(instance.x,instance.y,instance.z,instance.translation);
    var o: ShadowOutput;
    o.clip=camera.light_view_projection*vec4<f32>((model*bones*vec4<f32>(dressed_position(v,instance.y.w),1.0)).xyz,1.0);
    o.fence_uv=vec2<f32>(0.0);
    o.kind_pattern=vec2<f32>(7.0,instance.translation.w);
    return o;
}
@fragment fn fs_pedestrian(v: Output, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var material=v;
    let albedo=textureSample(pedestrian_albedo,pedestrian_sampler,v.fence_uv,i32(v.surface.w));
    material.color *= select(vec3(1.0), albedo.rgb, v.snow_control.x == 0.0);
    material.snow_control.x = 0.0;
    // Across-strand UV is constant along a lock. Its screen derivatives give
    // the deformed strand direction even when the head turns.
    let fiber = dpdx(v.world) * dpdy(v.fence_uv.x) - dpdy(v.world) * dpdx(v.fence_uv.x);
    if v.snow_control.x == 2.0 {
        let tangent = fiber * inverseSqrt(max(dot(fiber, fiber), 1e-22));
        let eye = camera.eye_time.xyz - v.world;
        let half_dir = camera.sun_exposure.xyz + eye * inverseSqrt(max(dot(eye, eye), 1e-10));
        let along = dot(tangent, half_dir) * inverseSqrt(max(dot(half_dir, half_dir), 1e-8));
        let sheen = pow(clamp(1.0 - along * along, 0.0, 1.0), 18.0);
        material.color += vec3(0.10, 0.061, 0.035) * sheen * camera.daylight_sky.x;
    }
    if v.surface.z>0.5 {
        let luma=dot(material.color,vec3(0.26,0.57,0.17));
        material.color=mix(material.color,vec3(0.42,0.64,0.41)*luma,0.77);
    }
    material.surface.w=0.0;
    return shade_local_surface(material,front,clamp(material.surface.y,0.0,1.0),prepare_surface(material));
}
