struct Settings {
    inverse:mat4x4<f32>, eye_radius:vec4<f32>, viewport:vec4<f32>, tuning:vec4<f32>, weather:vec4<f32>,
};
@group(0) @binding(0) var<uniform> settings:Settings;
@group(0) @binding(1) var depth:texture_depth_multisampled_2d;
@group(0) @binding(2) var scene:texture_2d<f32>;
@group(0) @binding(3) var occlusion:texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let points=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
    return vec4<f32>(points[index],0.0,1.0);
}
fn pixel(p:vec2<i32>)->vec2<i32> {
    return clamp(p,vec2<i32>(0),vec2<i32>(settings.viewport.xy)-vec2<i32>(1));
}
fn depth_at(p:vec2<i32>)->f32 {return textureLoad(depth,pixel(p),0);}
fn position(p:vec2<i32>)->vec3<f32> {
    let q=pixel(p);
    let uv=(vec2<f32>(q)+0.5)/settings.viewport.xy;
    let h=settings.inverse*vec4<f32>(uv.x*2.0-1.0,1.0-uv.y*2.0,depth_at(q),1.0);
    return h.xyz/h.w;
}
@fragment fn ao(@builtin(position) screen:vec4<f32>)->@location(0) vec2<f32> {
    let q=pixel(vec2<i32>(screen.xy)*2+vec2<i32>(1));
    if depth_at(q)>=0.999999 || settings.tuning.z<0.5 {return vec2<f32>(1.0,60000.0);}
    let p=position(q);
    let right=position(q+vec2<i32>(1,0))-p;
    let left=p-position(q-vec2<i32>(1,0));
    let down=position(q+vec2<i32>(0,1))-p;
    let up=p-position(q-vec2<i32>(0,1));
    // Prefer the smaller derivative at silhouettes instead of spanning the depth jump.
    let dx=select(right,left,dot(left,left)<dot(right,right));
    let dy=select(down,up,dot(up,up)<dot(down,down));
    let raw=cross(dx,dy);let magnitude=length(raw);
    let eye=settings.eye_radius.xyz;
    let distance=length(eye-p);
    if magnitude<1e-12 {return vec2<f32>(1.0,distance);}
    let facing=raw/magnitude;
    let n=select(-facing,facing,dot(facing,eye-p)>0.0);
    let radius=settings.eye_radius.w;
    let pixels=clamp(radius/max(max(length(dx),length(dy)),0.000001),2.0,48.0);
    let directions=array<vec2<f32>,8>(vec2<f32>(1.0,0.0),vec2<f32>(0.707,0.707),
        vec2<f32>(0.0,1.0),vec2<f32>(-0.707,0.707),vec2<f32>(-1.0,0.0),
        vec2<f32>(-0.707,-0.707),vec2<f32>(0.0,-1.0),vec2<f32>(0.707,-0.707));
    var blocked=0.0;
    for(var i=0u;i<8u;i++) {for(var ring=0u;ring<2u;ring++) {
        let step=pixels*select(0.28,0.78,ring==1u);
        let sample_pixel=pixel(q+vec2<i32>(round(directions[i]*step)));
        if depth_at(sample_pixel)>=0.999999 {continue;}
        let delta=position(sample_pixel)-p;let travel=length(delta);
        if travel<0.000001 || travel>radius {continue;}
        let horizon=max(dot(n,delta)-settings.tuning.x,0.0)/travel;
        blocked+=horizon*(1.0-smoothstep(radius*0.25,radius,travel));
    }}
    return vec2<f32>(1.0-clamp(blocked/16.0*settings.tuning.y,0.0,0.48),distance);
}
fn winter_atmosphere(color:vec4<f32>,distance:f32,has_surface:bool)->vec4<f32> {
    if settings.weather.y==0.0 {return color;}
    let daylight=clamp(settings.weather.z,0.0,1.0);
    let fog_color=mix(vec3<f32>(0.018,0.026,0.045),vec3<f32>(0.52,0.60,0.66),daylight);
    // Map units are 100 metres. Distant surfaces approach the same haze as the sky.
    let visibility = exp(-max(distance - 0.15, 0.0) * 0.58);
    let haze = 0.76 * select(1.0, 1.0 - visibility, has_surface);
    let result=mix(color.rgb,fog_color,haze*settings.weather.y);
    return vec4<f32>(result,color.a);
}
fn scene_color(screen:vec4<f32>)->vec4<f32> {
    let p=vec2<i32>(screen.xy);
    var color=textureLoad(scene,p,0);
    let receives_haze=color.a>0.0;
    color=vec4<f32>(color.rgb,1.0);
    let strength=settings.weather.w;
    if strength>0.0 {
        let radial=screen.xy/settings.viewport.xy*2.0-1.0;
        let edge=smoothstep(0.12,1.30,dot(radial,radial));
        let shift=vec2<i32>(round(radial*edge*settings.viewport.y*0.018*strength));
        let split=vec3<f32>(textureLoad(scene,pixel(p+shift),0).r,
            color.g,textureLoad(scene,pixel(p-shift),0).b);
        let light=dot(split,vec3<f32>(0.2126,0.7152,0.0722));
        // Violet shadows and amber highlights, with the strongest fringing at the rim.
        let grade=max(vec3<f32>(0.0),mix(vec3<f32>(light),split,1.65))
            *vec3<f32>(1.22,0.86,1.08)+vec3<f32>(0.040,0.002,0.085)*(1.0-light);
        color=vec4<f32>(mix(color.rgb,grade*(1.0-0.94*edge),strength),color.a);
    }
    let daylight_grade = settings.viewport.z * settings.weather.z;
    if settings.tuning.z<0.5 && settings.weather.y==0.0 && daylight_grade==0.0 {return color;}
    let has_surface=depth_at(p)<0.999999;
    var distance=10.0;
    if has_surface {distance=length(settings.eye_radius.xyz-position(p));}
    var result=color;
    if has_surface && settings.tuning.z>=0.5 {
        let size=vec2<i32>(textureDimensions(occlusion));
        let q=p/2;var total=0.0;var weight=0.0;
        let tolerance=settings.eye_radius.w*0.22;
        for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {
            let uv=clamp(q+vec2<i32>(x,y),vec2<i32>(0),size-vec2<i32>(1));
            let sample=textureLoad(occlusion,uv,0).rg;
            let difference=(sample.y-distance)/tolerance;
            let spatial=1.0/(1.0+f32(x*x+y*y));
            let w=exp(-difference*difference)*spatial;
            total+=sample.x*w;weight+=w;
        }}
        let factor=select(1.0,total/max(weight,0.000001),weight>0.000001);
        result=vec4<f32>(color.rgb*factor,color.a);
    }
    let haze_distance = distance * settings.viewport.w;
    if receives_haze && has_surface && settings.weather.y == 0.0 && daylight_grade > 0.0 {
        // Keep the first 80 metres clear and soften only the more distant blocks.
        let air = 0.10 * daylight_grade * smoothstep(0.80, 4.75, haze_distance);
        result = vec4<f32>(mix(result.rgb, vec3<f32>(0.43, 0.47, 0.53), air), result.a);
    }
    if receives_haze {result = winter_atmosphere(result, haze_distance, has_surface);}
    if daylight_grade > 0.0 {
        let luma = dot(result.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let midtones = smoothstep(0.012, 0.08, luma) * (1.0 - smoothstep(0.60, 1.0, luma));
        let gain = mix(0.74, 1.12, smoothstep(0.09, 0.45, luma));
        let blue = smoothstep(0.05, 0.48, result.b - max(result.g, result.r));
        let chroma = mix(1.0, 0.60, blue);
        let graded = mix(vec3<f32>(luma), result.rgb, chroma) * mix(1.0, gain, midtones);
        let weight = daylight_grade * mix(1.0, 0.35, settings.weather.y);
        result = vec4<f32>(mix(result.rgb, graded, weight), result.a);
    }
    return result;
}

fn zombie_grade(color: vec3<f32>, screen: vec2<f32>) -> vec3<f32> {
    let light = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let highlights = smoothstep(0.12, 0.75, light);
    // Bleach midtones but preserve reds. Cool shadows give way to pale amber in sunlight.
    let red = smoothstep(0.03, 0.3, color.r - max(color.g, color.b));
    let tint = mix(vec3<f32>(0.67, 0.91, 0.95), vec3<f32>(1.08, 1.02, 0.79), highlights);
    let graded = mix(vec3<f32>(light), color, 0.48 + 0.37 * red) * tint;

    let frame = screen / settings.viewport.xy * 2.0 - 1.0;
    let rim = smoothstep(0.16, 1.72, dot(frame, frame));
    let vignette = mix(vec3<f32>(1.0), vec3<f32>(0.45, 0.27, 0.30), rim);
    // Update the grain at film cadence; relative exposure keeps dark streets readable.
    let seed = floor(screen) + floor(settings.weather.x * 24.0) * vec2<f32>(17.0, 29.0);
    let grain = fract(52.9829189 * fract(dot(seed, vec2<f32>(0.06711056, 0.00583715)))) - 0.5;
    return graded * vignette * (1.0 + grain * mix(0.085, 0.035, highlights));
}

@fragment fn composite(@builtin(position) screen: vec4<f32>) -> @location(0) vec4<f32> {
    let color = scene_color(screen);
    if settings.tuning.w < 0.5 { return color; }
    return vec4<f32>(zombie_grade(color.rgb, screen.xy), color.a);
}
