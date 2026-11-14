struct LocalLight {
    position_range:vec4<f32>, direction_outer:vec4<f32>,
    color_intensity:vec4<f32>, beam:vec4<f32>,
};
struct LightGrid {
    origin:vec4<f32>, lights:array<LocalLight,128>, cells:array<vec4<u32>,256>,
};
@group(0) @binding(1) var<uniform> light_grid:LightGrid;
fn local_irradiance(world:vec3<f32>,normal:vec3<f32>)->vec3<f32> {
    let night=1.0-clamp(camera.daylight_sky.x,0.0,1.0);
    if night==0.0 {return vec3<f32>(0.0);}
    let cell=vec2<i32>(floor((world.xy-light_grid.origin.xy)*light_grid.origin.z));
    if any(cell<vec2<i32>(0)) || any(cell>=vec2<i32>(16)) {return vec3<f32>(0.0);}
    let masks=light_grid.cells[u32(cell.y)*16u+u32(cell.x)];
    var irradiance=vec3<f32>(0.0);
    for(var word=0u;word<4u;word++) {
        var bits=masks[word];
        while bits!=0u {
            let index=word*32u+firstTrailingBit(bits);
            bits=bits&(bits-1u);
            let light=light_grid.lights[index];
            let delta=light.position_range.xyz-world;
            let d2=dot(delta,delta);
            let radius=light.position_range.w;
            if d2>=radius*radius {continue;}
            let to_light=delta*inverseSqrt(max(d2,0.00000001));
            let cone=smoothstep(light.direction_outer.w,light.beam.x,-dot(to_light,light.direction_outer.xyz));
            let edge=max(1.0-d2/(radius*radius),0.0);
            let meters2=d2*10000.0;
            let attenuation=edge*edge*cone*night*night/(1.0+meters2/light.beam.y);
            irradiance+=light.color_intensity.rgb*light.color_intensity.w*attenuation*max(dot(normal,to_light),0.0);
        }
    }
    return irradiance;
}
fn shade_local_surface(v:Output,front:bool,coverage:f32,surface:SurfaceData)->vec4<f32> {
    var seasonal=surface;
    seasonal.snow=winter_coverage(v,surface);
    if v.surface.x>0.5 && v.surface.x<1.5 {
        return shade_grass(v,front,coverage,seasonal);
    }
    var n=surface.normal;
    if v.surface.x>10.5&&v.surface.x<11.5&&!front {n=-n;}
    return shade_surface_lit(v,front,coverage,seasonal,local_irradiance(v.world,n));
}
