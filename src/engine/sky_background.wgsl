struct Camera {
    view_projection: mat4x4<f32>, light_view_projection: mat4x4<f32>,
    eye_time: vec4<f32>, sun_exposure: vec4<f32>, daylight_sky: vec4<f32>,
    inverse_view_projection: mat4x4<f32>, sun_color: vec4<f32>,
    star_params: vec4<f32>, star_color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var sky: texture_cube<f32>;
@group(1) @binding(1) var sky_sampler: sampler;
@group(2) @binding(0) var panorama: texture_2d<f32>;
@group(2) @binding(1) var panorama_sampler: sampler;
@group(2) @binding(2) var<uniform> custom_panorama: vec4<u32>;
struct SkydomeInput {
    direction: vec3<f32>,
    uv: vec2<f32>,
    panorama: vec4<f32>,
    sky: vec3<f32>,
    daylight: f32,
    time: f32,
};
struct Ray { @builtin(position) clip: vec4<f32>, @location(0) ray: vec3<f32> };
@vertex fn vs(@builtin(vertex_index) vertex: u32) -> Ray {
    let p = vec2(f32((vertex << 1u) & 2u), f32(vertex & 2u)) * 2.0 - 1.0;
    let world = camera.inverse_view_projection * vec4(p, 0.0, 1.0);
    return Ray(vec4(p, 0.0, 1.0), world.xyz / world.w - camera.eye_time.xyz);
}
@fragment fn fs(v: Ray) -> @location(0) vec4<f32> {
    let ray = v.ray / max(length(v.ray), 0.000001);
    let atmosphere = textureSampleLevel(sky, sky_sampler, ray, 0.0).rgb;
    let elevation = asin(clamp(ray.z, -1.0, 1.0));
    // Z-up hemisphere: bottom of each source photograph is the horizon.
    let uv = vec2(atan2(v.ray.y, v.ray.x) / 6.2831853 + 0.5,
        1.0 - clamp(elevation / 1.5707963, 0.0, 1.0));
    let texel = textureSampleLevel(panorama, panorama_sampler, uv, 0.0);
    let daylight = clamp(camera.daylight_sky.x, 0.0, 1.0);
    // Fade the source's landscape strip and converge cleanly at the dome pole.
    let coverage = select(mix(texel.g, texel.r, daylight), 0.0, custom_panorama.x != 0u)
        * smoothstep(0.015, 0.08, elevation)
        * (1.0 - smoothstep(1.25, 1.55, elevation));
    let sunward = pow(max(dot(ray, camera.sun_exposure.xyz), 0.0), 8.0);
    let sunlight = camera.sun_color.rgb * min(camera.sun_color.w, 4.0)
        * daylight * (0.45 + 0.55 * sunward);
    let moonlight = vec3(0.035, 0.045, 0.075) * (1.0 - daylight);
    let illumination = atmosphere * 0.18 + sunlight + moonlight;
    let stars = sky_stars(ray, camera.star_params, camera.star_color)
        * (1.0 - coverage * 0.9);
    let radiance = atmosphere + illumination * coverage + stars;
    let base = filmic_display(radiance * camera.sun_exposure.w);
    let color = owe_skydome(SkydomeInput(ray, uv, texel, base, daylight, camera.eye_time.w));
    return vec4(color, 1.0);
}
