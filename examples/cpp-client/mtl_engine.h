#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Called on the engine thread. away is 1 during an unfinished mission when the
// player is outside the assigned vehicle. Negative previous means no timer.
// Return negative to clear, positive seconds to display, or zero to expire.
typedef float (*mtl_vehicle_return_fn)(uint32_t away, float previous, float elapsed);

// Call once on the main thread. The engine creates the window and runs until exit.
int32_t mtl_run_v1(mtl_vehicle_return_fn rule);

#ifdef __cplusplus
}
#endif
