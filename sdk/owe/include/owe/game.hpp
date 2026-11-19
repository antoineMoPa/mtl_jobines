#pragma once
#include "abi.h"
#include "coroutine.hpp"

namespace owe {
enum class Error : int32_t { Ok, NoMap, NotFound, UnknownPrefab, StaleEntity, ReadOnly, Invalid, Closed };
enum class Key : uint32_t { Left=1, Right=2, Down=4, Up=8, E=16, P=32, Escape=64, Tab=128 };
struct Vec3 { float x{}, y{}, z{}; };
struct Entity { uint32_t id{}; };
template<class T> struct Result {
    T value{};
    Error error{};
    explicit operator bool() const { return error == Error::Ok; }
};
namespace detail {
struct Reply { owe_packet_v1 data; Error error; };
inline Reply call(owe_operation_v1 op, Entity id={}, Vec3 pos={}, const char* text="") {
    owe_packet_v1 p{static_cast<uint32_t>(op),id.id,{pos.x,pos.y,pos.z},{}};
    unsigned i=0;
    while(text[i] && i+1<sizeof p.text) { p.text[i]=text[i]; ++i; }
    if(text[i]) return {p,Error::Invalid};
    const auto error=static_cast<Error>(owe_call_v1(&p));
    return {p,error};
}
}
struct World {
    Result<Entity> find(const char* name) const {
        auto r=detail::call(OWE_FIND,{},{},name); return {{r.data.id},r.error};
    }
    Result<Entity> spawn(const char* prefab, Vec3 at) const {
        auto r=detail::call(OWE_SPAWN,{},at,prefab); return {{r.data.id},r.error};
    }
    Result<Vec3> position(Entity e) const {
        auto r=detail::call(OWE_POSITION,e); return {{r.data.xyz[0],r.data.xyz[1],r.data.xyz[2]},r.error};
    }
    Error set_position(Entity e,Vec3 at) const { return detail::call(OWE_SET_POSITION,e,at).error; }
    Error velocity(Entity e,Vec3 metres_per_second) const { return detail::call(OWE_VELOCITY,e,metres_per_second).error; }
    Error destroy(Entity e) const { return detail::call(OWE_DESTROY,e).error; }
    Error follow(Entity e) const { return detail::call(OWE_FOLLOW,e).error; }
    Error step(float dt) const { return detail::call(OWE_STEP,{}, {dt,0,0}).error; }
};
struct Input {
    uint32_t held, presses, releases;
    bool down(Key k) const {return held & static_cast<uint32_t>(k);}
    bool pressed(Key k) const {return presses & static_cast<uint32_t>(k);}
    bool released(Key k) const {return releases & static_cast<uint32_t>(k);}
};
struct Steps {
    uint32_t count;
    struct Iterator {
        uint32_t i;
        float operator*() const {return 1.f/60.f;}
        Iterator& operator++(){++i;return *this;}
        bool operator!=(Iterator other) const {return i!=other.i;}
    };
    Iterator begin() const {return {0};}
    Iterator end() const {return {count};}
};
struct Frame {
    Input keys;
    Steps steps;
    bool closed;
    const Input& input() const {return keys;}
    Steps fixed_steps() const {return steps;}
    void present() const {detail::call(OWE_PRESENT);}
};
struct Maps {
    struct Load {
        const char* name; Error status{};
        bool await_ready() const noexcept {return false;}
        bool await_suspend(std::coroutine_handle<>) noexcept {
            status=detail::call(OWE_LOAD,{},{},name).error;return status==Error::Ok;
        }
        Error await_resume() const {return status==Error::Ok ? detail::call(OWE_LOAD_STATUS).error : status;}
    };
    Load load(const char* name) const {return {name};}
};
struct Game {
    World world;
    Maps maps;
    Frame frame{};
    struct Next : std::suspend_always {
        Frame* frame;
        Frame* await_resume() const {return frame->closed ? nullptr : frame;}
    };
    Next next_frame() {return {{},&frame};}
    Error message(const char* text) {return detail::call(OWE_MESSAGE,{},{},text).error;}
};

struct Task {
    struct promise_type {
        Error result=Error::Ok;
        Task get_return_object() {return {std::coroutine_handle<promise_type>::from_promise(*this)};}
        std::suspend_always initial_suspend() noexcept {return {};}
        std::suspend_always final_suspend() noexcept {return {};}
        void return_value(Error e) noexcept {result=e;}
        void unhandled_exception() noexcept {result=Error::Invalid;}
#ifdef __wasm__
        // One root task per Wasm instance. Local variables live here across frames.
        static void* operator new(decltype(sizeof(0)) bytes) noexcept {
            alignas(16) static unsigned char arena[65536];
            return bytes<=sizeof arena ? arena : nullptr;
        }
        static void operator delete(void*) noexcept {}
        static Task get_return_object_on_allocation_failure() noexcept {return {};}
#endif
    };
    std::coroutine_handle<promise_type> handle{};
};

template<Task (*Play)(Game&)>
int32_t resume(uint32_t down,uint32_t pressed,uint32_t released,uint32_t steps,uint32_t closed) {
    static Game game{};
    static Task task=Play(game);
    if(!task.handle.address()) return -static_cast<int32_t>(Error::Invalid);
    game.frame={{down,pressed,released},{steps},closed!=0};
    task.handle.resume();
    if(!task.handle.done() && !closed) return 0;
    auto status=task.handle.promise().result;
    task.handle.destroy();
    task.handle={};
    return status==Error::Ok || status==Error::Closed ? 1 : -static_cast<int32_t>(status);
}
}

#ifdef __wasm__
#define OWE_GAME(play) extern "C" int32_t owe_client_resume_v1(uint32_t h,uint32_t p,uint32_t r,uint32_t n,uint32_t c){return owe::resume<play>(h,p,r,n,c);}
#else
#define OWE_GAME(play) int main(int argc,char** argv){return owe_run_v1(argc>1 ? argv[1] : "game.pack.json",owe::resume<play>);}
#endif
