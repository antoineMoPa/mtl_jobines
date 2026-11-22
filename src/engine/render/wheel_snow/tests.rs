use super::*;
use crate::{
    dynamic_world::{catalog, Controls, WalkControls},
    engine::{scenery::Scenery, vehicle_scenery::VehicleScenery},
    hermes::Hermes,
    season::Season,
};

#[test]
fn wrapped_draw_keeps_each_live_slot_once() {
    assert_eq!(ring_ranges(4094, 4098), [4094..4096, 0..2]);
    assert_eq!(ring_ranges(4096, 4096), [0..0, 0..0]);
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_driving_leaves_tracks_and_short_lived_powder() {
    drive(
        Hermes::test_map_from_source(include_str!("../../../../tests/fixtures/local_lights.road")),
        None,
        "fixture",
    );
}

#[test]
#[ignore = "requires a native GPU adapter and Montreal map assets"]
fn native_montreal_winter_drive() {
    let map = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
    let city = Hermes::load_with_game_data(&map, None).unwrap();
    // A residential block on district_street_x_0_1.
    drive(
        city,
        Some(crate::dynamic_world::SpawnPoint {
            point: [11.013, -2.83],
            heading: Some(-std::f32::consts::FRAC_PI_2),
        }),
        "montreal",
    );
}

fn drive(mut city: Hermes, start: Option<crate::dynamic_world::SpawnPoint>, name: &str) {
    let mut renderer = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
    let mut scenery = Scenery::new(&renderer, &mut city);
    let mut world = DynamicWorld::with_start(&city, start).unwrap();
    renderer.season = Season::Winter;
    world.set_season(&city, Season::Winter);
    let suv = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-12")
        .unwrap();
    assert!(world.switch_vehicle(suv));
    let mut cars = VehicleScenery::new(&renderer);
    renderer.winter_weather = false;
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut time = 0.;
    let mut front_track = false;
    let mut rear_spray = false;
    let mut slip = 0_f32;
    for frame in 0..180 {
        let turning = (114..161).contains(&frame);
        world.step(
            &city,
            Controls {
                throttle: 1.,
                steering: if turning { 0.78 } else { 0. },
                handbrake: turning,
                ..Default::default()
            },
            WalkControls::default(),
            1. / 60.,
        );
        time += 1. / 60.;
        let p = world.player_position() / METERS_PER_UNIT;
        scenery.update_winter(&mut renderer, &world, city.city(), p, time);
        if turning {
            let player = world.player();
            slip = slip.max((player.orientation() * Vec3::Y).dot(player.velocity).abs());
            for wheel in world
                .snow_wheels(p * METERS_PER_UNIT)
                .filter(|w| w.key.0 == 0)
            {
                let contact = wheel.point / METERS_PER_UNIT;
                if renderer
                    .wheel_snow
                    .as_ref()
                    .unwrap()
                    .stamps
                    .iter()
                    .any(|s| {
                        let end = Vec3::from_slice(&s.to_width);
                        end.truncate().distance_squared(contact.truncate()) < 1e-10
                            && ((s.outward_seed[3] >= 0.) == wheel.sprays)
                    })
                {
                    if wheel.key.1 >= 2 && !wheel.sprays {
                        front_track = true;
                    }
                    if wheel.key.1 < 2 && wheel.sprays {
                        rear_spray = true;
                    }
                }
            }
        }
    }
    assert!(slip>0.15 && front_track && rear_spray,"sliding turn must lay impressions on both axles: lateral speed {slip}, front {front_track}, rear {rear_spray}");
    let channel = renderer.wheel_snow.as_ref().unwrap();
    assert!(
        channel.end > channel.powder && channel.end >= 4,
        "travel must emit tracks and fresh powder"
    );

    cars.update(&renderer, &world);
    let p = world.player_position() / METERS_PER_UNIT;
    let forward = Vec3::new(world.player().yaw.cos(), world.player().yaw.sin(), 0.);
    let eye = p - forward * 0.089 + Vec3::Z * 0.035 + forward.cross(Vec3::Z) * 0.025;
    let mut meshes = scenery.visible([p.x, p.y]);
    let winter_mesh_count = meshes.len();
    meshes.extend(cars.visible());
    let shot = |renderer: &Renderer, t| {
        renderer
            .report_screenshot(
                &meshes,
                eye,
                p + forward * 0.028,
                t,
                city.minimap_view([p.x, p.y], 0.),
            )
            .unwrap()
    };
    let wake = shot(&renderer, time);

    // Keep lighting and camera identical for each pair of reference renders.
    let effects = renderer.wheel_snow.as_mut().unwrap();
    let powder = effects.powder;
    effects.powder = effects.end;
    assert!(
        wake != shot(&renderer, time),
        "fresh spray must change visible pixels"
    );
    renderer.wheel_snow.as_mut().unwrap().powder = powder;
    let settled = shot(&renderer, time + 0.8);
    let effects = renderer.wheel_snow.as_mut().unwrap();
    effects.powder = effects.end;
    assert!(
        settled == shot(&renderer, time + 0.8),
        "powder persisted after 0.75 seconds"
    );
    let mut effects = renderer.wheel_snow.take().unwrap();
    assert!(
        settled != shot(&renderer, time + 0.8),
        "compacted tracks must outlast powder"
    );
    effects.powder = powder;
    renderer.wheel_snow = Some(effects);

    if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (effect, png) in [("driving-powder", wake), ("settled-tracks", settled)] {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{name}-{effect}.png")),
                png,
            )
            .unwrap();
        }
    }
    if start.is_some() {
        if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
            let mut props = crate::engine::prop_scenery::PropScenery::default();
            props.update(&renderer, &world);
            let mut gameplay_meshes = meshes.clone();
            gameplay_meshes.extend(props.visible());
            renderer.winter_weather = true;
            let png = renderer
                .report_screenshot(
                    &gameplay_meshes,
                    eye,
                    p + forward * 0.028,
                    time,
                    city.minimap_view([p.x, p.y], 0.),
                )
                .unwrap();
            std::fs::write(
                std::path::Path::new(&dir).join("montreal-winter-drive.png"),
                png,
            )
            .unwrap();
            renderer.winter_weather = false;
        }
    }
    if start.is_none() {
        use crate::dynamic_world::snow::Tracks;
        // The flat test avenue is two hundred metres long, with a deck at zero.
        let mut cleared = Tracks::default();
        for x in -205..205 {
            for y in -30..30 {
                cleared.clear_height((x, y), 0.);
            }
        }
        renderer.snow.update(&renderer.queue, [p.x, p.y], &cleared);
        let check_hidden = |r: &mut Renderer| {
            let with = shot(r, time);
            let effects = r.wheel_snow.take();
            assert!(
                with == shot(r, time),
                "bare or sheltered pavement showed winter wheel effects"
            );
            r.wheel_snow = effects;
        };
        check_hidden(&mut renderer);
        let mut roof = opencity::runtime::Mesh::default();
        roof.quad(
            [
                [-2., -1., 0.1],
                [2., -1., 0.1],
                [2., 1., 0.1],
                [-2., 1., 0.1],
            ],
            [0.3; 3],
            SurfaceKind::Roof,
        );
        renderer.snow.set_geometry(&[&roof]);
        renderer
            .snow
            .update(&renderer.queue, [p.x, p.y], &Tracks::default());
        check_hidden(&mut renderer);
    }
    scenery.update_winter(&mut renderer, &world, city.city(), p, time + 0.8);
    let channel = renderer.wheel_snow.as_ref().unwrap();
    assert_eq!(channel.powder, channel.end);
    assert!(channel.oldest < channel.end);
    renderer.season = Season::Summer;
    scenery.update_winter(&mut renderer, &world, city.city(), p, time + 1.);
    assert!(renderer.wheel_snow.is_none());
    assert!(
        winter_mesh_count > scenery.visible([p.x, p.y]).len(),
        "winter must add curb geometry"
    );
}
