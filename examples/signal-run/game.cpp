#include <owe/game.hpp>

using owe::Entity;
using owe::Error;
using owe::Key;
using owe::Vec3;

struct Delivery {
    const char* vehicle;
    const char* driver;
    const char* destination;
    float speed;
    const char* briefing;
};

constexpr Delivery jobs[] = {
    {"line_cart", "lin", "east_relay", 12.f, "Lin | Line cart: drive RIGHT to the amber relay, then E to install the fuse."},
    {"survey_rover", "sol", "north_beacon", 19.f, "Sol | Survey rover: drive UP to the blue beacon, then E to calibrate it."}
};

owe::Task play(owe::Game& game) {
    auto load = co_await game.maps.load("junction");
    if (load != Error::Ok) co_return load;

    for (const Delivery& job : jobs) {
        auto vehicle = game.world.find(job.vehicle);
        auto crew = game.world.find(job.driver);
        auto landmark = game.world.find(job.destination);
        if (!vehicle) co_return vehicle.error;
        if (!crew) co_return crew.error;
        if (!landmark) co_return landmark.error;
        auto goal = game.world.position(landmark.value);
        if (!goal) co_return goal.error;
        // The marker is runtime scenery; the tower itself belongs to the map.
        auto marker = game.world.spawn("signal_lamp", {goal.value.x, goal.value.y, 5.f});
        if (!marker) co_return marker.error;
        game.world.follow(vehicle.value);
        game.message(job.briefing);
        bool paused = false;
        bool delivered = false;
        while (auto frame = co_await game.next_frame()) {
            const auto& keys = frame->input();
            if (keys.pressed(Key::Escape)) co_return Error::Ok;
            if (keys.pressed(Key::P)) {
                paused = !paused;
                game.message(paused ? "Paused | P to resume" : job.briefing);
            }
            if (!paused) {
                float x = int(keys.down(Key::Right)) - int(keys.down(Key::Left));
                float y = int(keys.down(Key::Up)) - int(keys.down(Key::Down));
                const float speed = (x && y) ? job.speed * 0.70710678f : job.speed;
                game.world.velocity(vehicle.value, {x * speed, y * speed, 0.f});
                for (float dt : frame->fixed_steps()) game.world.step(dt);
                auto here = game.world.position(vehicle.value);
                if (!here) co_return here.error;
                game.world.set_position(crew.value, {here.value.x, here.value.y, here.value.z + 1.f});
                if (keys.pressed(Key::E)) {
                    float dx = here.value.x - goal.value.x, dy = here.value.y - goal.value.y;
                    delivered = dx*dx + dy*dy < 25.f;
                    if (!delivered) game.message("Out of reach. Drive within 5 m of the tower, then press E.");
                }
            }
            frame->present();
            if (delivered) break;
            if (frame->closed) co_return Error::Ok;
        }
        if (!delivered) co_return Error::Ok;
        game.world.velocity(vehicle.value, {});
        game.world.destroy(marker.value);
    }
    game.message("Both signals restored. Shift complete. Esc to exit.");
    while (auto frame = co_await game.next_frame()) {
        if (frame->input().pressed(Key::Escape)) break;
        frame->present();
    }
    co_return Error::Ok;
}

OWE_GAME(play)
