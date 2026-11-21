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

fn grass_tint(metres:vec2<f32>,footprint:f32)->vec4<f32> {
    // Irrationally related scales and offsets prevent a common visible period.
    let large=noise(metres/31.7+vec2<f32>(37.0,11.0));
    let medium=noise(metres/6.3+vec2<f32>(-53.0,19.0));
    let small=mix(0.5,noise(metres/1.13),1.0-smoothstep(0.3,1.5,footprint));
    let dry=smoothstep(0.48,0.83,large*0.55+medium*0.30+small*0.15);
    let tint=mix(vec3<f32>(0.90,1.04,0.88),vec3<f32>(1.24,1.06,0.77),dry)
        *(0.80+0.24*large+0.18*medium+0.08*small);
    return vec4<f32>(tint,dry);
}

fn shade_grass(v:Output,front:bool,coverage:f32,surface:SurfaceData)->vec4<f32> {
    let metres=v.world.xy*100.0;
    let dx=surface.world_dx.xy*100.0;let dy=surface.world_dy.xy*100.0;
    let footprint=max(length(dx),length(dy));
    let tint=grass_tint(metres,footprint);
    let p=metres/0.85;
    let color=grass_sample(p,dx/0.85,dy/0.85);
    var material=v;
    material.color=summer_vegetation_color(color*tint.rgb);
    material.color=road_band_edge(material.color,v.world.xy,surface.world_footprint.xy,v.fence_uv,v.surface.w,true);
    // Use the generic physical surface after evaluating grass, bypassing the old
    // procedural green tint. The original grass snow mask is retained.
    material.surface=vec4<f32>(7.0,0.0,0.0,0.0);
    var finish=surface;
    finish.finish=vec3<f32>(mix(0.92,0.99,tint.w),0.0,0.0);
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

struct Ribbon {
    @builtin(position) clip:vec4<f32>,
    @location(0) root:vec3<f32>,
    @location(1) light_alpha:vec2<f32>,
    @location(2) slope:vec2<f32>,
};
fn ribbon_key(cell:vec2<i32>)->u32 {
    var n=bitcast<u32>(cell.x)*0x9e3779b9u ^ bitcast<u32>(cell.y)*0x85ebca6bu;
    n=(n^(n>>16u))*0x7feb352du;
    n=(n^(n>>15u))*0x846ca68bu;
    return n^(n>>16u);
}
@vertex fn vs_ribbon(@builtin(vertex_index) index:u32,@location(0) seed:vec4<f32>,
    @location(1) tile:vec4<f32>,@location(2) terrain:vec4<f32>,@location(3) viewport:vec4<f32>)->Ribbon {
    let key=ribbon_key(vec2<i32>(tile.xy));
    let shift=vec2<f32>(f32(key&65535u),f32(key>>16u))/65536.0;
    var coord=seed.xy-0.5;
    switch key&3u {
        case 1u:{coord=vec2<f32>(-coord.y,coord.x);}
        case 2u:{coord=-coord;}
        case 3u:{coord=vec2<f32>(coord.y,-coord.x);}
        default:{}
    }
    coord=fract(coord+0.5+shift);
    let quad=vec2<u32>(coord*2.0);
    let foot=(tile.xy+coord)*0.32;
    let root=vec3<f32>(foot*0.01,tile.z+dot((coord-0.5)*0.0032,terrain.xy));
    let distance=length(root-camera.eye_time.xyz);
    let fade=select(1.0-smoothstep(terrain.z*0.7,terrain.z,distance),1.0,index/24u<u32(terrain.w));
    if (u32(tile.w)&(1u<<(quad.x+quad.y*2u)))==0u || fade<0.001 {
        return Ribbon(vec4<f32>(2.0,2.0,2.0,1.0),root,vec2<f32>(0.0),terrain.xy);
    }
    let t=f32((index%8u)/2u)/3.0;
    let blade=f32((index%24u)/8u);
    let a=(seed.w+shift.x)*6.2831853+blade*2.0943951;
    let fan=vec2<f32>(cos(a),sin(a));
    let growth=sin(dot(foot,vec2<f32>(4.1,6.8))+0.8*sin(foot.y*3.9));
    let sweep=1.8+2.0*growth;
    let breeze=vec2<f32>(cos(sweep),sin(sweep));
    let bend=0.23*fan+breeze*(0.15+0.07*growth+0.13*sin(dot(foot,vec2<f32>(3.7,2.1))-camera.eye_time.w*1.9));
    let linear=0.035*fan;
    let h=0.0003*(0.45+0.52*seed.z)*(0.75+0.125*((blade+floor(seed.y*2.99))%3.0))*(1.0+0.075*growth);
    var clip=camera.view_projection*vec4<f32>(root+h*vec3<f32>(linear*t+bend*t*t,t),1.0);
    let tangent=camera.view_projection*vec4<f32>(linear+2.0*t*bend,1.0,0.0);
    let dir=(tangent.xy*clip.w-clip.xy*tangent.w)*viewport.xy;
    let across=vec2<f32>(-dir.y,dir.x)/max(length(dir),1e-9);
    // Perspective focal length for the game's 58-degree vertical field of view.
    let px=(0.0000021+0.0000015*seed.y)*(1.0-t*t)*0.902024*viewport.y/max(0.00005,clip.w);
    let side=select(-1.0,1.0,(index&1u)==1u);
    clip=vec4<f32>(clip.xy+across*(2.0/viewport.xy)*max(px,0.52)*side*clip.w,clip.zw);
    return Ribbon(clip,root,vec2<f32>(mix(0.76,1.42,t)*(1.0+0.055*growth),0.5*min(1.0,px/0.52)*fade),terrain.xy);
}
@fragment fn fs_ribbon(r:Ribbon)->@location(0) vec4<f32> {
    var v:Output;
    v.world=r.root;v.normal=vec3<f32>(-r.slope,1.0);
    v.surface=vec4<f32>(7.0,0.0,0.0,0.0);
    let metres=r.root.xy*100.0;
    let p=metres/0.85;
    let tint=grass_tint(metres,0.0005);
    v.color=summer_vegetation_color(grass_sample(p,vec2<f32>(0.0005,0.0),vec2<f32>(0.0,0.0005))*tint.rgb)*r.light_alpha.x;
    var s=prepare_surface(v);
    s.finish=vec3<f32>(0.96,0.0,0.0);
    let rgb=shade_surface_lit(v,true,1.0,s,local_irradiance(v.world,s.normal)).rgb;
    return vec4<f32>(rgb,r.light_alpha.y);
}
