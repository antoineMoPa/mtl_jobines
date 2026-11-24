struct Settings {
    matrix:mat4x4<f32>, eye_time:vec4<f32>, viewport:vec4<f32>, weather:vec4<f32>,
};
@group(0) @binding(0) var<uniform> settings:Settings;
@group(0) @binding(1) var exposure:texture_2d<f32>;
@group(0) @binding(2) var<uniform> cache:vec4<f32>;
@group(1) @binding(0) var depth:texture_depth_multisampled_2d;
struct Flake {
    @builtin(position) clip:vec4<f32>,
    @location(0) local:vec2<f32>,
    @location(1) @interpolate(flat) opacity:f32,
    @location(2) @interpolate(flat) haze:f32,
};
fn seed_for(key:vec3<i32>,layer:u32)->f32 {
    // Integer world keys keep signed cells stable across large-map travel.
    let p=bitcast<vec3<u32>>(key);
    var h=(p.x*73856093u)^(p.y*19349663u)^(p.z*83492791u)^(layer*2654435761u);
    h=(h^(h>>16u))*2246822519u;
    h=(h^(h>>13u))*3266489917u;
    return f32((h^(h>>16u))&16777215u)/16777216.0;
}
@vertex fn vs(@builtin(vertex_index) vertex:u32,@builtin(instance_index) instance:u32)->Flake {
    let corners=array<vec2<f32>,4>(vec2<f32>(-1.0,-1.0),vec2<f32>(1.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(-1.0,1.0));
    var out:Flake;
    out.clip=vec4<f32>(2.0,2.0,2.0,1.0);out.local=corners[vertex];out.opacity=0.0;out.haze=0.0;
    let layer=instance%3u;let id=instance/3u;
    let spacing=array<f32,3>(0.0125,0.0185,0.025)[layer];
    let time=settings.eye_time.w;let eye=settings.eye_time.xyz;
    let speed=(0.8+f32(layer)*0.25)*0.01;
    let advection=vec3<f32>(time*0.0022,0.0,-time*speed);
    let anchor=vec3<i32>(floor((eye-advection)/spacing))-vec3<i32>(12,12,4);
    let key=anchor+vec3<i32>(i32(id%24u),i32((id/24u)%24u),i32(id/(24u*24u)));
    let seed=seed_for(key,layer);
    if seed>settings.weather.y {return out;}
    // Camera coordinates select cells only; each shared key retains its world position.
    let jitter=vec3<f32>(fract(seed*19.17),fract(seed*37.71),fract(seed*53.31));
    let center=(vec3<f32>(key)+0.12+jitter*0.76)*spacing+advection+
        vec3<f32>(sin(time*0.62+seed*6.283)*0.004,cos(time*0.43+seed*11.0)*0.0035,0.0);
    let delta=center-eye;let distance=length(delta);
    // Leave a full padded cell on each axis before camera/advection rebasing.
    let fade=(1.0-smoothstep(spacing*8.8,spacing*10.4,length(delta.xy)))*
        (1.0-smoothstep(spacing*2.2,spacing*3.0,abs(delta.z)))*smoothstep(0.0055,0.011,distance);
    if fade<0.012 {return out;}
    let pixel=vec2<i32>(floor((center.xy-cache.xy)*cache.z));
    if all(pixel>=vec2<i32>(0))&&all(pixel<vec2<i32>(i32(cache.w))) {
        // Per-flake exposure allows snow outside a roof while the camera is under it.
        if center.z<textureLoad(exposure,pixel,0).r+0.0001 {return out;}
    }
    var clip=settings.matrix*vec4<f32>(center,1.0);
    if clip.w<=0.0008||clip.z<0.0||clip.z>clip.w {return out;}
    let radius=0.00018+fract(seed*29.13)*0.00030;
    let pixel_radius=clamp(radius*settings.viewport.z/clip.w,(0.85+fract(seed*5.71)*0.30)*settings.viewport.w,4.0*settings.viewport.w);
    clip=vec4<f32>(clip.xy+out.local*pixel_radius*2.0/settings.viewport.xy*clip.w,clip.zw);
    out.clip=clip;out.opacity=fade*0.92;out.haze=smoothstep(0.04,0.26,distance)*0.32;
    return out;
}
@fragment fn fs(flake:Flake)->@location(0) vec4<f32> {
    let d=length(flake.local);let aa=fwidth(d);
    let alpha=(1.0-smoothstep(0.55,1.0+aa,d))*flake.opacity;
    if alpha<0.012 {discard;}
    if flake.clip.z<textureLoad(depth,vec2<i32>(flake.clip.xy),0) {discard;}
    let daylight=clamp(settings.weather.x,0.0,1.0);
    let color=mix(vec3<f32>(0.18,0.23,0.31),vec3<f32>(0.98,0.99,1.0),daylight);
    let fog=mix(vec3<f32>(0.018,0.026,0.045),vec3<f32>(0.52,0.60,0.66),daylight);
    return vec4<f32>(mix(color,fog,flake.haze),alpha);
}
