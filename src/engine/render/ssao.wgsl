struct Settings {
    inverse:mat4x4<f32>, eye_radius:vec4<f32>, viewport:vec4<f32>, tuning:vec4<f32>,
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
@fragment fn composite(@builtin(position) screen:vec4<f32>)->@location(0) vec4<f32> {
    let p=vec2<i32>(screen.xy);
    let color=textureLoad(scene,p,0);
    if depth_at(p)>=0.999999 || settings.tuning.z<0.5 {return color;}
    let center=length(settings.eye_radius.xyz-position(p));
    let size=vec2<i32>(textureDimensions(occlusion));
    let q=p/2;var total=0.0;var weight=0.0;
    let tolerance=settings.eye_radius.w*0.22;
    for(var y=-1;y<=1;y++) {for(var x=-1;x<=1;x++) {
        let uv=clamp(q+vec2<i32>(x,y),vec2<i32>(0),size-vec2<i32>(1));
        let sample=textureLoad(occlusion,uv,0).rg;
        let difference=(sample.y-center)/tolerance;
        let spatial=1.0/(1.0+f32(x*x+y*y));
        let w=exp(-difference*difference)*spatial;
        total+=sample.x*w;weight+=w;
    }}
    let factor=select(1.0,total/max(weight,0.000001),weight>0.000001);
    return vec4<f32>(color.rgb*factor,color.a);
}
