#include "content.hpp"

using owe::Error;
using owe::Key;
struct Point {
    float x, y;
};
constexpr int laps = 3;
struct Racer {
    const char *name;
    float distance, lane, pace;
    owe::Entity entity{};
};

float clamp(float v, float lo, float hi) { return v < lo ? lo : v > hi ? hi : v; }
struct TrackPose {
    owe::Vec3 position;
    Point forward;
    float bend;
};
TrackPose pose(owe::World &world, float distance, float lane) {
    auto at = [&](float offset) { return world.road_loop("raceway", distance + offset).value; };
    auto a = at(0), b = at(1), behind = at(-4), ahead = at(4);
    float dx = b.x - a.x, dy = b.y - a.y;
    float bend = ((a.x - behind.x) * (ahead.y - a.y) - (a.y - behind.y) * (ahead.x - a.x)) / 64.f;
    return {{a.x - lane * dy, a.y + lane * dx, .12f}, {dx,dy}, bend};
}

struct Heading {
    double x, y;
    void turn(float radians) {
        // Rotate by the half-angle formula. At a fixed step tan(angle/2) ≈ angle/2.
        double half = radians * 0.5;
        double c = (1 - half * half) / (1 + half * half);
        double s = 2 * half / (1 + half * half);
        double next_x = c * x - s * y;
        y = s * x + c * y;
        x = next_x;
    }
};

owe::Task play(owe::Game &game) {
    auto lens = game.retro_lens(1.f);
    if (lens != Error::Ok)
        co_return lens;
    for (;;) {
        auto err = co_await game.maps.load("circuit");
        if (err != Error::Ok)
            co_return err;
        auto road = game.world.road_loop("raceway", 0);
        if (!road) co_return road.error;
        const float lap_length = road.value.lap_length;
        const float finish = laps * lap_length;
        Racer cars[] = {{"player", 0, 0, 0},
                        {"rival_a", 4, 2.8f, 17},
                        {"rival_b", 8, -2.8f, 18},
                        {"rival_c", 12, 0, 19}};
        auto &player = cars[0];
        for (auto &car : cars) {
            auto entity = game.world.find(car.name);
            if (!entity)
                co_return entity.error;
            car.entity = entity.value;
            auto grid = pose(game.world, car.distance, car.lane);
            err = game.world.set_facing(car.entity, grid.forward.x, grid.forward.y);
            if (err != Error::Ok)
                co_return err;
            err = game.world.set_position(car.entity, grid.position);
            if (err != Error::Ok)
                co_return err;
        }
        err = game.world.follow(player.entity);
        if (err != Error::Ok)
            co_return err;
        bool paused = false, finished = false, replay = false;
        int last_lap = 0, last_rank = 0;
        float speed = 0;
        constexpr int ticks_per_light = 60;
        int start_ticks = 0;
        constexpr const char *lights[] = {"3", "2", "1", "GO!"};
        game.hud_message(lights[0], OWE_HUD_CENTER);
        Point grid_forward = pose(game.world, 0, 0).forward;
        Heading heading{grid_forward.x, grid_forward.y};
        while (auto frame = co_await game.next_frame()) {
            const auto &keys = frame->input();
            if (keys.pressed(Key::Escape))
                co_return Error::Ok;
            if (finished && keys.pressed(Key::E)) {
                replay = true;
                break;
            }
            if (!finished && keys.pressed(Key::P)) {
                paused = !paused;
                if (paused)
                    game.hud_message("Paused", OWE_HUD_CENTER);
                else if (start_ticks < 4 * ticks_per_light) {
                    game.hud_message(lights[start_ticks / ticks_per_light], OWE_HUD_CENTER);
                }
                last_lap = 0;
            }
            for (float dt : frame->fixed_steps()) {
                if (paused || finished)
                    break;
                if (start_ticks < 4 * ticks_per_light) {
                    ++start_ticks;
                    if (start_ticks % ticks_per_light == 0 && start_ticks < 4 * ticks_per_light)
                        game.hud_message(lights[start_ticks / ticks_per_light], OWE_HUD_CENTER);
                    if (start_ticks < 3 * ticks_per_light)
                        continue;
                }
                TrackPose track = pose(game.world, player.distance, player.lane);
                float steering = int(keys.down(Key::Left)) - int(keys.down(Key::Right));
                heading.turn(steering * dt * (2.6f - 0.032f * speed));
                float acceleration = keys.down(Key::Up) ? 8.f : -4.f;
                if (keys.down(Key::Down))
                    acceleration -= 18;
                speed = clamp(speed + acceleration * dt, 0, 24);
                if (player.lane > 4.5f || player.lane < -4.5f)
                    speed = clamp(speed, 0, 12);
                for (int i = 1; i < 4; ++i) {
                    auto &rival = cars[i];
                    float gap = rival.distance - player.distance;
                    // Compare adjacent laps too, so a lapped rival still occupies its lane.
                    while (gap < -lap_length / 2)
                        gap += lap_length;
                    while (gap > lap_length / 2)
                        gap -= lap_length;
                    float side = rival.lane - player.lane;
                    float curve = pose(game.world, rival.distance, 0).bend;
                    float target = rival.pace / (1 + 2 * (curve < 0 ? -curve : curve));
                    if (gap >= 0 && gap < 2.8f && side > -2.2f && side < 2.2f)
                        speed = clamp(speed, 0, target);
                    rival.distance += target * dt;
                }
                // Project actual travel onto the road tangent and across it. Steering changes
                // world heading; taking a bend requires turning the nose into the corner.
                float along = heading.x * track.forward.x + heading.y * track.forward.y;
                float across = heading.y * track.forward.x - heading.x * track.forward.y;
                player.distance +=
                    speed * dt * along / clamp(1 - track.bend * player.lane, .55f, 1.45f);
                player.lane = clamp(player.lane + speed * dt * across, -5.7f, 5.7f);
                for (auto &car : cars) {
                    auto location = pose(game.world, car.distance, car.lane);
                    Point forward = &car == &player ? Point{float(heading.x), float(heading.y)}
                                                    : location.forward;
                    err = game.world.set_facing(car.entity, forward.x, forward.y);
                    if (err != Error::Ok)
                        co_return err;
                    err = game.world.set_position(car.entity, location.position);
                    if (err != Error::Ok)
                        co_return err;
                }
                game.world.step(dt);
                int rank = 1;
                for (int i = 1; i < 4; ++i)
                    rank += cars[i].distance > player.distance;
                if (player.distance >= finish) {
                    const char *results[] = {"", "1st", "2nd", "3rd", "4th"};
                    char message[] = "Finish: 1st";
                    for (int i = 0; i < 3; ++i)
                        message[8 + i] = results[rank][i];
                    game.hud_message(message, OWE_HUD_CENTER);
                    finished = true;
                } else {
                    int lap = 1 + static_cast<int>(clamp(player.distance, 0, finish) / lap_length);
                    if (start_ticks == 4 * ticks_per_light && (rank != last_rank || lap != last_lap)) {
                        char message[] = "Lap 1/3 | Place 4/4";
                        message[4] = char('0' + lap);
                        message[16] = char('0' + rank);
                        game.hud_message(message);
                        last_rank = rank;
                        last_lap = lap;
                    }
                }
            }
            frame->present();
        }
        if (!replay)
            co_return Error::Ok;
    }
}
OWE_CLIENT(define_game, play)
