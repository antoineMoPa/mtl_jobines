// Appended to the actual world shader for native smoke validation only.
@vertex fn probe_vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=vec2(f32((index<<1u)&2u),f32(index&2u))*2.0-1.0;
    return vec4(p,0.0,1.0);
}
@fragment fn probe_fragment(@builtin(position) pixel:vec4<f32>)->@location(0) vec4<f32> {
    let pair=u32(pixel.x)/2u;
    let normals=array<vec3<f32>,8>(vec3(0.0,-1.0,0.0),vec3(0.0,1.0,0.0),
        vec3(-1.0,0.0,0.0),vec3(1.0,0.0,0.0),vec3(0.0,-1.0,0.0),vec3(0.0,1.0,0.0),
        vec3(0.0,-1.0,0.0),vec3(0.0,-1.0,0.0));
    let views=array<vec3<f32>,8>(vec3(0.0,-0.8,-0.6),vec3(0.0,0.8,-0.6),
        vec3(-0.8,0.0,-0.6),vec3(0.8,0.0,-0.6),vec3(0.8,-0.36,-0.48),vec3(0.8,0.36,-0.48),
        vec3(0.0,-0.1,-0.9949874),vec3(0.0,-0.99995,-0.01));
    var n=normals[min(pair,7u)];var view=views[min(pair,7u)];
    if pair==8u {
        // Select a real occupied stellar cell at ~30° altitude. Random rays
        // almost always miss the intentionally tiny native-resolution cores.
        var cell=vec2(90.0,120.0);
        for(var x=0u;x<360u;x++) {
            let candidate=vec2(f32(x),120.0);
            if sky_star_hash(candidate)>1.0-camera.star_params.x {cell=candidate;break;}
        }
        let offset=vec2(sky_star_hash(cell+vec2(31.0,73.0)),
            sky_star_hash(cell+vec2(97.0,17.0)));
        let center=cell+mix(vec2(0.4),vec2(0.6),offset);
        let longitude=(center.x-180.0)*0.0174532925;
        let latitude=(center.y-90.0)*0.0174532925;
        let star=vec3(cos(latitude)*cos(longitude),cos(latitude)*sin(longitude),sin(latitude));
        view=-reflect(star,n);
    }
    let ray=reflect(-view,n);
    let stellar=sky_stars(ray,camera.star_params,camera.star_color);
    var color=textureSampleLevel(sky_environment,sky_sampler,ray,0.0).rgb+stellar;
    if u32(pixel.x)%2u==1u {
        // Remove the specified Fresnel weight to compare the reflected sky's
        // radiance directly with the same world direction in the environment.
        let daylight=clamp(camera.daylight_sky.x,0.0,1.0);
        let weight=glass_reflectance(n,view,daylight);
        color=room_glass(vec3(0.0),n,view,daylight)/weight;
    }
    color*=camera.sun_exposure.w;
    var proof=1.0;
    if pair==8u && camera.star_color.w>0.99 && camera.star_params.x>=0.01
        && camera.star_params.z>0.0 && dot(camera.star_color.rgb,vec3(1.0))>0.1 {
        proof=min(1.0,max(stellar.r,max(stellar.g,stellar.b))/camera.star_params.z);
    }
    return vec4(filmic_display(color),proof);
}
