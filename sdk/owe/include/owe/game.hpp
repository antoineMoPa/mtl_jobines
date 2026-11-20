#pragma once
#include "abi.h"
#include "coroutine.hpp"

static_assert(sizeof(owe_packet_v1) == 180, "OWE wire layout");

namespace owe {
enum class Error : int32_t {
    Ok,
    NoMap,
    NotFound,
    UnknownPrefab,
    StaleEntity,
    ReadOnly,
    Invalid,
    Closed
};
enum class Key : uint32_t {
    Left = 1,
    Right = 2,
    Down = 4,
    Up = 8,
    E = 16,
    P = 32,
    Escape = 64,
    Tab = 128, Fire = 256, Aim = 512, Reload = 1024, One = 2048, Two = 4096,
    W = 8192, A = 16384, S = 32768, D = 65536
};
struct Vec3 {
    float x{}, y{}, z{};
};
struct Entity {
    uint32_t id{};
};
template <class T> struct Result {
    T value{};
    Error error{};
    explicit operator bool() const { return error == Error::Ok; }
};
namespace detail {
struct Reply {
    owe_packet_v1 data;
    Error error;
};
inline Reply call(owe_operation_v1 op, Entity id = {}, Vec3 pos = {}, const char *text = "") {
    owe_packet_v1 p{static_cast<uint32_t>(op), id.id, {pos.x, pos.y, pos.z}, {}};
    unsigned i = 0;
    while (text[i] && i + 1 < sizeof p.text) {
        p.text[i] = text[i];
        ++i;
    }
    if (text[i])
        return {p, Error::Invalid};
    const auto error = static_cast<Error>(owe_call_v1(&p));
    return {p, error};
}
} // namespace detail
enum class MuzzleFlash { Rifle, Handgun };
enum class ImpactMaterial { Wood, Concrete };
struct Strike { Entity entity; Vec3 point; };
struct RoadPoint { float x, y, lap_length; };
struct World {
    Result<RoadPoint> road_loop(const char *road, float metres) const {
        auto r=detail::call(OWE_ROAD_LOOP,{}, {metres,0,0},road);
        return {{r.data.xyz[0],r.data.xyz[1],r.data.xyz[2]},r.error};
    }
    Error first_person(Entity actor, float yaw, float pitch, float eye_height) const {
        return detail::call(OWE_FIRST_PERSON, actor, {yaw, pitch, eye_height}).error;
    }
    // Local offset in metres: forward, left, up. Empty model name clears it.
    Error view_model(const char *name, Vec3 offset) const {
        return detail::call(OWE_VIEW_MODEL, {}, offset, name).error;
    }
    // Once per accepted shot, after positioning the current view model.
    Error muzzle_flash(MuzzleFlash style) const {
        return detail::call(OWE_MUZZLE_FLASH, {static_cast<uint32_t>(style)}).error;
    }
    Vec3 take_look_delta() const {
        auto r = detail::call(OWE_LOOK_DELTA);
        return {r.data.xyz[0], r.data.xyz[1], 0};
    }
    // Pick the nearest prefab collider along the first-person sight line.
    Result<Entity> pick_reticle(float distance) const {
        auto r = detail::call(OWE_PICK_RETICLE, {}, {distance, 0, 0});
        return {{r.data.id}, r.error};
    }
    // Emits the assigned surface effect. Terrain returns an empty entity at the hit point.
    Result<Strike> strike_reticle(float distance, float strength) const {
        auto r = detail::call(OWE_STRIKE_RETICLE, {}, {distance, strength, 0});
        return {{{r.data.id}, {r.data.xyz[0], r.data.xyz[1], r.data.xyz[2]}}, r.error};
    }
    // Break an upright YZ panel into painted pieces and remove its collider.
    Error shatter_panel(Strike hit) const {
        return detail::call(OWE_SHATTER_PANEL, hit.entity, hit.point).error;
    }
    Result<Entity> find(const char *name) const {
        auto r = detail::call(OWE_FIND, {}, {}, name);
        return {{r.data.id}, r.error};
    }
    Result<Entity> spawn(const char *prefab, Vec3 at) const {
        auto r = detail::call(OWE_SPAWN, {}, at, prefab);
        return {{r.data.id}, r.error};
    }
    Result<Vec3> position(Entity e) const {
        auto r = detail::call(OWE_POSITION, e);
        return {{r.data.xyz[0], r.data.xyz[1], r.data.xyz[2]}, r.error};
    }
    Error set_position(Entity e, Vec3 at) const {
        return detail::call(OWE_SET_POSITION, e, at).error;
    }
    // Local +X is the nose; both components are in the horizontal world plane.
    Error set_facing(Entity e, float x, float y) const {
        return detail::call(OWE_SET_FACING, e, {x, y, 0}).error;
    }
    Error velocity(Entity e, Vec3 metres_per_second) const {
        return detail::call(OWE_VELOCITY, e, metres_per_second).error;
    }
    Error destroy(Entity e) const { return detail::call(OWE_DESTROY, e).error; }
    Error follow(Entity e) const { return detail::call(OWE_FOLLOW, e).error; }
    Error step(float dt) const { return detail::call(OWE_STEP, {}, {dt, 0, 0}).error; }
};
struct Input {
    uint32_t held, presses, releases;
    bool down(Key k) const { return held & static_cast<uint32_t>(k); }
    bool pressed(Key k) const { return presses & static_cast<uint32_t>(k); }
    bool released(Key k) const { return releases & static_cast<uint32_t>(k); }
};
struct Steps {
    uint32_t count;
    struct Iterator {
        uint32_t i;
        float operator*() const { return 1.f / 60.f; }
        Iterator &operator++() {
            ++i;
            return *this;
        }
        bool operator!=(Iterator other) const { return i != other.i; }
    };
    Iterator begin() const { return {0}; }
    Iterator end() const { return {count}; }
};
struct Frame {
    Input keys;
    Steps steps;
    bool closed;
    const Input &input() const { return keys; }
    Steps fixed_steps() const { return steps; }
    void present() const { detail::call(OWE_PRESENT); }
};
struct Maps {
    struct Load {
        const char *name;
        Error status{};
        bool await_ready() const noexcept { return false; }
        bool await_suspend(std::coroutine_handle<>) noexcept {
            status = detail::call(OWE_LOAD, {}, {}, name).error;
            return status == Error::Ok;
        }
        Error await_resume() const {
            return status == Error::Ok ? detail::call(OWE_LOAD_STATUS).error : status;
        }
    };
    Load load(const char *name) const { return {name}; }
};
// See “Skydome textures and shaders” in docs/client-apis.md.
struct Skydome {
    // Select a packaged asset by name. An empty name restores the engine default.
    Error set_texture(const char *name) const {
        return detail::call(OWE_SKYDOME_TEXTURE, {}, {}, name).error;
    }
    Error set_shader(const char *name) const {
        return detail::call(OWE_SKYDOME_SHADER, {}, {}, name).error;
    }
};
struct Game {
    Error retro_lens(float strength) const {
        return detail::call(OWE_RETRO_LENS, {}, {strength, 0, 0}).error;
    }
    World world;
    Maps maps;
    Skydome skydome;
    Frame frame{};
    struct Next : std::suspend_always {
        Frame *frame;
        Frame *await_resume() const { return frame->closed ? nullptr : frame; }
    };
    Next next_frame() { return {{}, &frame}; }
    Error hud_message(const char *text, owe_message_location_v1 location = OWE_HUD_CORNER) {
        return detail::call(OWE_MESSAGE, {static_cast<uint32_t>(location)}, {}, text).error;
    }
    Error message(const char *text) { return detail::call(OWE_MESSAGE, {}, {}, text).error; }
};

struct Task {
    struct promise_type {
        Error result = Error::Ok;
        Task get_return_object() {
            return {std::coroutine_handle<promise_type>::from_promise(*this)};
        }
        std::suspend_always initial_suspend() noexcept { return {}; }
        std::suspend_always final_suspend() noexcept { return {}; }
        void return_value(Error e) noexcept { result = e; }
        void unhandled_exception() noexcept { result = Error::Invalid; }
#ifdef __wasm__
        // One root task per Wasm instance. Local variables live here across frames.
        static void *operator new(decltype(sizeof(0)) bytes) noexcept {
            alignas(16) static unsigned char arena[65536];
            return bytes <= sizeof arena ? arena : nullptr;
        }
        static void operator delete(void *) noexcept {}
        static Task get_return_object_on_allocation_failure() noexcept { return {}; }
#endif
    };
    std::coroutine_handle<promise_type> handle{};
};

template <Task (*Play)(Game &)>
int32_t resume(uint32_t down, uint32_t pressed, uint32_t released, uint32_t steps,
               uint32_t closed) {
    static Game game{};
    static Task task = Play(game);
    if (!task.handle.address())
        return -static_cast<int32_t>(Error::Invalid);
    game.frame = {{down, pressed, released}, {steps}, closed != 0};
    task.handle.resume();
    if (!task.handle.done() && !closed)
        return 0;
    auto status = task.handle.promise().result;
    task.handle.destroy();
    task.handle = {};
    return status == Error::Ok || status == Error::Closed ? 1 : -static_cast<int32_t>(status);
}
} // namespace owe

#ifdef __wasm__
#define OWE_GAME(play)                                                                             \
    extern "C" int32_t owe_client_resume_v1(uint32_t h, uint32_t p, uint32_t r, uint32_t n,        \
                                            uint32_t c) {                                          \
        return owe::resume<play>(h, p, r, n, c);                                                   \
    }
#else
#define OWE_GAME(play)                                                                             \
    int main(int argc, char **argv) {                                                              \
        return argc > 1 ? owe_run_v1(argv[1], owe::resume<play>) : int(owe::Error::Invalid);               \
    }
#endif
