fn shadow_tile_visibility(clip: vec4<f32>, origin: vec2<f32>, size: f32, bias: f32) -> f32 {
    if clip.w <= 0.0 { return 1.0; }
    let p = clip.xyz / clip.w;
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    if !(all(uv > vec2(0.0)) && all(uv < vec2(1.0)) && p.z > 0.0 && p.z < 1.0) {
        return 1.0;
    }
    let pixel = uv * size;
    var value = 0.0;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            // Keep filtering inside this tile, including along the atlas seam.
            let tap = clamp(pixel + vec2(f32(x), f32(y)), vec2(0.5), vec2(size - 0.5));
            value += textureSampleCompareLevel(shadow_map, shadow_sampler,
                (origin + tap) * camera.shadow_atlas.zw, p.z - bias);
        }
    }
    return value / 9.0;
}

fn visibility(world: vec3<f32>, n: vec3<f32>) -> f32 {
    let coarse_clip = camera.light_view_projection * vec4(world + n * 0.00025, 1.0);
    let coarse = shadow_tile_visibility(coarse_clip, vec2(0.0), camera.shadow_atlas.x, 0.0001);
    if coarse <= 0.0 { return coarse; }
    let player_clip = camera.player_light_view_projection * vec4(world + n * 0.00004, 1.0);
    let player_origin = vec2(camera.shadow_atlas.x, 0.5 * (camera.shadow_atlas.x - camera.shadow_atlas.y));
    let player = shadow_tile_visibility(player_clip, player_origin, camera.shadow_atlas.y, 0.000005);
    return min(coarse, player);
}
