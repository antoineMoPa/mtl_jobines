struct Camera {
    vp:mat4x4<f32>, shadow:mat4x4<f32>, eye_time:vec4<f32>,
    sun_exposure:vec4<f32>, daylight_sky:vec4<f32>,
};
@group(0) @binding(0) var<uniform> cam:Camera;
struct WheelPixel {
    @builtin(position) clip:vec4<f32>,
    @location(0) uv:vec2<f32>,
    @location(1) ground:vec3<f32>,
    @location(2) @interpolate(flat) params:vec4<f32>,
};
@vertex fn vs(@builtin(vertex_index) vertex:u32,@location(0) start:vec4<f32>,
              @location(1) end:vec4<f32>,@location(2) outward:vec4<f32>)->WheelPixel {
    let uv=array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(1.,1.),vec2(-1.,-1.),vec2(1.,1.),vec2(-1.,1.))[vertex%6u];
    let delta=end.xyz-start.xyz;
    let along=delta/max(length(delta),0.000001);
    let across=vec3(-along.y,along.x,outward.z);
    let age=max(0.,cam.eye_time.w-start.w);
    var ground=mix(start.xyz,end.xyz,uv.x*0.5+0.5)+across*(uv.y*end.w*0.5);
    var pos=ground+vec3(0.,0.,0.00017);
    let spray=vertex>=6u;
    if spray && (outward.w<0. || age>=POWDER_LIFE) {return WheelPixel(vec4(2.,2.,2.,1.),uv,ground,vec4(0.));}
    if spray {
        ground=end.xyz;
        let right=vec3(cam.vp[0].x,cam.vp[1].x,cam.vp[2].x);
        let up=vec3(cam.vp[0].y,cam.vp[1].y,cam.vp[2].y);
        let drift=1.-exp(-age*2.7);
        let radius=(0.0028+age*0.012)*clamp(end.w/0.0024,0.45,1.35);
        pos=ground-along*(0.003+0.009*drift)+vec3(outward.xy,0.)*(0.001+0.006*drift)
            +vec3(0.,0.,0.0024+0.009*age)
            +radius*(right/max(length(right),0.001)*uv.x+up/max(length(up),0.001)*uv.y);
    }
    return WheelPixel(cam.vp*vec4(pos,1.),uv,ground,vec4(age,outward.w,f32(spray),length(delta)*100.));
}
fn grain(p:vec2<f32>)->f32 {
    let i=vec2<i32>(floor(p));let f=fract(p);let u=f*f*(3.-2.*f);
    var h:vec4<f32>;
    for(var k=0u;k<4u;k++){
        let q=bitcast<vec2<u32>>(i+vec2<i32>(i32(k&1u),i32(k>>1u)));
        var n=q.x*374761393u+q.y*668265263u;n=(n^(n>>13u))*1274126177u;
        h[k]=f32(n^(n>>16u))/4294967295.;
    }
    return mix(mix(h.x,h.y,u.x),mix(h.z,h.w,u.x),u.y);
}
@fragment fn fs(v:WheelPixel)->@location(0) vec4<f32>{
    let pixel=vec2<i32>(floor((v.ground.xy-snow_cache.xy)*snow_cache.z));
    if any(pixel<vec2<i32>(0)) || any(pixel>=vec2<i32>(i32(snow_cache.w))){discard;}
    if textureLoad(snow_exposure,pixel,0).r>v.ground.z+0.002 {discard;}
    let cover=1.-cleared_coverage(v.ground);
    if cover<=0.001 {discard;}
    let day=clamp(cam.daylight_sky.x,0.,1.);
    let age=v.params.x;
    if v.params.z>0.5 {
        let p=v.uv*2.2+v.params.y+vec2(0.,age);
        let n=.65*grain(p)+.35*grain(p*2.4);
        let density=1.-smoothstep(.12,.98,length(v.uv)+.65*(.5-n));
        let color=mix(vec3(.48,.60,.73),vec3(.92,.97,1.),n)*mix(.17,1.,day);
        return vec4(color,cover*density*(.59+.3*n)*(1.-smoothstep(.10,POWDER_LIFE,age)));
    }
    let edge=1.-smoothstep(.65,1.,abs(v.uv.y));
    let tread=abs(fract((v.uv.x*.5+.5)*v.params.w*9.+abs(v.uv.y)*.55)-.5);
    let rib=smoothstep(.22,.38,tread);
    return vec4(vec3(.26,.32,.38)*mix(.21,1.,day),
        cover*edge*(.32+.25*rib)*(1.-smoothstep(TRACK_LIFE*.65,TRACK_LIFE,age)));
}
