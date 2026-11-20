// One simplex corner contributes height and its analytic XY derivative.
// The integer hash selects a gradient independently at each world-space corner.
fn river_corner(cell: vec2<f32>, d: vec2<f32>) -> vec3<f32> {
    let angle = hash(cell) * 6.2831853;
    let g = vec2(cos(angle), sin(angle));
    let w = max(0.5 - dot(d, d), 0.0);
    let w3 = w*w*w;
    let h = dot(g, d);
    return 70.0 * w3 * vec3(w*h, w*g - 8.0*h*d);
}

fn river_noise(p: vec2<f32>) -> vec3<f32> {
    let cell = floor(p + vec2((p.x + p.y) * 0.3660254));
    let a = p - cell + vec2((cell.x + cell.y) * 0.21132487);
    let corner = select(vec2(0.0, 1.0), vec2(1.0, 0.0), a.x > a.y);
    return river_corner(cell, a)
        + river_corner(cell + corner, a - corner + vec2(0.21132487))
        + river_corner(cell + vec2(1.0), a - vec2(0.57735027));
}

// Each octave has its own drift, orientation and crest length. Project its
// gradient back into world space and attenuate unresolved detail by pixel size.
fn river_octave(p: vec2<f32>, axis: vec2<f32>, stretch: f32, frequency: f32,
                drift: vec2<f32>, strength: f32, footprint: f32) -> vec3<f32> {
    let weight = 1.0 - smoothstep(0.18, 0.85, footprint * frequency);
    if weight <= 0.0 { return vec3(0.0); }
    let along = vec2(-axis.y, axis.x) * stretch;
    let uv = vec2(dot(p, axis), dot(p, along)) * frequency + drift;
    let sampleWave = river_noise(uv);
    return weight * vec3(strength * (axis * sampleWave.y + along * sampleWave.z), sampleWave.x);
}

// The leading face of a raised, steep wind wave injects air in a short strip.
fn breaker_front(wave: vec3<f32>, swell: f32) -> f32 {
    let steep = -dot(wave.xy, vec2(0.28, 0.96));
    return smoothstep(0.08, 0.21, steep) * smoothstep(0.12, 0.49, wave.z + 0.22*swell);
}

// Sample the 128-square rank mask stored in the alpha of atlas layer 10.
// Texel-center addressing keeps the neighboring unused columns out of the filter.
fn threshold_rank(uv: vec2<f32>) -> f32 {
    let texel = fract(uv)*127.0 + vec2(0.5);
    return textureSampleLevel(commerce_signs, commerce_sampler, texel/vec2(512.0,128.0), 10, 0.0).a;
}

fn river_surface(p: vec2<f32>, t: f32, footprint: f32) -> vec3<f32> {
    // A slow current bends the shorter crests and varies capillary activity.
    let current = river_noise(p * 0.041 + vec2(-0.017, 0.009)*t);
    let bend = vec2(3.4, -2.1);
    let q = p + bend * current.x;
    let wind = 0.72 + 0.45*current.x;
    let swell = river_octave(p, vec2(0.60, 0.80), 0.52, 0.091,
        vec2(-0.077, 0.012)*t, 0.053, footprint);
    let windWave = river_octave(q, vec2(0.28, 0.96), 0.31, 0.37,
        vec2(-0.24, 0.047)*t + vec2(37.0, 9.0), 0.069, footprint);
    let chop = windWave + river_octave(q, vec2(-0.76, 0.65), 0.57, 0.83,
        vec2(-0.34, -0.11)*t + vec2(-13.0, 41.0), 0.045, footprint);
    let ripples = river_octave(q, vec2(0.48, 0.88), 0.28, 2.7,
        vec2(-0.73, 0.17)*t + vec2(81.0, -7.0), 0.041, footprint)
        + river_octave(q, vec2(-0.32, 0.95), 0.48, 7.9,
        vec2(-1.13, -0.31)*t + vec2(12.0, 62.0), 0.027, footprint);
    let detail = chop.xy + wind * ripples.xy;
    let front = breaker_front(windWave, swell.z);
    // Wind chop and swell supply the time-varying saturation. A wider crest
    // shoulder leaves room for the individual clumps to form and dissipate.
    let shoulder = smoothstep(0.08, 0.96, 0.58*chop.z + 0.40*swell.z);
    let saturation = clamp(0.54*shoulder + 0.24*front, 0.0, 0.76);
    let drift = p - vec2(0.055, 0.16)*t;
    let uv = drift*0.26 + vec2(current.x, swell.z)*0.15;
    // Periodic Gaussian-filtered white noise, ranked to a uniform threshold
    // distribution. Coarse and fine densities gate different-sized clumps.
    let cluster = threshold_rank(uv);
    let grain = threshold_rank(vec2(-uv.y,uv.x)*4.73 + vec2(0.37,0.81));
    let clusterMask = smoothstep(cluster-0.055, cluster+0.055, saturation);
    let filling = clamp(0.28 + 1.75*(saturation-cluster), 0.0, 0.95);
    let bubbles = mix(smoothstep(grain-0.05,grain+0.05,filling), filling,
        smoothstep(0.009,0.055,footprint));
    let coverage = 0.84*mix(clusterMask*bubbles, saturation*(0.28+0.78*saturation),
        smoothstep(0.055,0.28,footprint));
    return vec3(swell.xy + detail + 0.041*current.yz*dot(bend, detail), coverage);
}

// The bounds reject open water before walking a nearby convex waterline.
fn inside_waterline(p: vec2<f32>) -> bool {
    for (var i = 0u; i < 8u; i++) {
        let bounds = light_grid.hulls[i].bounds;
        if bounds.x == bounds.z { break; }
        if any(p < bounds.xy) || any(p > bounds.zw) { continue; }
        var previous = light_grid.hulls[i].pairs[15].zw;
        var inside = true;
        for (var j = 0u; j < 16u; j++) {
            let pair = light_grid.hulls[i].pairs[j];
            let a = pair.xy - previous;
            let b = pair.zw - pair.xy;
            if dot(vec2(-a.y,a.x), p-previous) < 0.0 || dot(vec2(-b.y,b.x), p-pair.xy) < 0.0 {
                inside = false;
                break;
            }
            previous = pair.zw;
        }
        if inside { return true; }
    }
    return false;
}

fn shade_river(v: Output, surface: SurfaceData, shore: CoastalWater) -> vec4<f32> {
    let p = v.world.xy * 100.0;
    let t = camera.eye_time.w;
    let footprint = 100.0 * max(length(surface.world_dx.xy), length(surface.world_dy.xy));
    let water = river_surface(p, t, footprint);
    var wake = 0.0;
    for (var i=0u; i<32u; i++) {
        let packet=light_grid.wakes[i];
        if packet.shape.w == 0.0 {break;}
        let delta = p - packet.origin_direction.xy;
        let reach = packet.shape.z;
        if dot(delta,delta) > (reach+1.5)*(reach+1.5) {continue;}
        let forward = packet.origin_direction.zw;
        let along = -dot(delta,forward);
        let lateral = abs(dot(delta,vec2(-forward.y,forward.x)));
        let size = abs(packet.shape.x);
        let spread = packet.shape.y;
        let ridge=abs(lateral-(size*0.32+along*spread));
        let coverage=(1.0-smoothstep(0.04,0.08+0.15*size,ridge))
            *smoothstep(-0.24,0.02,along)*(1.0-smoothstep(0.15,reach,along));
        wake=max(wake,coverage*min(1.0,2.0*size)*packet.shape.w);
    }
    let slope = water.xy*(1.0+0.5*wake) + shore.crest_slope;
    let n = vec3(-slope, 1.0) * inverseSqrt(1.0 + dot(slope, slope));
    let eye = camera.eye_time.xyz - v.world;
    let distance = length(eye);
    let view = eye / max(distance, 0.00001);
    let reflected = reflect(-view, n);
    let facing = clamp(dot(n, view), 0.0, 1.0);
    let fresnel = 0.021 + 0.979 * pow(1.0 - facing, 5.0);
    let rough = smoothstep(0.05, 1.2, footprint);
    let environment = textureSampleLevel(filtered_environment, sky_sampler,
        vec3(reflected.xy, abs(reflected.z)), 0.8 + 1.8*rough).rgb;
    let daylight = clamp(camera.daylight_sky.x, 0.0, 1.0);
    let scatter = v.color * vec3(0.72, 0.70, 0.53) * (1.0 + 0.7*slope.y);
    let shadow = visibility(v.world, vec3(0.0, 0.0, 1.0));
    let local = local_irradiance(v.world, n);
    let body = mix(scatter,shore.bed,shore.transmission) * (mix(0.05, 0.48 + 0.52*shadow, daylight) + local);
    let sun = camera.sun_exposure.xyz;
    let exponent = mix(340.0, 88.0, rough);
    let glint = pow(max(dot(reflected, sun), 0.0), exponent) * (exponent / 340.0);
    let sunlight = camera.sun_color.rgb * camera.sun_color.w * daylight * shadow;
    let lit = mix(body, environment, fresnel) + sunlight * glint * (0.14 + fresnel);
    let foamLight = vec3(0.87, 0.93, 0.94) * (mix(vec3(0.024, 0.032, 0.048),
        vec3(0.58, 0.65, 0.74), daylight) + sunlight*(0.22 + 0.45*max(sun.z, 0.0)) + local);
    let foamed = mix(lit, foamLight, max(max(shore.foam,water.z*(1.0-shore.transmission)),wake*(0.32+0.45*threshold_rank(p*0.45))));
    let fog = (1.0 - exp(-distance*0.025)*(1.0 - smoothstep(5.8, 7.6, distance))) * (1.0 - camera.daylight_sky.y);
    let sky = textureSampleLevel(sky_environment, sky_sampler, -view, 0.0).rgb;
    return vec4(filmic_display(mix(foamed, sky, fog) * camera.sun_exposure.w), 1.0);
}
