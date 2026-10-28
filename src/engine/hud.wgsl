struct Hud { rect:vec4<f32>, screen:vec4<f32>, map:vec4<f32>, mode:vec4<f32>, destination:vec4<f32> };
@group(0) @binding(0) var chart:texture_2d<f32>;
@group(0) @binding(1) var filtering:sampler;
@group(0) @binding(2) var<uniform> hud:Hud;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index:u32)->Vertex {
    let corners=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));
    let uv=corners[index];let p=hud.rect.xy+uv*hud.rect.zw;
    var out:Vertex;out.position=vec4<f32>(p.x/hud.screen.x*2.-1.,1.-p.y/hud.screen.y*2.,0.,1.);out.uv=uv;return out;
}
fn inside(p:vec2<f32>, r:vec4<f32>)->bool {
    return all(p>=r.xy)&&all(p<=r.xy+r.zw);
}
// Small bitmap lettering keeps menus independent of a platform font service.
fn glyph(code:u32)->u32 {
    switch code {
        case 65u: {return 18415150u;}
        case 66u: {return 16301615u;}
        case 67u: {return 31491134u;}
        case 68u: {return 16303663u;}
        case 69u: {return 32554047u;}
        case 70u: {return 1096767u;}
        case 71u: {return 32044094u;}
        case 72u: {return 18415153u;}
        case 73u: {return 32641183u;}
        case 74u: {return 6594844u;}
        case 75u: {return 18128177u;}
        case 76u: {return 32539681u;}
        case 77u: {return 18405233u;}
        case 78u: {return 18667121u;}
        case 79u: {return 15255086u;}
        case 80u: {return 1097263u;}
        case 81u: {return 32294446u;}
        case 82u: {return 18136623u;}
        case 83u: {return 16267326u;}
        case 84u: {return 4329631u;}
        case 85u: {return 15255089u;}
        case 86u: {return 4539953u;}
        case 87u: {return 18732593u;}
        case 88u: {return 18157905u;}
        case 89u: {return 4329809u;}
        case 90u: {return 32575775u;}
        default: {return 0u;}
    }
}
fn lettering(p:vec2<f32>, origin:vec2<f32>, scale:f32, word:u32)->bool {
    let q=(p-origin)/scale;
    if q.x<0. || q.y<0. || q.y>=5. {return false;}
    let index=u32(q.x)/6u; let col=u32(q.x)%6u;
    if col>=5u {return false;}
    var code=32u;
    if word==0u && index<9u {
        code=array<u32,9>(71u,65u,77u,69u,32u,77u,69u,78u,85u)[index];
    }
    if word==1u && index<6u {
        code=array<u32,6>(82u,69u,83u,85u,77u,69u)[index];
    }
    if word==2u && index<3u {
        code=array<u32,3>(77u,65u,80u)[index];
    }
    if word==3u && index<5u {
        code=array<u32,5>(83u,84u,65u,82u,84u)[index];
    }
    if word==4u && index<4u {
        code=array<u32,4>(69u,88u,73u,84u)[index];
    }
    if word==5u && index<14u {
        code=array<u32,14>(67u,76u,73u,67u,75u,32u,84u,79u,32u,82u,79u,85u,84u,69u)[index];
    }
    if word==6u && index<20u {
        code=array<u32,20>(82u,73u,71u,72u,84u,32u,67u,76u,73u,67u,75u,32u,84u,79u,32u,67u,76u,69u,65u,82u)[index];
    }
    if word==7u && index<8u {
        code=array<u32,8>(78u,79u,32u,82u,79u,85u,84u,69u)[index];
    }
    if word==8u && index<7u {
        code=array<u32,7>(65u,82u,82u,73u,86u,69u,68u)[index];
    }
    return (glyph(code) & (1u << (u32(q.y)*5u+col)))!=0u;
}
fn map_color(uv:vec2<f32>, pixels:vec2<f32>, forward:vec2<f32>, atlas_pixels:f32)->vec3<f32> {
    var color=textureSample(chart,filtering,uv).rgb;
    if any(uv<vec2<f32>(0.)) || any(uv>vec2<f32>(1.)) {color=vec3<f32>(0.008,0.048,0.091);}
    if hud.destination.z>0. {
        let distance=length((uv-hud.destination.xy)*atlas_pixels);
        var marker=vec3<f32>(0.05,0.65,1.);
        if hud.destination.z==2. {marker=vec3<f32>(1.,0.16,0.1);}
        if hud.destination.z==3. {marker=vec3<f32>(0.1,1.,0.4);}
        if distance<10. {color=vec3<f32>(0.015,0.025,0.03);}
        if distance<8. {color=marker;}
        if distance<4. {color=vec3<f32>(0.95);}
    }
    let q=pixels;
    let left=vec2<f32>(-forward.y,forward.x);
    let along=dot(q,forward);let across=abs(dot(q,left));
    if along > -9. && along < 13. && across<(13.-along)*0.48 {color=vec3<f32>(0.015,0.025,0.03);}
    if along > -6. && along < 10. && across<(10.-along)*0.42 {color=vec3<f32>(1.,0.63,0.08);}
    return color;
}
@fragment fn fs(v:Vertex)->@location(0) vec4<f32> {
    let p=v.uv*hud.rect.zw;
    let screen=hud.screen.xy;
    if hud.mode.x==1. {
        var color=vec4<f32>(0.015,0.025,0.035,0.78);
        if inside(p,vec4<f32>(screen*0.5-vec2<f32>(150.,85.),300.,220.)) {color=vec4<f32>(0.035,0.055,0.07,1.);}
        if inside(p,vec4<f32>(screen.x*0.5-110.,screen.y*0.5,220.,56.)) {color=vec4<f32>(0.12,0.28,0.32,1.);}
        if inside(p,vec4<f32>(screen.x*0.5-110.,screen.y*0.5+68.,220.,48.)) {color=vec4<f32>(0.24,0.10,0.10,1.);}
        let selected_y=select(screen.y*0.5,screen.y*0.5+68.,hud.mode.z==1.);
        let selected_height=select(56.,48.,hud.mode.z==1.);
        let selected=vec4<f32>(screen.x*0.5-110.,selected_y,220.,selected_height);
        if inside(p,selected) && !inside(p,selected+vec4<f32>(3.,3.,-6.,-6.)) {
            color=vec4<f32>(1.,0.63,0.08,1.);
        }
        if lettering(p,vec2<f32>(screen.x*0.5-34.5,screen.y*0.5+84.),3.,4u) {color=vec4<f32>(0.93,0.96,0.95,1.);}
        if lettering(p,vec2<f32>(screen.x*0.5-79.5,screen.y*0.5-49.),3.,0u)
            || (hud.mode.y==0. && lettering(p,vec2<f32>(screen.x*0.5-52.5,screen.y*0.5+20.),3.,1u))
            || (hud.mode.y==1. && lettering(p,vec2<f32>(screen.x*0.5-43.5,screen.y*0.5+20.),3.,3u)) {color=vec4<f32>(0.93,0.96,0.95,1.);}
        return color;
    }
    if hud.mode.x==2. {
        if !inside(p,vec4<f32>(20.,20.,screen-40.)) {return vec4<f32>(0.01,0.02,0.03,0.8);}
        if lettering(p,vec2<f32>(40.,37.),3.,2u) {return vec4<f32>(0.93,0.96,0.95,1.);}
        if screen.x>700. && (lettering(p,vec2<f32>(140.,39.),2.,5u)
            || lettering(p,vec2<f32>(screen.x-320.,39.),2.,6u)) {return vec4<f32>(0.7,0.83,0.85,1.);}
        if hud.destination.z==2. && lettering(p,vec2<f32>(40.,screen.y-33.),2.,7u) {return vec4<f32>(1.,0.3,0.2,1.);}
        if hud.destination.z==3. && lettering(p,vec2<f32>(40.,screen.y-33.),2.,8u) {return vec4<f32>(0.1,1.,0.4,1.);}
        let close=p-vec2<f32>(screen.x-44.,44.);
        if max(abs(close.x),abs(close.y))<9. && abs(abs(close.x)-abs(close.y))<2. {return vec4<f32>(0.93,0.96,0.95,1.);}
        let size=screen-vec2<f32>(80.,120.);
        let local=p-vec2<f32>(40.,80.);
        if any(local<vec2<f32>(0.)) || any(local>size) {return vec4<f32>(0.035,0.055,0.07,1.);}
        let span=min(size.x,size.y);
        let uv=vec2<f32>(0.5)+(local-size*0.5)/span;
        let forward=vec2<f32>(cos(hud.screen.w),-sin(hud.screen.w));
        return vec4<f32>(map_color(uv,(uv-hud.map.zw)*span,forward,span),1.);
    }
    let size=max(1.,min(clamp(min(screen.x,screen.y)*0.29,60.,340.),min(screen.x,screen.y)-16.));
    let uv=(p-vec2<f32>(16.,screen.y-size-16.))/size;
    let radius=length((uv-0.5)*size);
    let edge=size*0.5-radius;
    if edge<0. {discard;}
    // Atlas Y points south. Screen up samples ahead of the player's heading;
    // screen right samples the player's right, keeping the marker upright.
    let forward=vec2<f32>(cos(hud.screen.w),-sin(hud.screen.w));
    let right=vec2<f32>(-forward.y,forward.x);
    let offset=uv-0.5;
    let chart_offset=right*offset.x-forward*offset.y;
    var color=map_color(hud.map.xy+chart_offset*hud.screen.z,offset*size,vec2<f32>(0.,-1.),size/hud.screen.z);
    if edge<3. {color=vec3<f32>(0.035,0.055,0.065);}
    // 100 m scale, positioned inside the circular mask.
    if uv.x>0.375 && uv.x<0.625 && uv.y>0.88 && uv.y<0.89 {color=vec3<f32>(0.95);}
    return vec4<f32>(color,smoothstep(0.,1.,edge));
}

// Route geometry remains in atlas coordinates; only the small ribbon buffer changes on reroute.
fn route_screen(uv:vec2<f32>)->vec2<f32> {
    if hud.mode.x==2. {
        let size=hud.screen.xy-vec2<f32>(80.,120.);
        return vec2<f32>(40.,80.)+size*0.5+(uv-0.5)*min(size.x,size.y);
    }
    let screen=hud.screen.xy;
    let size=max(1.,min(clamp(min(screen.x,screen.y)*0.29,60.,340.),min(screen.x,screen.y)-16.));
    let forward=vec2<f32>(cos(hud.screen.w),-sin(hud.screen.w));
    let right=vec2<f32>(-forward.y,forward.x);
    let offset=(uv-hud.map.xy)/hud.screen.z;
    return vec2<f32>(16.+size*0.5,screen.y-size*0.5-16.)+vec2<f32>(dot(offset,right),-dot(offset,forward))*size;
}
@vertex fn vs_route(@location(0) uv:vec2<f32>, @location(1) other:vec2<f32>, @location(2) side:f32)->Vertex {
    let a=route_screen(uv);let delta=route_screen(other)-a;
    let perpendicular=vec2<f32>(-delta.y,delta.x)/max(length(delta),0.001);
    let p=a+perpendicular*side*3.;
    var out:Vertex;
    out.position=vec4<f32>(p.x/hud.screen.x*2.-1.,1.-p.y/hud.screen.y*2.,0.,1.);
    out.uv=p;return out;
}
@fragment fn fs_route(v:Vertex)->@location(0) vec4<f32> {
    let p=v.uv;
    if hud.mode.x==2. {
        if !inside(p,vec4<f32>(40.,80.,hud.screen.xy-vec2<f32>(80.,120.))) {discard;}
    } else {
        let screen=hud.screen.xy;
        let size=max(1.,min(clamp(min(screen.x,screen.y)*0.29,60.,340.),min(screen.x,screen.y)-16.));
        if length(p-vec2<f32>(16.+size*0.5,screen.y-size*0.5-16.))>size*0.5-3. {discard;}
    }
    // Keep the destination and heading markers above the route.
    if length(p-route_screen(hud.map.zw))<14. || length(p-route_screen(hud.destination.xy))<10. {discard;}
    return vec4<f32>(0.05,0.65,1.,1.);
}
