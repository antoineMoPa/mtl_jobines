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

// OWE_MESSAGE uses the packet id to choose where the message appears.
typedef enum { OWE_STATUS = 0, OWE_HUD_CORNER, OWE_HUD_CENTER } owe_message_location_v1;

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
    OWE_STEP,
    OWE_SET_FACING,
    OWE_RETRO_LENS,
    OWE_FIRST_PERSON,
    OWE_VIEW_MODEL,
    OWE_LOOK_DELTA,
    OWE_PICK_RETICLE,
    OWE_ROAD_LOOP = 19,
    OWE_STRIKE_RETICLE = 20,
    OWE_SHATTER_PANEL = 21,
    OWE_MUZZLE_FLASH = 22,
    OWE_SKYDOME_TEXTURE = 32,
    OWE_SKYDOME_SHADER = 33
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
