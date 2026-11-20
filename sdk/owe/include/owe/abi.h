#pragma once
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

// Synchronous call on the client thread. Strings are NUL-terminated UTF-8.
// The bridge copies this fixed-size packet between separate Wasm memories.
typedef struct {
    uint32_t op, id;
    float xyz[3];
    char text[160];
} owe_packet_v1;

typedef enum {
    OWE_LOAD = 1,
    OWE_LOAD_STATUS,
    OWE_PRESENT,
    OWE_MESSAGE,
    OWE_FIND,
    OWE_SPAWN,
    OWE_POSITION,
    OWE_SET_POSITION,
    OWE_VELOCITY,
    OWE_DESTROY,
    OWE_FOLLOW,
    OWE_STEP
} owe_operation_v1;
int32_t owe_call_v1(owe_packet_v1 *);
typedef int32_t (*owe_resume_v1)(uint32_t held, uint32_t pressed, uint32_t released,
                                 uint32_t fixed_steps, uint32_t closing);
#ifndef __wasm__
int32_t owe_run_v1(const char *package_path, owe_resume_v1);
#endif
#ifdef __cplusplus
}
#endif
