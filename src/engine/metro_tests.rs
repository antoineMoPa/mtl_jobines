use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::DynamicWorld,
    hermes::{
        metro::{MetroMap, StationId, FLOOR_HEIGHT},
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
    let ground = hermes
        .city()
        .mesh(
            opencity::runtime::Region::radius(s.center.to_array(), 0.9).unwrap(),
            0.04,
        )
        .unwrap();
    let ground = renderer.upload_vertices(&ground.vertices);
    let station = map.scenery().unwrap().unwrap();
    let mut station = renderer.upload_instanced(&station.mesh.vertices);
    station.static_model = true;
    let mut transform = glam::Mat4::IDENTITY.to_cols_array_2d();
    transform[1][3] = -1.;
    renderer.update_instances(&mut station, &[transform]);
    let mut meshes = vec![&ground, &station];
    meshes.extend(vehicles.visible());
    for (name, eye, look) in [
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
            s.point(-0.53, -0.05, 0.15),
            s.point(-0.32, 0.055, 0.11),
        ),
        (
            "metro-open-doors",
            s.point(0.11, 0.07, FLOOR_HEIGHT + 0.01875),
            s.point(0.02, 0.0, FLOOR_HEIGHT + 0.01175),
        ),
        (
            "metro-cabin",
            s.point(-0.06, 0.0, FLOOR_HEIGHT + 0.01775),
            s.point(0.055, 0.0, FLOOR_HEIGHT + 0.01175),
        ),
    ] {
        let png = renderer
            .report_screenshot(
                &meshes,
                eye,
                look,
                0.,
                hermes.minimap_view(s.center.to_array(), 0.),
            )
            .unwrap();
        std::fs::write(folder.join(format!("{name}.png")), png).unwrap();
    }
    std::fs::write(folder.join("metro-map.svg"), hermes.minimap_svg()).unwrap();
}
