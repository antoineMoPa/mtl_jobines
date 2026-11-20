#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>

#include "content.hpp"

using namespace owe;
namespace {
struct Weapon {
  const char *model;
  int capacity, loaded, reserve, damage;
  float interval;
};
struct Panel {
  Entity entity;
  int health = 3;
};
Task play(Game &game) {
  for (;;) {
    auto status = co_await game.maps.load("bay");
    if (status != Error::Ok)
      co_return status;
    const auto actor = game.world.find("player");
    if (!actor)
      co_return actor.error;
    std::array<Panel, 3> panels{};
    const char *names[] = {"left", "middle", "right"};
    for (int i = 0; i < 3; ++i) {
      const auto panel = game.world.find(names[i]);
      if (!panel)
        co_return panel.error;
      panels[i].entity = panel.value;
    }
    Weapon guns[] = {{"rifle", 12, 12, 36, 2, .17f},
                     {"handgun", 7, 7, 21, 1, .22f}};
    int selected = 0;
    const auto spawn = game.world.position(actor.value);
    if (!spawn)
      co_return spawn.error;
    Vec3 position = spawn.value;
    float yaw = 0, pitch = 0, cooldown = 0, reload = 0;
    while (auto frame = co_await game.next_frame()) {
      const auto &input = frame->input();
      if (input.pressed(Key::Escape))
        co_return Error::Ok;
      if (input.pressed(Key::E))
        break;
      const int slot = input.pressed(Key::One)   ? 0
                       : input.pressed(Key::Two) ? 1
                                                 : selected;
      if (selected != slot) {
        selected = slot;
        reload = 0;
      }
      auto &gun = guns[selected];
      bool aim = input.down(Key::Aim);
      auto mouse = game.world.take_look_delta();
      float sensitivity = aim ? .0013f : .0025f;
      yaw -= mouse.x * sensitivity;
      pitch -= mouse.y * sensitivity;
      if (input.pressed(Key::Reload) && !reload && gun.loaded < gun.capacity &&
          gun.reserve)
        reload = .85f;
      for (float dt : frame->fixed_steps()) {
        yaw += (input.down(Key::Left) - input.down(Key::Right)) * dt * 1.5f;
        pitch += (input.down(Key::Up) - input.down(Key::Down)) * dt;
        const float ahead = input.down(Key::W) - input.down(Key::S);
        const float left = input.down(Key::A) - input.down(Key::D);
        const float step = dt * 3.f / std::max(1.f, std::hypot(ahead, left));
        position.x = std::clamp(
            position.x + step * (ahead * std::cos(yaw) - left * std::sin(yaw)),
            -6.f, 5.f);
        position.y = std::clamp(
            position.y + step * (ahead * std::sin(yaw) + left * std::cos(yaw)),
            -7.f, 7.f);
        cooldown = std::max(0.f, cooldown - dt);
        if (reload > 0 && (reload = std::max(0.f, reload - dt)) == 0) {
          int count = std::min(gun.capacity - gun.loaded, gun.reserve);
          gun.loaded += count;
          gun.reserve -= count;
        }
        game.world.step(dt);
      }
      pitch = std::clamp(pitch, -1.3f, 1.3f);
      game.world.set_position(actor.value, position);
      game.world.first_person(actor.value, yaw, pitch, 1.68f);
      const bool fired = input.pressed(Key::Fire) && !cooldown && !reload && gun.loaded;
      if (fired) {
        --gun.loaded;
        cooldown = gun.interval;
        auto hit = game.world.strike_reticle(60, selected ? .73f : 1.f);
        if (hit) {
          for (auto &panel : panels) {
            if (hit.value.entity.id != panel.entity.id)
              continue;
            panel.health -= gun.damage;
            if (panel.health <= 0) {
              const auto result = game.world.shatter_panel(hit.value);
              if (result != Error::Ok)
                co_return result;
            }
            break;
          }
        } else if (hit.error != Error::NotFound)
          co_return hit.error;
      }
      const float kick = cooldown / gun.interval;
      game.world.view_model(gun.model,
                            {(selected ? .56f : .83f) - .045f * kick,
                             aim ? 0.f : -.23f, aim ? -.095f : -.22f});
      if (fired) {
        const auto result = game.world.muzzle_flash(selected ? MuzzleFlash::Handgun : MuzzleFlash::Rifle);
        if (result != Error::Ok)
          co_return result;
      }
      char ammo[48];
      std::snprintf(ammo, sizeof(ammo), "%d / %d%s", gun.loaded, gun.reserve,
                    reload ? "  ..." : "");
      game.hud_message(ammo);
      frame->present();
    }
    // A closed window also resumes a suspended task.
    if (game.frame.closed)
      co_return Error::Ok;
  }
}
} // namespace
OWE_CLIENT(define_game, play)
