//! Replay saved bug locations through the game's renderer for visual regression review.
use super::{prop_scenery::PropScenery, render::Renderer, scenery::Scenery};
use crate::{
    dynamic_world::{DynamicWorld, SpawnPoint, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec3;
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Deserialize)]
struct Report {
    map_coordinates: [f32; 3],
    camera_position: Option<[f32; 3]>,
    camera_look_at: Option<[f32; 3]>,
    nearest_intersection: Option<Intersection>,
}
#[derive(Deserialize)]
struct Intersection {
    point_ref: Option<String>,
    map_coordinates: [f32; 2],
}

impl Report {
    /// A named junction anchors reports recorded before the city's quarter turn.
    /// Replays of custom maps and current reports keep their recorded coordinates.
    fn align_to_junction(&mut self, city: &opencity::runtime::City) {
        let Some(anchor) = &mut self.nearest_intersection else {
            return;
        };
        let Some(junction) = city
            .junctions
            .iter()
            .find(|j| Some(&j.point_ref) == anchor.point_ref.as_ref())
        else {
            return;
        };
        let [x, y] = anchor.map_coordinates;
        let actual = glam::Vec2::new(junction.position.x, junction.position.y);
        let rotated = glam::Vec2::new(y, -x);
        if actual.distance(rotated) > 0.0001 || actual.distance(glam::Vec2::new(x, y)) <= 0.0001 {
            return;
        }
        anchor.map_coordinates = rotated.to_array();
        let turn = |[x, y, z]: [f32; 3]| [y, -x, z];
        self.map_coordinates = turn(self.map_coordinates);
        self.camera_position = self.camera_position.map(turn);
        self.camera_look_at = self.camera_look_at.map(turn);
    }
}

pub(super) fn run(
    hermes: &mut Hermes,
    reports: &Path,
    output: &Path,
    landmark_id: Option<&str>,
) -> Result<(), String> {
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800))?;
    renderer.sky_settings.solar_hour = Some(12.);
    let mut scenery = Scenery::new(&renderer, hermes);
    let mut folders: Vec<_> = fs::read_dir(reports)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("report.json").is_file())
        .collect();
    folders.sort();
    for folder in folders {
        let mut report: Report = serde_json::from_slice(
            &fs::read(folder.join("report.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        report.align_to_junction(hermes.city());
        let id = folder.file_name().unwrap().to_string_lossy();
        let recorded_heading = match (report.camera_position, report.camera_look_at) {
            (Some(eye), Some(look)) => Some((look[1] - eye[1]).atan2(look[0] - eye[0])),
            _ => report.nearest_intersection.as_ref().and_then(|j| {
                let dx = j.map_coordinates[0] - report.map_coordinates[0];
                let dy = j.map_coordinates[1] - report.map_coordinates[1];
                (dx.hypot(dy) < 0.5).then(|| dy.atan2(dx))
            }),
        };
        let world = DynamicWorld::with_start(
            hermes,
            Some(SpawnPoint {
                point: [report.map_coordinates[0], report.map_coordinates[1]],
                heading: recorded_heading,
            }),
        )?;
        let p = world.player_position() / METERS_PER_UNIT;
        let heading = world.player_yaw();
        let f = Vec3::new(heading.cos(), heading.sin(), 0.);
        let eye = report
            .camera_position
            .map(Vec3::from_array)
            .unwrap_or(p - f * 0.15 + Vec3::Z * 0.075);
        let look = report
            .camera_look_at
            .map(Vec3::from_array)
            .unwrap_or(p + f * 0.045 + Vec3::Z * 0.014);
        let eye = if report.camera_position.is_some() {
            eye
        } else {
            world.camera_eye(hermes, p + Vec3::Z * 0.014, eye)
        };
        capture(
            hermes,
            &mut renderer,
            &mut scenery,
            &world,
            eye,
            look,
            &output.join(format!("report-{id}.png")),
        )?;
        capture(
            hermes,
            &mut renderer,
            &mut scenery,
            &world,
            p - f * 0.8 + Vec3::Z * 0.9,
            p,
            &output.join(format!("report-{id}-overview.png")),
        )?;
        eprintln!(
            "Replayed report {id}: {p:?}; original camera recorded: {}",
            report.camera_position.is_some()
        );
    }
    // Optionally exercise a palette destination as well as the saved report locations.
    if let Some(id) = landmark_id {
        let mut world = DynamicWorld::with_start(hermes, None)?;
        let landmark = hermes
            .landmarks()
            .into_iter()
            .find(|l| l.id == id)
            .ok_or_else(|| format!("Unknown landmark: {id}"))?;
        world.respawn_at_landmark(hermes, &landmark)?;
        let p = world.player_position() / METERS_PER_UNIT;
        let yaw = world.player_yaw();
        let f = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        capture(
            hermes,
            &mut renderer,
            &mut scenery,
            &world,
            p - f * 0.15 + Vec3::Z * 0.075,
            p + f * 0.045 + Vec3::Z * 0.014,
            &output.join("landmark-spawn.png"),
        )?;
        let left = Vec3::new(-f.y, f.x, 0.);
        capture(
            hermes,
            &mut renderer,
            &mut scenery,
            &world,
            p - f * 0.65 + left * 0.75 + Vec3::Z * 0.9,
            Vec3::new(landmark.point[0], landmark.point[1], p.z),
            &output.join("landmark-overview.png"),
        )?;
        eprintln!("Landmark {id} respawn: {p:?}; heading {yaw}");
    }
    Ok(())
}

fn capture(
    hermes: &mut Hermes,
    renderer: &mut Renderer,
    scenery: &mut Scenery,
    world: &DynamicWorld,
    eye: Vec3,
    look: Vec3,
    path: &Path,
) -> Result<(), String> {
    let p = world.player_position() / METERS_PER_UNIT;
    let mut moving = opencity::runtime::Mesh::default();
    world.write_moving_mesh(&mut moving);
    let car = renderer.upload_dynamic(&moving);
    let mut vehicles = super::vehicle_scenery::VehicleScenery::new(renderer);
    vehicles.update(renderer, world);
    let mut props = PropScenery::default();
    props.update(renderer, world);
    renderer.update_vector_map(hermes.city())?;
    scenery.update_graffiti(renderer, [p.x, p.y]);
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.push(&car);
    meshes.extend(props.visible());
    meshes.extend(vehicles.visible());
    let png = renderer.report_screenshot(
        &meshes,
        eye,
        look,
        0.,
        hermes.minimap_view([p.x, p.y], world.player_yaw()),
    )?;
    fs::write(path, png).map_err(|e| e.to_string())?;
    eprintln!("Captured {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_camera_and_player_follow_the_same_named_junction_once() {
        let program = opencity::parse(
            "road street: carlane; end
             anchor=map_point(-2,-1); west=map_point(-3,-1); east=map_point(-1,-1);
             south=map_point(-2,-2); north=map_point(-2,0);
             street(west,-2.7,-1,-2.3,-1,anchor); street(anchor,-1.7,-1,-1.3,-1,east);
             street(south,-2,-1.7,-2,-1.3,anchor); street(anchor,-2,-0.7,-2,-0.3,north);
             intersection4(anchor,street);",
        )
        .unwrap();
        let city = opencity::runtime::City::from_program(&program, None).unwrap();
        let mut report = Report {
            map_coordinates: [1., -3., 0.1],
            camera_position: Some([0.8, -3., 0.2]),
            camera_look_at: Some([1.2, -3., 0.1]),
            nearest_intersection: Some(Intersection {
                point_ref: Some("anchor".into()),
                map_coordinates: [1., -2.],
            }),
        };
        for _ in 0..2 {
            report.align_to_junction(&city);
            assert_eq!(report.map_coordinates, [-3., -1., 0.1]);
            assert_eq!(report.camera_position, Some([-3., -0.8, 0.2]));
            assert_eq!(report.camera_look_at, Some([-3., -1.2, 0.1]));
        }
    }
}
