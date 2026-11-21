// Ground coordinates are in metres (100 metres per map unit).
@group(3) @binding(12) var grass_image:texture_2d<f32>;
@group(3) @binding(13) var grass_sampler:sampler;

fn grass_rotation(seed:f32)->mat2x2<f32> {
    let angle=seed*6.2831853;
    let c=cos(angle);let s=sin(angle);
    return mat2x2<f32>(vec2<f32>(c,s),vec2<f32>(-s,c));
}

// Every lattice vertex owns a different crop and orientation. Crops stay inside
// the photograph: opposite image edges never have to be seamless.
fn grass_patch(p:vec2<f32>,vertex:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec3<f32> {
    let rotation=grass_rotation(hash(vertex+vec2<f32>(173.0,59.0)));
    let center=vec2<f32>(hash(vertex+vec2<f32>(31.0,97.0)),hash(vertex+vec2<f32>(71.0,13.0)))*0.24+0.38;
    let uv=center+rotation*(p-vertex)*0.20;
    return textureSampleGrad(grass_image,grass_sampler,uv,rotation*dx*0.20,rotation*dy*0.20).rgb;
}

fn grass_sample(p:vec2<f32>,dx:vec2<f32>,dy:vec2<f32>)->vec3<f32> {
    let cell=floor(p);let f=fract(p);
    var a=cell;var b=cell+vec2<f32>(1.0,0.0);var c=cell+vec2<f32>(0.0,1.0);
    var weight=vec3<f32>(1.0-f.x-f.y,f.x,f.y);
    if f.x+f.y>1.0 {
        a=cell+vec2<f32>(1.0);b=cell+vec2<f32>(0.0,1.0);c=cell+vec2<f32>(1.0,0.0);
        weight=vec3<f32>(f.x+f.y-1.0,1.0-f.x,1.0-f.y);
    }
    // Smooth weights suppress visible triangle boundaries without moving UVs.
    weight=weight*weight*weight;
    weight/=dot(weight,vec3<f32>(1.0));
    return grass_patch(p,a,dx,dy)*weight.x+grass_patch(p,b,dx,dy)*weight.y+grass_patch(p,c,dx,dy)*weight.z;
}

fn shade_grass(v:Output,front:bool,coverage:f32,surface:SurfaceData)->vec4<f32> {
    let metres=v.world.xy*100.0;
    let dx=surface.world_dx.xy*100.0;let dy=surface.world_dy.xy*100.0;
    let footprint=max(length(dx),length(dy));
    // Irrationally related scales and offsets prevent a common visible period.
    let large=noise(metres/31.7+vec2<f32>(37.0,11.0));
    let medium=noise(metres/6.3+vec2<f32>(-53.0,19.0));
    let small=mix(0.5,noise(metres/1.13),1.0-smoothstep(0.3,1.5,footprint));
    let dry=smoothstep(0.48,0.83,large*0.55+medium*0.30+small*0.15);
    let p=metres/0.85;
    let color=grass_sample(p,dx/0.85,dy/0.85);
    var material=v;
    material.color=color*mix(vec3<f32>(0.90,1.04,0.88),vec3<f32>(1.24,1.06,0.77),dry)
        *(0.80+0.24*large+0.18*medium+0.08*small);
    material.color=summer_vegetation_color(material.color);
    material.color=road_band_edge(material.color,v.world.xy,surface.world_footprint.xy,v.fence_uv,v.surface.w,true);
    // Use the generic physical surface after evaluating grass, bypassing the old
    // procedural green tint. The original grass snow mask is retained.
    material.surface=vec4<f32>(7.0,0.0,0.0,0.0);
    var finish=surface;
    finish.finish=vec3<f32>(mix(0.92,0.99,dry),0.0,0.0);
    // A restrained photo-derived bump is an approximation, not measured height.
    // Sample the same stochastic field so detail remains aligned across patches.
    let epsilon=0.006;
    let luminance=vec3<f32>(0.2126,0.7152,0.0722);
    let h=dot(color,luminance);
    let hx=dot(grass_sample(p+vec2<f32>(epsilon,0.0),dx/0.85,dy/0.85),luminance);
    let hy=dot(grass_sample(p+vec2<f32>(0.0,epsilon),dx/0.85,dy/0.85),luminance);
    let strength=(1.0-smoothstep(0.015,0.12,footprint))*(1.0-surface.snow)*0.035;
    let slope=clamp(vec2<f32>(hx-h,hy-h)/epsilon,vec2<f32>(-4.0),vec2<f32>(4.0))*strength;
    let perturbation=vec3<f32>(-slope,0.0);
    let n=surface.normal+perturbation-surface.normal*dot(perturbation,surface.normal);
    finish.normal=n/max(length(n),0.00001);
    return shade_surface_lit(material,front,coverage,finish,local_irradiance(v.world,finish.normal));
}
