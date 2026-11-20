// Terrain and water triangles carry the same interpolated bed elevation.
// Positive heights are inland. All distances below are in metres.
struct CoastalWater {
    bed: vec3<f32>,
    transmission: f32,
    crest_slope: vec2<f32>,
    foam: f32,
};

fn shade_shore(v: Output, front: bool, coverage: f32, surface: SurfaceData) -> vec4<f32> {
    let p = v.world.xy * 100.0;
    let height = v.fence_uv.x * 100.0;
    let dx = surface.world_dx.xy * 100.0;
    let dy = surface.world_dy.xy * 100.0;
    let footprint = max(length(dx), length(dy));
    let detail = 1.0 - smoothstep(0.1, 0.8, footprint);
    let coast = noise(p * 0.035);
    let sediment_noise = noise(p * 0.34);
    let grain = mix(0.5, noise(p * 9.0), 1.0 - smoothstep(0.03, 0.16, footprint));
    let edge = 0.34*(coast-0.5) + 0.16*(sediment_noise-0.5) + 0.06*(grain-0.5);

    // Coherent stretches of lawn reach the swash between exposed beaches.
    let beach_end = mix(-0.46, 0.90, smoothstep(0.30, 0.66, coast));
    let grass = smoothstep(beach_end, beach_end + 0.44, height + edge);
    let stones = smoothstep(0.40, 0.73, noise(p * 0.095 + vec2(21.0, 8.0)));
    // The same field shapes each exposed stone and tilts its lighting normal.
    let pebble = river_noise(p * 3.4);
    let gravel = 0.5 + 0.5*pebble.x*detail;
    let exposed = stones * smoothstep(0.32, 0.62, gravel + 0.15*(grain-0.5));
    let sand = vec3(0.57,0.44,0.28) * (0.68 + 0.21*sediment_noise + 0.40*grain);
    let rock = vec3(0.24,0.26,0.25) * (0.57 + 0.71*gravel + 0.27*grain);
    var bed = mix(sand, rock, exposed);
    if grass > 0.0 {
        let blades = grass_sample(p/0.85, dx/0.85, dy/0.85);
        bed = mix(bed, summer_vegetation_color(blades), grass);
    }

    let phase = camera.eye_time.w * 1.18 + 3.0*noise(p*0.061) + 0.28*sediment_noise;
    let runup = 0.22*sin(phase) + 0.05*sin(2.0*phase-0.6);
    let depth = runup - height - edge*0.35;
    let film = smoothstep(-0.025,0.045,depth);
    // Earlier positions of the same swash leave a fading wet band on retreat.
    var wet = film;
    for (var i = 1; i <= 5; i++) {
        let age = f32(i)*0.66;
        let past = phase-age;
        let reach = 0.22*sin(past)+0.05*sin(2.0*past-0.6);
        wet = max(wet, exp(-0.31*age)*smoothstep(-0.11,0.04,reach-height-edge*0.35));
    }
    bed *= 1.0 - wet*mix(0.43,0.19,grass);

    var bed_surface = surface;
    let relief = (1.0-grass)*exposed*detail*(1.0-surface.snow);
    let beveled = surface.normal - vec3(pebble.yz * (0.07*relief),0.0);
    bed_surface.normal = beveled * inverseSqrt(max(dot(beveled,beveled),0.00001));

    if film > 0.0 {
        let angle = height*8.2 - phase + edge;
        let crest = 0.5 + 0.5*cos(angle);
        let zone = (1.0-smoothstep(0.4,1.65,depth))*(1.0-smoothstep(0.35,2.0,footprint));
        let slope = -0.17*sin(angle)*crest*crest*zone;
        let lip = 1.0-smoothstep(0.015,0.11,depth);
        // Whitewater concentrates on the advancing front and breaks into pockets on retreat.
        let advancing = smoothstep(-0.30,0.30,0.22*cos(phase)+0.10*cos(2.0*phase-0.6));
        let lace = smoothstep(0.32,0.76,threshold_rank(p*0.45 + vec2(0.015,-0.022)*camera.eye_time.w));
        let foam = zone * max(lip*mix(0.27,0.88,advancing),pow(crest,15.0)*0.76)
            * mix(0.29,lace,detail) * mix(0.40,1.0,sediment_noise);
        // Taper the last trace of the bed before rejoining the deep-water path.
        let transmission = exp(-1.55*max(depth,0.0))*(1.0-smoothstep(2.4,3.6,-height));
        let uphill = vec2(cos(v.surface.z),sin(v.surface.z));
        var river = v;
        river.color = vec3(0.025,0.15,0.20);
        let water = shade_river(river,surface,CoastalWater(bed,transmission,uphill*slope,foam));
        if film >= 1.0 { return water; }
        return mix(shore_ground(v,front,coverage,bed_surface,bed,grass,wet,exposed),water,film);
    }
    let ground = shore_ground(v,front,coverage,bed_surface,bed,grass,wet,exposed);
    if height <= 1.0 { return ground; }
    return mix(ground,shade_grass(v,front,coverage,surface),smoothstep(1.0,1.7,height));
}

fn shore_ground(v: Output, front: bool, coverage: f32, surface: SurfaceData,
                bed: vec3<f32>, grass: f32, wet: f32, stone: f32) -> vec4<f32> {
    var ground = v;
    ground.color = bed;
    ground.surface = vec4(7.0,0.0,0.0,0.0);
    var finish = surface;
    finish.snow *= 1.0-wet;
    finish.finish = vec3(mix(0.91,mix(0.36,0.54,grass),wet),0.0,wet*mix(0.20+0.27*stone,0.13,grass));
    return shade_surface_lit(ground,front,coverage,finish,local_irradiance(v.world,finish.normal));
}
