// Vehicle geometry stays in metres on the GPU. One instance matrix per body/wheel part.
@group(3) @binding(6) var vehicle_albedo: texture_2d_array<f32>;
@group(3) @binding(7) var vehicle_albedo_sampler: sampler;
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
    let excluded=v.surface.y<0.999 || (v.surface.x>=0.0 && (v.surface.w== -3.0 || v.surface.w>=64.0)) || (v.surface.x<0.0 && fract(-v.surface.x)>0.1);
    o.snow_control=vec2<f32>(select(1.0,0.0,excluded),1.0);
    if v.surface.x<0.0 {
        o.fence_uv=v.surface.zw;
        o.surface.w=select(0.0,64.0,fract(-v.surface.x)==0.25 && instance.x.w>0.0);
    }
    if v.surface.x>=0.0 && floor(v.surface.z)==1.0 && v.surface.w==0.0 {
        let variant=u32(instance.z.w);
        if variant==1u {o.color=vec3<f32>(0.035,0.09,0.19);}
        else if variant==2u {o.color=vec3<f32>(0.86,0.85,0.80);}
        else if variant==3u {o.color=vec3<f32>(0.30,0.022,0.045);}
    }
    if v.surface.x>=0.0 && v.surface.w==64.0 {o.surface.w*=instance.x.w;}
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
    let uv_dx=dpdx(v.fence_uv); let uv_dy=dpdy(v.fence_uv);
    var material=v;
    if v.surface.x<0.0 {
        let albedo=textureSampleGrad(vehicle_albedo,vehicle_albedo_sampler,v.fence_uv,i32(floor(-v.surface.x))-1,uv_dx,uv_dy);
        material.color*=albedo.rgb;
        material.surface=vec4<f32>(7.0,v.surface.y,0.0,v.surface.w);
    }
    var surface=prepare_surface(material);
    surface.finish=vec3<f32>(0.65,0.0,0.0);
    if v.surface.x>=0.0 && fract(v.surface.z)>=0.5 {
        let packed=u32(round(fract(v.surface.z)*65536.0));
        surface.finish=vec3<f32>(max(f32(packed&127u)/127.0,0.07),f32((packed>>7u)&127u)/127.0,f32((packed>>14u)&1u)*0.7);
    }
    // Glass remains dielectric; MSAA coverage and authored tint remain intact.
    if material.surface.y<0.999 {surface.finish=vec3<f32>(0.12,0.0,0.0);}
    if surface.finish.z>0.0 {
        // Broad finish variation stays stable under motion; microdetail fades
        // before it becomes subpixel, rather than sparkling in the distance.
        let footprint=max(length(surface.world_dx),length(surface.world_dy))*100.0;
        let detail=1.0-smoothstep(0.002,0.008,footprint);
        surface.finish.x=clamp(surface.finish.x+(noise(v.world.xy*18000.0)-0.5)*0.035*detail,0.07,0.95);
    }
    // Roof beacons retain their authored lens colors without automatic emission.
    if material.surface.w==64.0 {
        let night=1.0-clamp(camera.daylight_sky.x,0.0,1.0);
        return mix(shade_local_surface(material,front,clamp(material.surface.y,0.0,1.0),surface),lamp_color(material),night*night);
    }
    return shade_local_surface(material, front, clamp(material.surface.y, 0.0, 1.0), surface);
}
@fragment fn fs_vehicle_shadow(v: ShadowOutput) {
    let pattern = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let pixel = vec2<u32>(v.clip.xy) % vec2<u32>(4u);
    let threshold = (pattern[pixel.y * 4u + pixel.x] + 0.5) / 16.0;
    if v.kind_pattern.y <= threshold { discard; }
}
