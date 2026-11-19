#include "mtl_engine.h"

extern "C" float mtl_vehicle_return_v1(uint32_t away, float previous, float elapsed) {
    if (!away) {
        return -1.0f;
    }
    const float remaining = (previous < 0.0f ? 10.0f : previous) - elapsed;
    return remaining > 0.0f ? remaining : 0.0f;
}

#ifndef __wasm__
int main() {
    return mtl_run_v1(mtl_vehicle_return_v1);
}
#endif
