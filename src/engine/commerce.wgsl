// Complementary coverage keeps opaque depth writes and foliage alpha cutouts.
fn tree_lod_mask(pixel:vec2<f32>,fade:f32) {
    let p=floor(pixel);
    let threshold=fract(52.9829189*fract(dot(p,vec2<f32>(0.06711056,0.00583715))));
    if fade>0.0 && threshold<fade {discard;}
    if fade<0.0 && threshold>=-fade {discard;}
}
// Game-private material IDs: 40 = approved alpha sign; 41 = virtual shop room.
// Vertex stride remains 52 bytes. yz carry interpolated UV; w is a stable atlas layer.
@group(3) @binding(0) var commerce_signs: texture_2d_array<f32>;
@group(3) @binding(1) var commerce_rooms: texture_2d<f32>;
@group(3) @binding(2) var commerce_sampler: sampler;
@group(3) @binding(8) var tree_atlas: texture_2d_array<f32>;
@group(3) @binding(9) var tree_sampler: sampler;
@group(3) @binding(10) var tree_impostors: texture_2d_array<f32>;
@group(3) @binding(11) var tree_impostor_sampler: sampler;
fn tree_image_color(v:Output)->vec4<f32> {
    let winter=select(0,6,camera.daylight_sky.z>0.2);
    let layer=i32(v.surface.x)-24+winter;
    let tex=textureSampleGrad(tree_impostors,tree_impostor_sampler,v.fence_uv,layer,dpdx(v.fence_uv),dpdy(v.fence_uv));
    if tex.a<0.12 {discard;}
    let eye=camera.eye_time.xyz-v.world;
    let distance=length(eye);
    let fog=(1.0-exp(-distance*0.025)*(1.0-smoothstep(5.8,7.6,distance)))*(1.0-camera.daylight_sky.y);
    let sky=textureSampleLevel(sky_environment,sky_sampler,-eye/max(distance,0.00001),0.0).rgb;
    return vec4<f32>(filmic_display(mix(tex.rgb,sky,fog)*camera.sun_exposure.w),tex.a);
}
// Explicit layer mapping: foliage, grass, spare, bark, spare, bark normal.
fn tree_layer(kind:i32)->i32 {
    if kind==16 {return 1;}
    if kind>=18 {return 3;}
    return 0;
}
fn tree_color(v:Output,front:bool)->vec4<f32> {
    let kind=i32(v.surface.x);
    if kind<=17 && camera.daylight_sky.z>0.2 {discard;}
    if kind==19 && camera.daylight_sky.z<=0.2 {discard;}
    let tex=textureSampleGrad(tree_atlas,tree_sampler,v.fence_uv,tree_layer(kind),dpdx(v.fence_uv),dpdy(v.fence_uv));
    if kind<=17 && tex.a<0.42 {discard;}
    var material=v;
    material.color*=tex.rgb;
    material.surface=vec4<f32>(7.0,1.0,0.0,0.0);
    var n=v.normal/max(length(v.normal),0.0001);
    if kind<=17 && !front {n=-n;}
    if kind==18 {
        // Cotangent frame from the actual trunk UVs; no extra vertex format.
        let dx=dpdx(v.world);let dy=dpdy(v.world);
        let ux=dpdx(v.fence_uv);let uy=dpdy(v.fence_uv);
        let p=cross(dy,n);let q=cross(n,dx);
        let t=p*ux.x+q*uy.x;let b=p*ux.y+q*uy.y;
        let scale=inverseSqrt(max(max(dot(t,t),dot(b,b)),0.0000000001));
        // Normal layer shares the sRGB array; undo its transfer before decoding.
        let encoded=textureSampleGrad(tree_atlas,tree_sampler,v.fence_uv,5,ux,uy).rgb;
        let raw=select(encoded*12.92,1.055*pow(encoded,vec3<f32>(1.0/2.4))-0.055,encoded>vec3<f32>(0.0031308));
        let detail=raw*2.0-1.0;
        let perturbed=(t*detail.x+b*detail.y)*scale*0.7+n*detail.z;
        n=perturbed/max(length(perturbed),0.0001);
    }
    if kind==19 {material.color=vec3<f32>(0.86,0.92,0.96);}
    material.normal=n;
    let surface=prepare_surface(material);
    let sun=camera.sun_exposure.xyz/max(length(camera.sun_exposure.xyz),0.0001);
    var transmitted=vec3<f32>(0.0);
    if kind==15 {
        let back=pow(max(dot(-n,sun),0.0),1.3);
        let shell=smoothstep(0.38,0.94,v.color.r);
        transmitted=camera.sun_color.rgb*camera.sun_color.w*vec3<f32>(1.0,0.94,0.48)
            *0.25*back*shell*clamp(camera.daylight_sky.x,0.0,1.0)*visibility(v.world,-n);
    }
    return shade_surface_lit(material,front,1.0,surface,local_irradiance(v.world,n)+transmitted);
}
fn tree_shadow(v:ShadowOutput) {
    let kind=i32(v.kind_pattern.x);
    if kind<=17 && camera.daylight_sky.z>0.2 {discard;}
    if (kind==19 || kind==22 || kind==23) && camera.daylight_sky.z<=0.2 {discard;}
    if kind<=17 {
        let alpha=textureSampleLevel(tree_atlas,tree_sampler,v.fence_uv,tree_layer(kind),0.0).a;
        if alpha<0.42 {discard;}
    }
}
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
    let fog=(1.0-exp(-length(eye)*0.025)*(1.0-smoothstep(5.8,7.6,length(eye))))*(1.0-camera.daylight_sky.y);
    let sky=textureSampleLevel(sky_environment,sky_sampler,-view,0.0).rgb;
    return vec4<f32>(filmic_display(mix(lit,sky,fog)*camera.sun_exposure.w),coverage);
}
// Colored vehicle/signal lamps carry an explicit marker; ordinary street
// lighting retains its authored warm emission in the shared shader.
fn lamp_color(v:Output)->vec4<f32> {
    return lamp_color_strength(v,1.0);
}
fn lamp_color_strength(v:Output,strength:f32)->vec4<f32> {
        let eye=camera.eye_time.xyz-v.world;
        let distance=length(eye);
        let radiance=v.color*2.0*strength*camera.sun_exposure.w;
        let fog=(1.0-exp(-distance*0.025)*(1.0-smoothstep(5.8,7.6,distance)))*(1.0-camera.daylight_sky.y);
        let sky=textureSampleLevel(sky_environment,sky_sampler,-eye/max(distance,0.00001),0.0).rgb*camera.sun_exposure.w;
        return vec4<f32>(filmic_display(mix(radiance,sky,fog)),1.0);
}
// No reachable discard, preserving early depth rejection for opaque scenery.
@fragment fn fs_opaque_game(v:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    let surface=prepare_surface(v);
    if v.surface.x>8.5 && v.surface.x<9.5 && v.surface.w>=2.0 && v.surface.w<=109.0 {
        let encoded=u32(v.surface.w);
        let base=encoded&15u;
        let strip=base>=10u;
        let state=base-select(2u,10u,strip);
        // A small inset segment of each original lens is clear for reversing.
        // Continuous bars retain their silhouette with two end segments.
        let u=v.fence_uv.x;let z=v.fence_uv.y;
        let clear_lens=select(u>0.18 && u<0.82 && z>0.12 && z<0.32,
            ((u>0.08 && u<0.14)||(u>0.86 && u<0.92)) && z>0.18 && z<0.82,strip);
        let lit_lens=select((state&1u)!=0u,(state&2u)!=0u,clear_lens);
        var lens=v;
        lens.surface.x=7.0;
        let flashing=((encoded&16u)!=0u && (!strip || u>0.75)) ||
            ((encoded&32u)!=0u && (!strip || u<0.25));
        if flashing && !clear_lens {
            lens.color=vec3<f32>(1.0,0.3,0.001);
            return lamp_color(lens);
        }
        if clear_lens {lens.color=vec3<f32>(0.62,0.65,0.64);}
        if lit_lens {
            lens.color=select(vec3<f32>(1.0,0.001,0.0005),vec3<f32>(1.0,1.0,0.98),clear_lens);
            return lamp_color(lens);
        }
        if (encoded&64u)!=0u && !clear_lens {
            let unlit=shade_local_surface(lens,front,1.0,surface);
            let night=1.0-clamp(camera.daylight_sky.x,0.0,1.0);
            lens.color=vec3<f32>(1.0,0.001,0.0005);
            return mix(unlit,lamp_color_strength(lens,0.12),night*night);
        }
        return shade_local_surface(lens,front,1.0,surface);
    }
    if v.surface.x>8.5 && v.surface.x<9.5 && v.surface.w==1.0 {
        return lamp_color(v);
    }
    return shade_local_surface(v,front,1.0,surface);
}
@fragment fn fs_game(v:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    if v.surface.x>=24.0 && v.surface.x<=29.0 {return tree_image_color(v);}
    if v.surface.x>=15.0 && v.surface.x<=23.0 {return tree_color(v,front);}
    let du=dpdx(v.fence_uv);let dv=dpdy(v.fence_uv);let dx=dpdx(v.world);let dy=dpdy(v.world);
    let surface=prepare_surface(v);let footprint=fwidth(v.fence_uv);
    if v.surface.x>39.5 {return commerce_color(v,du,dv,dx,dy);}
    var coverage=1.0;
    if v.surface.x>10.5&&v.surface.x<11.5 {coverage=fence_coverage(v.fence_uv,v.surface.w,footprint);if coverage<0.5 {discard;}}
    return shade_local_surface(v,front,coverage,surface);
}
@fragment fn fs_shadow_game(v:ShadowOutput) {
    if v.kind_pattern.x>=15.0 && v.kind_pattern.x<=23.0 {tree_shadow(v);return;}
    let footprint=fwidth(v.fence_uv);
    // Thin applied decals do not cast a second rectangular shadow on their shell.
    if v.kind_pattern.x>39.5&&v.kind_pattern.x<40.5 {discard;}
    if v.kind_pattern.x>10.5&&v.kind_pattern.x<11.5 {if fence_coverage(v.fence_uv,v.kind_pattern.y,footprint)<0.5 {discard;}}
}
