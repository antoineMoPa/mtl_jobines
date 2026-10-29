// Game-private material IDs: 40 = approved alpha sign; 41 = virtual shop room.
// Vertex stride remains 52 bytes. yz carry interpolated UV; w is a stable atlas layer.
@group(3) @binding(0) var commerce_signs: texture_2d_array<f32>;
@group(3) @binding(1) var commerce_rooms: texture_2d<f32>;
@group(3) @binding(2) var commerce_sampler: sampler;
fn shop_room(uv:vec2<f32>,size:vec2<f32>,n:vec3<f32>,tangent:vec3<f32>,eye:vec3<f32>,recipe:f32)->vec3<f32> {
    let origin=vec3<f32>(clamp(uv.x,0.0001,0.9999)*size.x,(1.0-clamp(uv.y,0.0001,0.9999))*size.y,0.0);
    let ray=vec3<f32>(-dot(eye,tangent),-eye.z,max(dot(eye,n),0.00001));
    let room_size=vec3<f32>(size,0.048);
    let exit_plane=select(vec3<f32>(0.0),room_size,ray>=vec3<f32>(0.0));
    let travel=select(vec3<f32>(1e10),(exit_plane-origin)/select(vec3<f32>(1.0),ray,abs(ray)>vec3<f32>(0.000001)),abs(ray)>vec3<f32>(0.000001));
    let exit=min(travel.x,min(travel.y,travel.z));
    let hit=clamp(origin+ray*exit,vec3<f32>(0.0),room_size);
    var color=vec3<f32>(0.30,0.28,0.22);
    if hit.y<0.00001 {color=vec3<f32>(0.16,0.14,0.11);}
    if hit.y>size.y-0.00001 {color=vec3<f32>(0.42,0.40,0.32);}
    // Composite back to front. Both layers are bounded by the SAME finite room.
    for(var layer=1;layer>=0;layer--) {
        let depth=select(0.0085,0.026,layer==1);
        let t=depth/ray.z;
        let p=origin+ray*t;
        if t<exit && all(p.xy>=vec2<f32>(0.0)) && all(p.xy<=size) {
            let local=vec2<f32>(p.x/size.x,1.0-p.y/size.y);
            let atlas=(vec2<f32>(f32(layer)*512.0,recipe*256.0)+vec2<f32>(4.0)+local*vec2<f32>(504.0,248.0))/1024.0;
            let sample=textureSampleLevel(commerce_rooms,commerce_sampler,atlas,0.0);
            color=mix(color,sample.rgb,sample.a);
        }
    }
    return color*mix(0.8,1.2,clamp(camera.daylight_sky.x,0.0,1.0));
}
fn commerce_color(v:Output,du:vec2<f32>,dv:vec2<f32>,dx:vec3<f32>,dy:vec3<f32>)->vec4<f32> {
    let n=v.normal;let eye=camera.eye_time.xyz-v.world;let view=eye/max(length(eye),0.00001);
    let daylight=clamp(camera.daylight_sky.x,0.0,1.0);
    var lit:vec3<f32>;var coverage=1.0;
    if v.surface.x<40.5 {
        let artwork=textureSampleLevel(commerce_signs,commerce_sampler,v.fence_uv,i32(v.surface.w),0.0);
        if artwork.a<0.45 {discard;}
        lit=artwork.rgb*mix(0.55,1.3,daylight);coverage=artwork.a;
    } else {
        // Recover physical room dimensions from continuous shared storefront UVs.
        let determinant=du.x*dv.y-du.y*dv.x;
        let inv=1.0/select(0.00000001,determinant,abs(determinant)>0.00000001);
        let horizontal=(dx*dv.y-dy*du.y)*inv;
        let vertical=(dy*du.x-dx*dv.x)*inv;
        let size=clamp(vec2<f32>(length(horizontal),length(vertical)),vec2<f32>(0.006),vec2<f32>(0.40,0.08));
        let room=shop_room(v.fence_uv,size,n,horizontal/max(length(horizontal),0.000001),eye,v.surface.w);
        lit=room_glass(room,n,view,daylight);
    }
    lit*=camera.sun_exposure.w;lit=lit/(lit+vec3<f32>(1.0));
    let fog=1.0-exp(-length(eye)*0.025)*(1.0-smoothstep(5.8,7.6,length(eye)));
    let sky=textureSampleLevel(sky_environment,sky_sampler,-view,0.0).rgb*camera.sun_exposure.w;
    return vec4<f32>(mix(lit,sky/(sky+vec3<f32>(1.0)),fog),coverage);
}
@fragment fn fs_game(v:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    let du=dpdx(v.fence_uv);let dv=dpdy(v.fence_uv);let dx=dpdx(v.world);let dy=dpdy(v.world);
    let surface=prepare_surface(v);let footprint=fwidth(v.fence_uv);
    if v.surface.x>39.5 {return commerce_color(v,du,dv,dx,dy);}
    var coverage=1.0;
    if v.surface.x>10.5&&v.surface.x<11.5 {coverage=fence_coverage(v.fence_uv,v.surface.w,footprint);if coverage<0.5 {discard;}}
    return shade_surface(v,front,coverage,surface);
}
@fragment fn fs_shadow_game(v:ShadowOutput) {
    let footprint=fwidth(v.fence_uv);
    // Thin applied decals do not cast a second rectangular shadow on their shell.
    if v.kind_pattern.x>39.5&&v.kind_pattern.x<40.5 {discard;}
    if v.kind_pattern.x>10.5&&v.kind_pattern.x<11.5 {if fence_coverage(v.fence_uv,v.kind_pattern.y,footprint)<0.5 {discard;}}
}
