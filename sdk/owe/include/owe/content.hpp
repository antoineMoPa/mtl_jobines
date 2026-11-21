#pragma once
#include "game.hpp"

extern "C" int32_t owe_content_v1(uint32_t kind, const char *name, const char *owner,
                                  const char *prefab, const float *values,
                                  const unsigned char *asset, uint32_t size);
#ifndef __wasm__
extern "C" int32_t owe_run_defined_v1(int32_t (*define)(), owe_resume_v1);
#endif

namespace owe {
enum class Canopy : uint32_t { BroadMaple, UprightMaple, YoungMaple };
struct FloraDefinition {
    Canopy canopy;
    float height_metres;
};
// Executed once before the engine opens the window. Asset bytes are copied by the host.
struct Content {
    Error status = Error::Ok;
    void title(const char *name) { add(1, name); }
    template<unsigned long N> void map(const char *name, const unsigned char (&road)[N]) {
        add(2, name, "", "", {}, {}, {}, road, N);
    }
    template<unsigned long N> void elevation(const char *map, const unsigned char (&png)[N]) {
        add(3, map, "", "", {}, {}, {}, png, N);
    }
    void box(const char *prefab, Vec3 min, Vec3 max, Vec3 colour) {
        add(4, prefab, "", "", min, max, colour);
    }
    void flora(const char *prefab, FloraDefinition definition) {
        add(10, prefab, "", "", {static_cast<float>(definition.canopy), definition.height_metres, 0});
    }
    void impact_material(const char *prefab, ImpactMaterial material) {
        add(9, prefab, "", "", {static_cast<float>(material), 0, 0});
    }
    void place(const char *map, const char *name, const char *prefab, Vec3 position, bool fixed=false) {
        add(5, name, map, prefab, position, {float(fixed),0,0});
    }
    template<unsigned long N> void view_model(const char *name, const unsigned char (&glb)[N], float metres) {
        add(6, name, "", "", {metres,0,0}, {}, {}, glb, N);
    }
    template<unsigned long N> void sky_texture(const char *name, const unsigned char (&png)[N]) {
        add(7, name, "", "", {}, {}, {}, png, N);
    }
    template<unsigned long N> void sky_shader(const char *name, const unsigned char (&wgsl)[N]) {
        add(8, name, "", "", {}, {}, {}, wgsl, N);
    }
private:
    void add(uint32_t kind, const char *name, const char *owner="", const char *prefab="",
             Vec3 a={},Vec3 b={},Vec3 c={},const unsigned char *asset=nullptr,uint32_t size=0) {
        if(status!=Error::Ok) return;
        const float values[]={a.x,a.y,a.z,b.x,b.y,b.z,c.x,c.y,c.z};
        status=static_cast<Error>(owe_content_v1(kind,name,owner,prefab,values,asset,size));
    }
};
}

#define OWE_DECLARE(content_fn) \
    extern "C" int32_t owe_client_define_v1() { owe::Content content; content_fn(content); return static_cast<int32_t>(content.status); }
#ifdef __wasm__
#define OWE_CLIENT(content_fn, play_fn) OWE_DECLARE(content_fn) OWE_GAME(play_fn)
#else
#define OWE_CLIENT(content_fn, play_fn) OWE_DECLARE(content_fn) \
    int main() {return owe_run_defined_v1(owe_client_define_v1,owe::resume<play_fn>);}
#endif
