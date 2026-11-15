struct Hud { rect:vec4<f32>, screen:vec4<f32>, map:vec4<f32>, mode:vec4<f32>, full_map:vec4<f32>, destination:vec4<f32>, performance:vec4<f32>, vehicle:vec4<f32>, vehicle_letters:array<vec4<u32>,12> };
@group(0) @binding(0) var chart:texture_2d<f32>;
@group(0) @binding(1) var filtering:sampler;
@group(0) @binding(2) var<uniform> hud:Hud;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index:u32)->Vertex {
    let corners=array<vec2<f32>,6>(vec2<f32>(0.,0.),vec2<f32>(1.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,0.),vec2<f32>(1.,1.),vec2<f32>(0.,1.));
    let uv=corners[index%6u];
    var rect=hud.rect;
    if hud.mode.x==0. {
        let size=max(1.,min(clamp(min(hud.screen.x,hud.screen.y)*0.29,60.,340.),min(hud.screen.x,hud.screen.y)-16.));
        rect=vec4<f32>(16.,hud.screen.y-size-16.,size,size);
        if index>=6u {rect=fps_rect();}
        if index>=12u {rect=vehicle_rect();}
    }
    let p=rect.xy+uv*rect.zw;
    var out:Vertex;out.position=vec4<f32>(p.x/hud.screen.x*2.-1.,1.-p.y/hud.screen.y*2.,0.,1.);
    out.uv=select(uv,p,hud.mode.x==0.);return out;
}
fn inside(p:vec2<f32>, r:vec4<f32>)->bool {
    return all(p>=r.xy)&&all(p<=r.xy+r.zw);
}
// Small bitmap lettering keeps menus independent of a platform font service.
fn glyph(code:u32)->u32 {
    switch code {
        case 43u: {return 4357252u;}
        case 47u: {return 1118480u;}
        case 45u: {return 31744u;}
        case 48u: {return 15259182u;}
        case 49u: {return 14815428u;}
        case 50u: {return 32645678u;}
        case 51u: {return 16267791u;}
        case 52u: {return 8682793u;}
        case 53u: {return 16268351u;}
        case 54u: {return 15252526u;}
        case 55u: {return 4334111u;}
        case 56u: {return 15252014u;}
        case 57u: {return 15235630u;}
        case 65u, 97u: {return 18415150u;}
        case 66u, 98u: {return 16301615u;}
        case 67u, 99u: {return 31491134u;}
        case 68u, 100u: {return 16303663u;}
        case 69u, 101u: {return 32554047u;}
        case 70u, 102u: {return 1096767u;}
        case 71u, 103u: {return 32044094u;}
        case 72u, 104u: {return 18415153u;}
        case 73u, 105u: {return 32641183u;}
        case 74u, 106u: {return 6594844u;}
        case 75u, 107u: {return 18128177u;}
        case 76u, 108u: {return 32539681u;}
        case 77u, 109u: {return 18405233u;}
        case 78u, 110u: {return 18667121u;}
        case 79u, 111u: {return 15255086u;}
        case 80u, 112u: {return 1097263u;}
        case 81u, 113u: {return 32294446u;}
        case 82u, 114u: {return 18136623u;}
        case 83u, 115u: {return 16267326u;}
        case 84u, 116u: {return 4329631u;}
        case 85u, 117u: {return 15255089u;}
        case 86u, 118u: {return 4539953u;}
        case 87u, 119u: {return 18732593u;}
        case 88u, 120u: {return 18157905u;}
        case 89u, 121u: {return 4329809u;}
        case 90u, 122u: {return 32575775u;}
        default: {return 0u;}
    }
}
fn fps_scale()->f32 {return clamp(hud.screen.y/800.0,1.0,2.0);}
fn vehicle_scale()->f32 {return min(2.0*fps_scale(),max(0.5,(hud.screen.x-40.0)/max(6.0*hud.vehicle.x,1.0)));}
fn vehicle_rect()->vec4<f32> {
    let scale=vehicle_scale();
    let width=hud.vehicle.x*6.0*scale+20.0;
    return vec4<f32>(hud.screen.x-width-16.0,hud.screen.y-48.0,width,32.0);
}
fn vehicle_lettering(p:vec2<f32>)->bool {
    let q=(p-vehicle_rect().xy-vec2<f32>(10.0,10.0))/vehicle_scale();
    if q.x<0.0 || q.y<0.0 || q.y>=5.0 {return false;}
    let index=u32(q.x)/6u;let col=u32(q.x)%6u;
    if index>=u32(hud.vehicle.x) || col>=5u {return false;}
    let code=hud.vehicle_letters[index/4u][index%4u];
    return (glyph(code)&(1u<<(u32(q.y)*5u+col)))!=0u;
}
fn fps_rect()->vec4<f32> {
    let scale=fps_scale();
    return vec4<f32>(16.0*scale,16.0*scale,126.0*scale,32.0*scale);
}
fn fps_lettering(p:vec2<f32>,origin:vec2<f32>,scale:f32)->bool {
    let q=(p-origin)/scale;
    if q.x<0.0 || q.y<0.0 || q.y>=5.0 {return false;}
    let index=u32(q.x)/6u;let col=u32(q.x)%6u;
    if index>=8u || col>=5u {return false;}
    var code=32u;
    if index<4u {
        if hud.performance.x<=0.0 {code=45u;}
        else {
            let fps=u32(clamp(round(hud.performance.x),1.0,9999.0));
            let divisor=array<u32,4>(1000u,100u,10u,1u)[index];
            if fps>=divisor || index==3u {code=48u+(fps/divisor)%10u;}
        }
    }
    if index==5u {code=70u;}
    if index==6u {code=80u;}
    if index==7u {code=83u;}
    return (glyph(code)&(1u<<(u32(q.y)*5u+col)))!=0u;
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
    if word==9u && index<24u {
        code=array<u32,24>(83u,67u,82u,79u,76u,76u,32u,43u,47u,45u,32u,90u,79u,79u,77u,32u,32u,48u,32u,82u,69u,83u,69u,84u)[index];
    }
    if word==10u && index<13u {
        code=array<u32,13>(67u,76u,73u,67u,75u,32u,84u,79u,32u,74u,85u,77u,80u)[index];
    }
    if word==11u && index<21u {
        code=array<u32,21>(82u,73u,71u,72u,84u,32u,67u,76u,73u,67u,75u,32u,84u,79u,32u,67u,65u,78u,67u,69u,76u)[index];
    }
    if word==12u && index<19u {
        code=array<u32,19>(67u,72u,79u,79u,83u,69u,32u,67u,76u,69u,65u,82u,32u,71u,82u,79u,85u,78u,68u)[index];
    }
    return (glyph(code) & (1u << (u32(q.y)*5u+col)))!=0u;
}
// Full-map display uses a clockwise quarter turn of the north-up atlas.
fn atlas_to_map_offset(offset:vec2<f32>)->vec2<f32> {
    return vec2<f32>(-offset.y,offset.x);
}
fn map_to_atlas_offset(offset:vec2<f32>)->vec2<f32> {
    return vec2<f32>(offset.y,-offset.x);
}
fn map_color(uv:vec2<f32>, pixels:vec2<f32>, forward:vec2<f32>, atlas_pixels:f32)->vec3<f32> {
    var color=textureSampleLevel(chart,filtering,uv,0.0).rgb;
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
    let p=select(v.uv*hud.rect.zw,v.uv,hud.mode.x==0.);
    let screen=hud.screen.xy;
    if hud.mode.x==0. && inside(p,vehicle_rect()) {
        if hud.vehicle.x<1.0 {discard;}
        if vehicle_lettering(p) {return vec4<f32>(0.93,0.96,0.95,1.0);}
        return vec4<f32>(0.015,0.025,0.035,0.85);
    }
    if hud.mode.x==0. && inside(p,fps_rect()) {
        let scale=fps_scale();
        let origin=fps_rect().xy+vec2<f32>(10.0,11.0)*scale;
        if fps_lettering(p,origin,2.0*scale) {return vec4<f32>(0.93,0.96,0.95,1.0);}
        return vec4<f32>(0.015,0.025,0.035,0.85);
    }
    if hud.mode.x==1. {
        var color=vec4<f32>(0.015,0.025,0.035,0.78);
        if inside(p,vec4<f32>(screen*0.5-vec2<f32>(150.,85.),300.,280.)) {color=vec4<f32>(0.035,0.055,0.07,1.);}
        if inside(p,vec4<f32>(screen.x*0.5-110.,screen.y*0.5,220.,56.)) {color=vec4<f32>(0.12,0.28,0.32,1.);}
        if inside(p,vec4<f32>(screen.x*0.5-110.,screen.y*0.5+68.,220.,48.)) {color=vec4<f32>(0.24,0.10,0.10,1.);}
        if inside(p,vec4<f32>(screen.x*0.5-110.,screen.y*0.5+128.,220.,48.)) {color=vec4<f32>(0.24,0.10,0.10,1.);}
        let selected_y=screen.y*0.5+select(0.,68.,hud.mode.z>=1.)+select(0.,60.,hud.mode.z==2.);
        let selected_height=select(56.,48.,hud.mode.z>=1.);
        let selected=vec4<f32>(screen.x*0.5-110.,selected_y,220.,selected_height);
        if inside(p,selected) && !inside(p,selected+vec4<f32>(3.,3.,-6.,-6.)) {
            color=vec4<f32>(1.,0.63,0.08,1.);
        }
        if lettering(p,vec2<f32>(screen.x*0.5-34.5,screen.y*0.5+144.),3.,4u) {color=vec4<f32>(0.93,0.96,0.95,1.);}
        if lettering(p,vec2<f32>(screen.x*0.5-25.5,screen.y*0.5+84.),3.,2u) {color=vec4<f32>(0.93,0.96,0.95,1.);}
        if lettering(p,vec2<f32>(screen.x*0.5-79.5,screen.y*0.5-49.),3.,0u)
            || (hud.mode.y==0. && lettering(p,vec2<f32>(screen.x*0.5-52.5,screen.y*0.5+20.),3.,1u))
            || (hud.mode.y==1. && lettering(p,vec2<f32>(screen.x*0.5-43.5,screen.y*0.5+20.),3.,3u)) {color=vec4<f32>(0.93,0.96,0.95,1.);}
        return color;
    }
    if hud.mode.x==2. {
        if !inside(p,vec4<f32>(20.,20.,screen-40.)) {return vec4<f32>(0.01,0.02,0.03,0.8);}
        if lettering(p,vec2<f32>(40.,37.),3.,2u) {return vec4<f32>(0.93,0.96,0.95,1.);}
        let jumping=hud.full_map.w>0.;
        if screen.x>700. && (lettering(p,vec2<f32>(140.,39.),2.,select(5u,10u,jumping))
            || lettering(p,vec2<f32>(screen.x-320.,39.),2.,select(6u,11u,jumping))) {return vec4<f32>(0.7,0.83,0.85,1.);}
        if hud.full_map.w==2. && lettering(p,vec2<f32>(40.,screen.y-33.),2.,12u) {return vec4<f32>(1.,0.3,0.2,1.);}
        if hud.full_map.w==1. && screen.x<=700. && lettering(p,vec2<f32>(40.,screen.y-33.),2.,10u) {return vec4<f32>(0.7,0.83,0.85,1.);}
        if !jumping && hud.destination.z==2. && lettering(p,vec2<f32>(40.,screen.y-33.),2.,7u) {return vec4<f32>(1.,0.3,0.2,1.);}
        if !jumping && hud.destination.z==3. && lettering(p,vec2<f32>(40.,screen.y-33.),2.,8u) {return vec4<f32>(0.1,1.,0.4,1.);}
        if screen.x>700. && lettering(p,vec2<f32>(screen.x-340.,screen.y-33.),2.,9u) {return vec4<f32>(0.7,0.83,0.85,1.);}
        let close=p-vec2<f32>(screen.x-44.,44.);
        if max(abs(close.x),abs(close.y))<9. && abs(abs(close.x)-abs(close.y))<2. {return vec4<f32>(0.93,0.96,0.95,1.);}
        let size=screen-vec2<f32>(80.,120.);
        let local=p-vec2<f32>(40.,80.);
        if any(local<vec2<f32>(0.)) || any(local>size) {return vec4<f32>(0.035,0.055,0.07,1.);}
        let span=min(size.x,size.y)*hud.full_map.z;
        let uv=hud.full_map.xy+map_to_atlas_offset((local-size*0.5)/span);
        let forward=atlas_to_map_offset(vec2<f32>(cos(hud.screen.w),-sin(hud.screen.w)));
        return vec4<f32>(map_color(uv,atlas_to_map_offset(uv-hud.map.zw)*span,forward,span),1.);
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
        return vec2<f32>(40.,80.)+size*0.5+atlas_to_map_offset(uv-hud.full_map.xy)*min(size.x,size.y)*hud.full_map.z;
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
