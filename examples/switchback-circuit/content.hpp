#pragma once
#include <owe/content.hpp>

namespace {
constexpr unsigned char road_data[] = {
#embed "circuit.road"
};

void define_game(owe::Content &game) {
    game.title("Switchback Circuit");
    game.map("circuit", road_data);
    auto car = [&](const char *name, owe::Vec3 paint) {
        game.box(name, {-1.85f, -0.92f, 0.1f}, {1.92f, 0.92f, 0.44f}, {0.055f, 0.06f, 0.09f});
        game.box(name, {-1.72f, -0.87f, 0.44f}, {1.85f, 0.87f, 0.82f}, paint);
        game.box(name, {-1.04f, -0.69f, 0.82f}, {0.28f, 0.69f, 1.31f}, {0.065f, 0.14f, 0.22f});
        game.box(name, {0.28f, -0.17f, 0.82f}, {1.82f, 0.17f, 0.9f}, {0.99f, 0.86f, 0.56f});
        game.box(name, {-1.66f, -1.04f, 0.89f}, {-1.39f, 1.04f, 1.11f}, paint);
        game.box(name, {1.84f, -0.78f, 0.51f}, {1.96f, -0.46f, 0.73f}, {1, 0.95f, 0.66f});
        game.box(name, {-1.83f, -0.78f, 0.48f}, {-1.72f, -0.46f, 0.72f}, {0.98f, 0.02f, 0.01f});
        game.box(name, {1.84f, 0.46f, 0.51f}, {1.96f, 0.78f, 0.73f}, {1, 0.95f, 0.66f});
        game.box(name, {-1.83f, 0.46f, 0.48f}, {-1.72f, 0.78f, 0.72f}, {0.98f, 0.02f, 0.01f});
    };
    car("comet", {1, 0.23f, 0.05f});
    car("mint", {0.08f, 0.88f, 0.55f});
    car("cobalt", {0.08f, 0.4f, 1});
    car("orchid", {0.85f, 0.15f, 0.86f});
    game.box("gantry", {-0.45f, -7, 0}, {0.45f, -6, 5}, {0.9f, 0.24f, 0.07f});
    game.box("gantry", {-0.45f, 6, 0}, {0.45f, 7, 5}, {0.9f, 0.24f, 0.07f});
    for (int lane = -7; lane < 7; ++lane) {
        const float shade = (lane + 7) % 2 ? 0.94f : 0.09f;
        game.box("gantry", {-0.45f, float(lane), 5}, {0.45f, float(lane + 1), 5.8f},
                 {shade, shade, shade});
    }
    game.place("circuit", "player", "comet", {0, -46.14f, 0.12f});
    game.place("circuit", "rival_a", "mint", {4, -44, 0.12f});
    game.place("circuit", "rival_b", "cobalt", {8, -48, 0.12f});
    game.place("circuit", "rival_c", "orchid", {12, -46, 0.12f});
    game.place("circuit", "start_line", "gantry", {0, -46, 0}, true);
}
} // namespace
