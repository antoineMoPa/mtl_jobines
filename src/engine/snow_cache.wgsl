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
