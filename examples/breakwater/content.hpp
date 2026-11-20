#pragma once
#include <owe/content.hpp>

namespace {
constexpr unsigned char road_data[] = {
#embed "range.road"
};
constexpr unsigned char rifle_model[] = {
#embed "../../assets/fps/fps-rifle.glb"
};
constexpr unsigned char handgun_model[] = {
#embed "../../assets/fps/fps-handgun.glb"
};

void define_game(owe::Content &game) {
    game.title("Breakwater");
    game.map("bay", road_data);
    game.view_model("rifle", rifle_model, 0.92f);
    game.view_model("handgun", handgun_model, 0.24f);
    game.box("panel", {-0.025f, -0.68f, 1}, {0.025f, 0.68f, 2.36f}, {0.7f, 0.22f, 0.07f});
    game.box("panel", {-0.027f, -0.39f, 1.29f}, {-0.025f, 0.39f, 2.07f}, {0.86f, 0.75f, 0.56f});
    game.box("panel", {-0.029f, -0.18f, 1.5f}, {-0.027f, 0.18f, 1.86f}, {0.24f, 0.13f, 0.08f});
    game.box("post", {-0.08f, -0.1f, 0}, {0.08f, 0.1f, 1}, {0.13f, 0.16f, 0.15f});
    game.box("barrier", {-0.3f, -1.2f, 0}, {0.3f, 1.2f, 1.95f}, {0.34f, 0.37f, 0.34f});
    game.box("backstop", {0, -12, 0}, {0.6f, 12, 4}, {0.26f, 0.32f, 0.3f});
    game.box("player-body", {-0.05f, -0.07f, -0.06f}, {0.05f, 0.07f, 0.06f}, {0.69f, 0.36f, 0.19f});
    game.impact_material("panel", owe::ImpactMaterial::Wood);
    game.impact_material("post", owe::ImpactMaterial::Wood);
    game.impact_material("barrier", owe::ImpactMaterial::Concrete);
    game.impact_material("backstop", owe::ImpactMaterial::Concrete);
    constexpr float yard_z = -2.f; // The land surface of range.road, in metres.
    game.place("bay", "left-post", "post", {13, 3, yard_z}, true);
    game.place("bay", "middle-post", "post", {17, 0, yard_z}, true);
    game.place("bay", "right-post", "post", {11, -3, yard_z}, true);
    game.place("bay", "player", "player-body", {0, 0, yard_z});
    game.place("bay", "left", "panel", {13, 3, yard_z});
    game.place("bay", "middle", "panel", {17, 0, yard_z});
    game.place("bay", "right", "panel", {11, -3, yard_z});
    game.place("bay", "wall", "backstop", {20, 0, yard_z}, true);
    game.place("bay", "cover", "barrier", {6, -3, yard_z}, true);
}
} // namespace
