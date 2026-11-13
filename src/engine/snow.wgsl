// Shared material preparation supplies zero snow to standalone consumers.
@group(1) @binding(5) var snow_exposure:texture_2d<f32>;
@group(1) @binding(6) var snow_cleared:texture_2d<f32>;
@group(1) @binding(7) var<uniform> snow_cache:vec4<f32>;
fn cleared_deck(pixel:vec2<i32>,height:f32)->f32 {
    if any(pixel<vec2<i32>(0))||any(pixel>=vec2<i32>(i32(snow_cache.w))) {return 0.0;}
    // Classify each elevation first: filtering raw heights would mix unrelated decks.
    let decks=textureLoad(snow_cleared,pixel,0);
    return select(0.0,1.0,any(abs(decks-vec4<f32>(height))<vec4<f32>(0.0035)));
}
fn cleared_coverage(world:vec3<f32>)->f32 {
    let grid=(world.xy-snow_cache.xy)*snow_cache.z-0.5;
    let pixel=vec2<i32>(floor(grid));
    let blend=smoothstep(vec2<f32>(0.0),vec2<f32>(1.0),fract(grid));
    return mix(mix(cleared_deck(pixel,world.z),cleared_deck(pixel+vec2<i32>(1,0),world.z),blend.x),
        mix(cleared_deck(pixel+vec2<i32>(0,1),world.z),cleared_deck(pixel+vec2<i32>(1,1),world.z),blend.x),blend.y);
}
fn winter_coverage(v:Output,surface:SurfaceData)->f32 {
    if camera.daylight_sky.z==0.0 || v.snow_control.x==0.0 {return 0.0;}
    let kind=u32(max(v.surface.x,0.0));
    // Windows, water, lamps, architectural glazing, signs and virtual rooms stay intact.
    if kind==4u||kind==8u||kind==9u||kind==14u||kind>=40u {return 0.0;}
    let slope=smoothstep(0.25,0.85,surface.normal.z);
    if slope==0.0 {return 0.0;}
    let pixel=vec2<i32>(floor((v.world.xy-snow_cache.xy)*snow_cache.z));
    var exposure=1.0;var cleared=0.0;
    if all(pixel>=vec2<i32>(0))&&all(pixel<vec2<i32>(i32(snow_cache.w))) {
        let height=textureLoad(snow_exposure,pixel,0).r;
        // Account for slope within a texel so curved GLBs do not reject their own snow in stripes.
        let tolerance=0.0018+0.5/snow_cache.z*(abs(surface.normal.x)+abs(surface.normal.y))/max(surface.normal.z,0.25);
        exposure=step(height-tolerance,v.world.z);
        // Only road surfaces consult clearing, and only at the matching deck height.
        if v.snow_control.y==0.0&&(kind==0u||kind==6u) {
            cleared=cleared_coverage(v.world);
        }
    }
    return slope*exposure*(1.0-cleared)*camera.daylight_sky.z;
}
