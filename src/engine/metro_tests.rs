use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::DynamicWorld,
    hermes::{
        metro::{MetroMap, StationId, FLOOR_HEIGHT, TRACK_HEIGHT},
        Hermes,
    },
};
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_underground_metro_station_and_boarding_views() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = DynamicWorld::new(&hermes);
    assert!(world.preview_metro_station(&hermes, StationId::Acadie));
    let mut renderer = pollster::block_on(Renderer::new(None, 1400, 900)).unwrap();
    renderer.cinematic = true;
    renderer.daylight = Some(1.);
    let mut vehicles = VehicleScenery::new(&renderer);
    vehicles.update(&renderer, &world);
    let map = MetroMap::from_city(hermes.city());
    let s = &map.stations[0];
    let folder = std::path::PathBuf::from(
        std::env::var_os("METRO_CAPTURE").unwrap_or_else(|| "/tmp/metro-capture".into()),
    );
    std::fs::create_dir_all(&folder).unwrap();
    let mut ground = hermes
        .city()
        .mesh(
            opencity::runtime::Region::radius(s.center.to_array(), 0.9).unwrap(),
            0.04,
        )
        .unwrap();
    map.open_portals(&mut ground);
    let ground = renderer.upload_vertices(&ground.vertices);
    let station = map.scenery().unwrap().unwrap();
    let mut station = renderer.upload_instanced(&station.mesh.vertices);
    station.static_model = true;
    let mut transform = glam::Mat4::IDENTITY.to_cols_array_2d();
    transform[1][3] = -1.;
    renderer.update_instances(&mut station, &[transform]);
    renderer.local_lights.set(world.local_lights());
    let mut meshes = vec![&ground, &station];
    meshes.extend(vehicles.visible());
    for (name, eye, look) in [
        (
            "report-82",
            glam::Vec3::new(1.1737835, 0.8173996, 0.14275095),
            glam::Vec3::new(1.2091126, 0.9242083, 0.12300094),
        ),
        (
            "report-83",
            glam::Vec3::new(1.239702, 1.2868125, 0.018000003),
            glam::Vec3::new(1.2703384, 1.3950607, -0.001749998),
        ),
        (
            "metro-window",
            s.point(-0.06, 0., FLOOR_HEIGHT + 0.01775),
            s.point(-0.06, -0.10, FLOOR_HEIGHT + 0.01775),
        ),
        (
            "acadie-platform",
            s.point(0.15, 0.09, FLOOR_HEIGHT + 0.01875),
            s.point(-0.09, 0.0, FLOOR_HEIGHT + 0.01375),
        ),
        (
            "acadie-stairs",
            s.point(-0.16, 0.06, FLOOR_HEIGHT + 0.01675),
            s.point(-0.34, 0.06, 0.085),
        ),
        (
            "acadie-entrance",
            s.point(-0.63, -0.05, 0.15),
            s.point(-0.42, 0.055, 0.11),
        ),
        (
            "metro-open-doors",
            s.point(0.11, 0.07, FLOOR_HEIGHT + 0.01875),
            s.point(0.02, 0.0, FLOOR_HEIGHT + 0.01175),
        ),
        (
            "metro-tunnel",
            s.point(0.28, 0., TRACK_HEIGHT + 0.023),
            s.point(0.70, 0., TRACK_HEIGHT + 0.02),
        ),
        (
            "metro-concrete-rails",
            s.point(0.28, 0.05, FLOOR_HEIGHT + 0.015),
            s.point(0.50, 0., TRACK_HEIGHT),
        ),
        (
            "metro-cabin",
            s.point(-0.06, 0.0, FLOOR_HEIGHT + 0.01775),
            s.point(0.055, 0.0, FLOOR_HEIGHT + 0.01175),
        ),
    ] {
        for msaa in [false, true] {
            renderer.set_antialiasing(msaa);
            let png = renderer
                .report_screenshot(
                    &meshes,
                    eye,
                    look,
                    0.,
                    hermes.minimap_view(s.center.to_array(), 0.),
                )
                .unwrap();
            std::fs::write(
                folder.join(format!(
                    "{name}-{}.png",
                    if msaa { "msaa" } else { "single" }
                )),
                png,
            )
            .unwrap();
        }
    }
    std::fs::write(folder.join("metro-map.svg"), hermes.minimap_svg()).unwrap();
}

#[test]
#[ignore = "requires a native GPU adapter; measures complete city scenery"]
fn native_metro_spatial_batch_profile() {
    use super::{prop_scenery::PropScenery, scenery::Scenery};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
    let mut hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = DynamicWorld::new(&hermes);
    world.preview_metro_station(&hermes, StationId::Acadie);
    let mut renderer = pollster::block_on(Renderer::new(None, 1400, 900)).unwrap();
    renderer.cinematic = true;
    renderer.daylight = Some(1.);
    renderer.local_lights.set(world.local_lights());
    let mut vehicles = VehicleScenery::new(&renderer);
    vehicles.update(&renderer, &world);
    let mut props = PropScenery::default();
    props.update(&renderer, &world);
    let original: Vec<_> = hermes
        .external_models
        .iter()
        .map(|group| {
            let mut gpu = renderer.upload_instanced(&group.mesh.vertices);
            gpu.static_model = true;
            let transforms: Vec<_> = group
                .transforms
                .iter()
                .map(|matrix| {
                    let mut columns = matrix.to_cols_array_2d();
                    columns[1][3] = -1.;
                    columns
                })
                .collect();
            renderer.update_instances(&mut gpu, &transforms);
            gpu
        })
        .collect();
    let scenery = Scenery::new(&renderer, &mut hermes);
    let s = &scenery.metro.stations[0];
    let eye = glam::Vec3::new(1.239702, 1.2868125, 0.018000003);
    let look = glam::Vec3::new(1.2703384, 1.3950607, -0.001749998);
    assert!(scenery.metro.enclosed_view(eye));
    let mut selected = scenery.visible(s.center.to_array());
    selected.extend(props.visible());
    selected.extend(vehicles.visible());
    let mut baseline: Vec<_> = scenery
        .visible(s.center.to_array())
        .into_iter()
        .filter(|m| !m.static_model)
        .collect();
    baseline.extend(original.iter());
    baseline.extend(props.visible());
    baseline.extend(vehicles.visible());
    let exits: Vec<_> = scenery
        .metro
        .stations
        .iter()
        .map(|s| s.point(-0.40, 0.06, 0.).truncate())
        .collect();
    let underground: Vec<_> = selected
        .iter()
        .copied()
        .filter(|m| m.visible_underground(&exits))
        .collect();
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("metro frame benchmark"),
        size: wgpu::Extent3d {
            width: 1400,
            height: 900,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.config.format.add_srgb_suffix(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    for (name, meshes) in [
        ("monolithic", &baseline),
        ("spatial", &selected),
        ("underground", &underground),
    ] {
        renderer.underground_exits = (name == "underground").then(|| {
            scenery
                .metro
                .stations
                .iter()
                .map(|s| s.point(-0.40, 0.06, 0.).truncate())
                .collect()
        });
        let stats = renderer.visibility_stats(meshes, eye, look, 0.);
        let mut times = Vec::new();
        for frame in 0..24 {
            let start = std::time::Instant::now();
            renderer.render(
                &view,
                meshes,
                eye,
                look,
                0.,
                hermes.minimap_view(s.center.to_array(), 0.),
            );
            renderer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            if frame >= 8 {
                times.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "METRO {name}: median {:.2} ms; {stats:?}",
            times[times.len() / 2]
        );
        if let Some(folder) = std::env::var_os("METRO_CAPTURE") {
            let folder = std::path::PathBuf::from(folder);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join(format!("profile-{name}.png")),
                renderer
                    .report_screenshot(
                        meshes,
                        eye,
                        look,
                        0.,
                        hermes.minimap_view(s.center.to_array(), 0.),
                    )
                    .unwrap(),
            )
            .unwrap();
        }
    }
    let exits = renderer.underground_exits.take();
    let before = renderer.visibility_stats(&baseline, eye, look, 0.);
    renderer.underground_exits = exits;
    let after = renderer.visibility_stats(&underground, eye, look, 0.);
    assert!(after.camera_vertices < before.camera_vertices);
    assert!(after.shadow_vertices < before.shadow_vertices);
}
