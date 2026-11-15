// Two monochrome paint masks: R=shade, G=highlight, A=coverage. Colors stay runtime data.
@group(3) @binding(3) var graffiti_masks:texture_2d_array<f32>;
@group(3) @binding(4) var graffiti_sampler:sampler;
struct PaintSettings {color_override:vec4<f32>,ranges:vec4<f32>};
@group(3) @binding(5) var<uniform> paint_settings:PaintSettings;
@fragment fn fs_graffiti(v:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    let packed=u32(v.surface.w);
    let tex=textureSample(graffiti_masks,graffiti_sampler,v.fence_uv,i32(packed&1u));
    let seed=f32((packed>>17u)&127u);
    let opacity=mix(paint_settings.ranges.x,paint_settings.ranges.y,f32((packed>>1u)&255u)/255.0);
    let wash=mix(paint_settings.ranges.z,paint_settings.ranges.w,f32((packed>>9u)&255u)/255.0);
    let tangent=vec3<f32>(v.normal.y,-v.normal.x,0.0);
    let p=vec2<f32>(dot(v.world,tangent),v.world.z)*100.0;
    // World-fixed erosion: no clock input, so paint never crawls as the camera moves.
    let patches=noise(p*3.8+vec2<f32>(seed,seed*1.73));
    let grain=noise(p*39.0+vec2<f32>(seed*2.1));
    var erosion=smoothstep(wash*0.94-0.15,wash*0.94+0.10,patches);
    if wash<=0.0 {erosion=1.0;}
    if wash>=1.0 {erosion=0.0;}
    let distant=1.0-smoothstep(1.1,1.5,length(camera.eye_time.xyz-v.world));
    let coverage=tex.a*clamp(opacity,0.0,1.0)*erosion*mix(0.60,1.0,grain)*distant;
    let base=mix(v.color,paint_settings.color_override.rgb,clamp(paint_settings.color_override.w,0.0,1.0));
    var paint=v;
    paint.color=mix(base*0.16,base,smoothstep(0.10,0.86,tex.r));
    paint.color=mix(paint.color,base*0.55+vec3<f32>(0.38,0.37,0.32),tex.g*0.36);
    paint.surface=vec4<f32>(7.0,0.0,0.0,0.0);
    let surface=prepare_surface(paint);
    if coverage<0.001 {discard;}
    return shade_local_surface(paint,front,coverage,surface);
}
