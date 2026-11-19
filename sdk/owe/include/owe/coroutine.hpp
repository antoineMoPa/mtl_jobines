#pragma once
// Standalone wasm32-unknown-unknown Clang has coroutine builtins and no C++ sysroot.
// Supply its required compiler protocol for our single task. Hosted builds use libc++.
#if !defined(__wasm__) || __has_include(<coroutine>)
#include <coroutine>
#else
namespace std {
template<class R, class... Args> struct coroutine_traits { using promise_type = typename R::promise_type; };
template<class Promise = void> struct coroutine_handle;
template<> struct coroutine_handle<void> {
    void* ptr;
    static coroutine_handle from_address(void* p) { return {p}; }
    void resume() const { __builtin_coro_resume(ptr); }
    void destroy() const { __builtin_coro_destroy(ptr); }
    bool done() const { return __builtin_coro_done(ptr); }
    void* address() const { return ptr; }
};
template<class P> struct coroutine_handle : coroutine_handle<void> {
    static coroutine_handle from_address(void* p) { return {{p}}; }
    static coroutine_handle from_promise(P& p) { return from_address(__builtin_coro_promise(&p, alignof(P), true)); }
    P& promise() const { return *static_cast<P*>(__builtin_coro_promise(this->ptr, alignof(P), false)); }
};
struct suspend_always {
    bool await_ready() const noexcept { return false; }
    void await_suspend(coroutine_handle<>) const noexcept {}
    void await_resume() const noexcept {}
};
}
#endif
