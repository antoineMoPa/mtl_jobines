struct Camera {
    view_projection: mat4x4<f32>, light_view_projection: mat4x4<f32>,
    eye_time: vec4<f32>, sun_exposure: vec4<f32>, daylight_sky: vec4<f32>,
    inverse_view_projection: mat4x4<f32>, sun_color: vec4<f32>,
    star_params: vec4<f32>, star_color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var sky: texture_cube<f32>;
@group(1) @binding(1) var sky_sampler: sampler;
struct Ray { @builtin(position) clip: vec4<f32>, @location(0) ray: vec3<f32> };
@vertex fn vs(@builtin(vertex_index) vertex: u32) -> Ray {
    let p = vec2(f32((vertex << 1u) & 2u), f32(vertex & 2u)) * 2.0 - 1.0;
    let world = camera.inverse_view_projection * vec4(p, 1.0, 1.0);
    return Ray(vec4(p, 1.0, 1.0), world.xyz / world.w - camera.eye_time.xyz);
}
@fragment fn fs(v: Ray) -> @location(0) vec4<f32> {
    let radiance = textureSampleLevel(sky, sky_sampler, v.ray, 0.0).rgb
        + sky_stars(v.ray, camera.star_params, camera.star_color);
    let color = radiance * camera.sun_exposure.w;
    return vec4(color / (color + vec3(1.0)), 1.0);
}
