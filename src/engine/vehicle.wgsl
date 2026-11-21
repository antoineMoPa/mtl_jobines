// Set by world_pipelines for screenshot and MSAA rendering paths.
const VEHICLE_SAMPLE_COUNT: u32 = 4u;
fn vehicle_alpha_threshold(position: vec2<f32>) -> f32 {
    let pattern = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let pixel = vec2<u32>(position) % vec2<u32>(4u);
    return (pattern[pixel.y * 4u + pixel.x] + 0.5) / 16.0;
}
// Vehicle geometry stays in metres on the GPU. One instance matrix per body/wheel part.
@group(3) @binding(6) var vehicle_albedo: texture_2d_array<f32>;
@group(3) @binding(7) var vehicle_albedo_sampler: sampler;
struct VehicleTransform {
    @location(4) x: vec4<f32>, @location(5) y: vec4<f32>,
    @location(6) z: vec4<f32>, @location(7) translation: vec4<f32>,
};
@vertex fn vs_vehicle(v: Input, instance: VehicleTransform) -> Output {
    let model = mat4x4<f32>(instance.x, instance.y, instance.z, instance.translation);
    var world = (model * vec4<f32>(v.position, 1.0)).xyz;
    let tree=v.surface.x>=15.0 && v.surface.x<=23.0;
    let image=v.surface.x>=24.0 && v.surface.x<=29.0;
    if tree {world=tree_vertex(world,instance.translation.xyz);}
    if image {
        let direction=camera.eye_time.xy-instance.translation.xy;
        let right=normalize(vec2<f32>(-direction.y,direction.x));
        world=instance.translation.xyz+vec3<f32>(right*v.position.x/100.0,v.position.z/100.0);
        world=tree_vertex(world,instance.translation.xyz);
    }
    var o: Output;
    o.clip = camera.view_projection * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = (model * vec4<f32>(v.normal, 0.0)).xyz * 100.0;
    if instance.y.w< -0.5 {
        let a=instance.x.xyz;let b=instance.y.xyz;let c=instance.z.xyz;
        let determinant=dot(a,cross(b,c));
        let normal=(cross(b,c)*v.normal.x+cross(c,a)*v.normal.y+cross(a,b)*v.normal.z)/determinant;
        o.normal=normal/max(length(normal),0.000001);o.color=v.color;o.surface=v.surface;o.fence_uv=v.surface.yz;
        o.snow_control=vec2<f32>(select(1.0,0.0,v.surface.w== -3.0),-1.0);return o;
    }
    o.color = v.color; o.surface = v.surface; if !tree {o.surface.y *= instance.translation.w;} o.fence_uv = v.surface.yz;
    let excluded=v.surface.y<0.999 || (v.surface.x>=0.0 && (v.surface.w== -3.0 || v.surface.w>=64.0)) || (v.surface.x<0.0 && fract(-v.surface.x)>0.1);
    o.snow_control=vec2<f32>(select(1.0,0.0,excluded || tree),1.0);
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
    if instance.y.w > 1.5 && (v.surface.w == 65.0 || v.surface.w == 66.0) {
        let red_phase = fract(camera.eye_time.w * 2.0) < 0.5;
        if red_phase == (v.surface.w == 65.0) {o.surface.w = 67.0;}
    }
    if tree || image {o.surface.w=instance.x.w;}
    return o;
}
@vertex fn vs_vehicle_shadow(v: Input, instance: VehicleTransform) -> ShadowOutput {
    let model = mat4x4<f32>(instance.x, instance.y, instance.z, instance.translation);
    var o: ShadowOutput;
    var world=(model * vec4<f32>(v.position, 1.0)).xyz;
    if v.surface.x>=15.0 && v.surface.x<=23.0 {world=tree_vertex(world,instance.translation.xyz);}
    o.clip = camera.light_view_projection * vec4<f32>(world, 1.0);
    o.fence_uv = v.surface.yz; o.kind_pattern = vec2<f32>(v.surface.x, v.surface.y * instance.translation.w);
    if v.surface.x<0.0 {o.fence_uv=v.surface.zw;}
    if v.surface.x>=15.0 && v.surface.x<=29.0 {o.kind_pattern.y=instance.x.w;}
    if instance.y.w< -0.5 {o.kind_pattern.y=1.0;}
    return o;
}
// Alpha-to-coverage retains depth-correct seats behind glass without sorting
// every triangle of each moving vehicle. Opaque parts still cover all samples.
@fragment fn fs_vehicle(v: Output, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let uv_dx=dpdx(v.fence_uv); let uv_dy=dpdy(v.fence_uv);
    let world_dx=dpdx(v.world); let world_dy=dpdy(v.world);
    var material=v;
    if v.surface.x<0.0 {
        let albedo=textureSampleGrad(vehicle_albedo,vehicle_albedo_sampler,v.fence_uv,i32(floor(-v.surface.x))-1,uv_dx,uv_dy);
        material.color*=albedo.rgb;
        material.surface.y*=albedo.a;
        material.surface=vec4<f32>(7.0,material.surface.y,0.0,v.surface.w);
    }
    var surface=prepare_surface(material);
    if v.snow_control.y< -0.5 {

        return shade_local_surface(v,front,1.0,surface);
    }
    // Single-sample captures do not have alpha-to-coverage.
    let threshold=select(0.01,vehicle_alpha_threshold(v.clip.xy),VEHICLE_SAMPLE_COUNT==1u);
    if v.surface.x>=15.0 && v.surface.x<=29.0 {tree_lod_mask(v.clip.xy,v.surface.w);}
    if v.surface.x>=24.0 && v.surface.x<=29.0 {return tree_image_color(v,uv_dx,uv_dy);}
    if v.surface.x>=15.0 && v.surface.x<=23.0 {return tree_color(v,front,uv_dx,uv_dy,world_dx,world_dy,surface);}
    // Tree materials carry mesh UVs in surface.y. Vehicle materials carry opacity.
    if material.surface.y<=threshold {discard;}
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
    if material.surface.w == 67.0 {
        return vec4<f32>(max(material.color * 2.6, vec3<f32>(0.10)), material.surface.y);
    }
    if material.surface.w==64.0 {
        let night=1.0-clamp(camera.daylight_sky.x,0.0,1.0);
        return mix(shade_local_surface(material,front,clamp(material.surface.y,0.0,1.0),surface),lamp_color(material),night*night);
    }
    return shade_local_surface(material, front, clamp(material.surface.y, 0.0, 1.0), surface);
}
@fragment fn fs_vehicle_shadow(v: ShadowOutput) {
    if v.kind_pattern.x>=24.0 && v.kind_pattern.x<=29.0 {discard;}
    if v.kind_pattern.x>=15.0 && v.kind_pattern.x<=23.0 {tree_lod_mask(v.clip.xy,v.kind_pattern.y);tree_shadow(v);return;}
    let pattern = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    let pixel = vec2<u32>(v.clip.xy) % vec2<u32>(4u);
    let threshold = (pattern[pixel.y * 4u + pixel.x] + 0.5) / 16.0;
    var coverage=v.kind_pattern.y;
    if v.kind_pattern.x<0.0 {
        coverage*=textureSampleLevel(vehicle_albedo,vehicle_albedo_sampler,v.fence_uv,i32(floor(-v.kind_pattern.x))-1,0.0).a;
    }
    if coverage <= threshold { discard; }
}
