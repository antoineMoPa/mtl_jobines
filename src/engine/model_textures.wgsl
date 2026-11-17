@group(3) @binding(14) var model_colour:texture_2d_array<f32>;
@group(3) @binding(15) var model_detail:texture_2d_array<f32>;
@group(3) @binding(16) var model_sampler:sampler;
fn model_surface(v:Output,front:bool,du:vec2<f32>,dv:vec2<f32>,dx:vec3<f32>,dy:vec3<f32>,surface:SurfaceData)->vec4<f32> {
    let layer=i32(v.surface.w);
    let colour=textureSampleGrad(model_colour,model_sampler,v.fence_uv,layer,du,dv);
    let detail=textureSampleGrad(model_detail,model_sampler,v.fence_uv,layer,du,dv);
    var n=v.normal/max(length(v.normal),0.000001);
    let determinant=du.x*dv.y-du.y*dv.x;
    if abs(determinant)>0.0000000001 {
        let raw_t=(dx*dv.y-dy*du.y)/determinant;
        let raw_b=(dy*du.x-dx*dv.x)/determinant;
        let tangent=raw_t-n*dot(n,raw_t);
        let t=tangent/max(length(tangent),0.000001);
        let b=cross(n,t)*select(-1.0,1.0,dot(cross(n,t),raw_b)>=0.0);
        let encoded=detail.rgb*2.0-vec3<f32>(1.0);
        n=t*encoded.x+b*encoded.y+n*encoded.z;
        n/=max(length(n),0.000001);
    }
    var shaded=v;shaded.color*=colour.rgb;shaded.surface.x=9.0;
    var material=surface;material.normal=n;material.finish=vec3<f32>(detail.a,0.0,0.0);
    return shade_surface_lit(shaded,front,1.0,material,local_irradiance(v.world,n));
}
struct ModelTransform { @location(4) x:vec4<f32>,@location(5) y:vec4<f32>,@location(6) z:vec4<f32>,@location(7) translation:vec4<f32> };
@vertex fn vs_model(v:Input,instance:ModelTransform)->Output {
    let model=mat4x4<f32>(instance.x,instance.y,instance.z,instance.translation);
    let a=instance.x.xyz;let b=instance.y.xyz;let c=instance.z.xyz;
    let n=(cross(b,c)*v.normal.x+cross(c,a)*v.normal.y+cross(a,b)*v.normal.z)/dot(a,cross(b,c));
    var o:Output;o.world=(model*vec4<f32>(v.position,1.0)).xyz;o.clip=camera.view_projection*vec4<f32>(o.world,1.0);
    o.normal=n/max(length(n),0.000001);o.color=v.color;o.surface=v.surface;o.fence_uv=v.surface.yz;o.snow_control=vec2<f32>(0.0);return o;
}
@fragment fn fs_model(v:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    let du=dpdx(v.fence_uv);let dv=dpdy(v.fence_uv);let dx=dpdx(v.world);let dy=dpdy(v.world);let surface=prepare_surface(v);
    if v.surface.x==42.0 {return model_surface(v,front,du,dv,dx,dy,surface);}
    return shade_surface_lit(v,front,1.0,surface,local_irradiance(v.world,surface.normal));
}
