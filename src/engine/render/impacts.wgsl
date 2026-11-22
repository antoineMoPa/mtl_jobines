struct Camera {
    vp: mat4x4<f32>, shadow: mat4x4<f32>, eye_time: vec4<f32>,
    sun_exposure: vec4<f32>, daylight_sky: vec4<f32>
};
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var crash_atlas: texture_2d<f32>;
@group(1) @binding(1) var crash_sampler: sampler;
struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) params: vec4<f32>,
};
@vertex fn vs(@location(0) pos:vec3<f32>, @location(1) uv:vec2<f32>,
              @location(2) color:vec3<f32>, @location(3) params:vec4<f32>) -> Varyings {
    return Varyings(cam.vp*vec4(pos,1.0),uv,color,params);
}
fn hash(p:vec2<f32>) -> f32 {
    var h=fract(vec3(p.xyx)*0.1031);
    h+=dot(h,h.yzx+33.33);
    return fract((h.x+h.y)*h.z);
}
fn noise(p:vec2<f32>) -> f32 {
    let i=floor(p); let f=fract(p); let w=f*f*(3.0-2.0*f);
    return mix(mix(hash(i),hash(i+vec2(1.,0.)),w.x),mix(hash(i+vec2(0.,1.)),hash(i+1.),w.x),w.y);
}
fn cloud(p:vec2<f32>) -> f32 {
    return 0.54*noise(p)+0.28*noise(p*2.07+17.4)+0.13*noise(p*4.13-7.2)+0.05*noise(p*8.3);
}
@fragment fn fs(v:Varyings) -> @location(0) vec4<f32> {
    let uv=v.uv; let seed=v.params.y;
    if v.params.x < 0. {
        let r = length(uv);
        let core = 1. - smoothstep(0.12, 0.49, r);
        let halo = 1. - smoothstep(0., 1., r);
        return vec4(v.color, (core + (1. - core) * 0.31 * halo * halo) * v.params.z);
    }
    let light=mix(0.18,0.95,clamp(cam.daylight_sky.x,0.,1.));
    if v.params.x>10.5 {
        return vec4(vec3(1.,0.49,0.055), (1.-smoothstep(0.2,1.,length(uv)))*v.params.z);
    }
    if v.params.x>8.5 {
        let tile=vec2(seed%2.,floor(seed*0.5));
        let puff=textureSampleLevel(crash_atlas,crash_sampler,(tile+vec2(uv.x,-uv.y)*0.5+0.5)*0.5,0.).rg;
        let alpha=puff.g*v.params.z;
        if alpha<0.007 {discard;}
        if v.params.x>9.5 {
            let heat=puff.r*0.73+0.42*(1.-v.params.w);
            let fire=mix(vec3(1.,0.075,0.002),vec3(1.,0.55,0.035),smoothstep(0.10,0.66,heat));
            return vec4(mix(fire,vec3(1.,0.94,0.57),smoothstep(0.63,0.96,heat)),alpha);
        }
        let smoke=mix(vec3(0.018,0.017,0.017),vec3(0.36,0.33,0.29),puff.r)*light;
        let ember=vec3(0.24,0.045,0.003)*exp(-10.*v.params.w)*(1.-puff.r);
        return vec4(smoke+ember,alpha);
    }
    if v.params.x<0.5 {
        let flow=vec2(0.14,-0.52)*v.params.w;
        let p=uv*2.4+vec2(seed,seed*0.61)+flow;
        let f=cloud(p+0.63*vec2(noise(p+8.),noise(p-7.)));
        let density=1.-smoothstep(0.04,0.95,length(uv)+(0.5-f)*1.05);
        let rim=cloud(p+vec2(-0.17,0.24))-f;
        return vec4(v.color*light*(0.78+f*0.39+rim*1.6),density*v.params.z*(0.43+0.57*f));
    }
    if v.params.x>6.5 {
        let grain=cloud(uv*vec2(5.,3.)+seed+v.params.w*3.);
        var heat=0.;
        if v.params.x<7.5 {
            let x=uv.x*0.5+0.5;
            let width=(0.06+0.67*sin(pow(x,0.70)*3.14159))*(1.18-0.55*x);
            let ribbon=abs(uv.y+(grain-0.5)*(0.18+0.8*x));
            heat=(1.-smoothstep(0.,width,ribbon))*(1.-smoothstep(0.40,0.99,x+(0.5-grain)*0.30));
            heat*=smoothstep(0.,0.085,x);
        } else {
            let r=length(uv);
            let angle=atan2(uv.y,uv.x)+seed;
            let lobes=pow(0.5+0.5*cos(angle*5.+1.15*sin(angle*3.+seed)),2.7);
            let edge=0.42+lobes*0.40+(grain-0.5)*0.26;
            heat=1.-smoothstep(0.,edge,r);
        }
        let flame=mix(vec3(1.,0.14,0.009),vec3(1.,0.61,0.09),smoothstep(0.08,0.60,heat));
        let core=mix(flame,vec3(1.,0.98,0.78),smoothstep(0.48,0.88,heat));
        return vec4(core,smoothstep(0.015,0.59,heat)*v.params.z);
    }
    if v.params.x>5.5 {
        let r=length(uv);
        let a=atan2(uv.y,uv.x)+seed;
        let spokes=abs(sin(4.*a+0.33*noise(uv*9.+seed)+0.06*sin(r*28.+seed)));
        let cracks=(1.-smoothstep(0.011,0.065,spokes*max(0.12,r)))*(1.-smoothstep(0.35,1.,r));
        let pit=1.-smoothstep(0.055,0.14,r+0.034*noise(uv*11.+seed));
        return vec4(vec3(0.70,0.83,0.86)*light,clamp(cracks*0.84+pit*0.75,0.,1.));
    }
    let wood=v.params.x<1.5;
    let soil=v.params.x>2.5 && v.params.x<3.5;
    let grain=cloud(uv*select(vec2(6.),vec2(14.,2.4),wood)+seed);
    let tooth=cloud(uv*3.0+seed*1.3);
    let d=length(uv)+(grain-0.5)*select(0.49,0.95,wood)+(tooth-0.5)*0.22;
    let alpha=1.-smoothstep(0.78,0.97,d);
    let cavity=1.-smoothstep(0.19,0.49,d);
    var rim=mix(vec3(0.23,0.25,0.23),vec3(0.65,0.65,0.56),grain);
    var core=vec3(0.044,0.044,0.037);
    if wood {
        rim=mix(vec3(0.25,0.12,0.05),vec3(0.8,0.54,0.24),grain);
        core=vec3(0.045,0.022,0.009);
    }
    if soil {rim=vec3(0.19,0.135,0.08)*(0.7+grain);core=vec3(0.065,0.046,0.031);}
    if v.params.x>3.5 {rim=mix(vec3(0.10),vec3(0.39,0.37,0.34),grain);core=vec3(0.027);}
    if v.params.x>4.5 {rim=mix(v.color*0.55,v.color*1.6+vec3(0.11),grain);core=v.color*0.14;}
    let bevel=1.+0.27*dot(uv,vec2(-0.4,0.9));
    return vec4(mix(rim*bevel,core,cavity)*light,alpha);
}
