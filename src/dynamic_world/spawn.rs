//! Explicit OpenCity coordinates shared by CLI launches and reset.
use super::{Vehicle, METERS_PER_UNIT};
use glam::{Vec2, Vec3};
use opencity::runtime::City;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnPoint {
    pub point: [f32; 2],
    /// Radians counterclockwise from east. Missing heading follows the nearest road.
    pub heading: Option<f32>,
}
impl SpawnPoint {
    /// Authored default shared by native and browser maps; CLI coordinates override it.
    pub(crate) fn from_program(program: &opencity::Program) -> Result<Option<Self>, String> {
        Self::from_named_program(program, "player_start")
    }
    pub(crate) fn walking_from_program(
        program: &opencity::Program,
    ) -> Result<Option<Self>, String> {
        Self::from_named_program(program, "pedestrian_start")
    }
    fn from_named_program(program: &opencity::Program, name: &str) -> Result<Option<Self>, String> {
        let mut result = None;
        for statement in &program.statements {
            let opencity::Statement::Call(call) = statement else {
                continue;
            };
            if call.name != name {
                continue;
            }
            if result.is_some() {
                return Err(format!("Only one {name} is allowed"));
            }
            let values = call
                .args
                .iter()
                .map(|argument| match argument {
                    opencity::Argument::Number(value) => Ok(value.as_str()),
                    _ => Err(format!("{name} expects x,y[,heading-degrees]")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            result = Some(values.join(",").parse()?);
        }
        Ok(result)
    }
}
impl FromStr for SpawnPoint {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        let values = value
            .split(',')
            .map(str::parse::<f32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| {
                "--start expects x,y[,heading-degrees] in OpenCity coordinates".to_string()
            })?;
        if !(2..=3).contains(&values.len()) || values.iter().any(|v| !v.is_finite()) {
            return Err("--start expects finite x,y[,heading-degrees]".into());
        }
        Ok(Self {
            point: [values[0], values[1]],
            heading: values.get(2).map(|v| v.to_radians()),
        })
    }
}
impl Vehicle {
    pub fn spawn_at(city: &City, start: SpawnPoint) -> Result<Self, String> {
        let support = city
            .surface_at(start.point, f32::INFINITY)
            .ok_or_else(|| "Selected start has no ground or road support".to_string())?;
        if support.kind == opencity::runtime::SurfaceKind::Water {
            return Err("Selected start is in water".into());
        }
        if city.building_collision(
            start.point,
            0.022,
            support.height + 0.0005,
            support.height + 0.016,
        ) {
            return Err("Selected start overlaps a building".into());
        }
        let yaw = start.heading.unwrap_or_else(|| {
            city.roads
                .iter()
                .filter(|road| {
                    road.definition
                        .elements
                        .iter()
                        .any(|element| element.kind == opencity::RoadElementKind::CarLane)
                })
                .flat_map(|road| (0..=40).map(move |i| (road, i as f32 / 40.)))
                .min_by(|(a, ta), (b, tb)| {
                    let distance = |road: &opencity::runtime::Road, t| {
                        let p = road.segment.point_at(t);
                        (p.x - start.point[0]).powi(2) + (p.y - start.point[1]).powi(2)
                    };
                    distance(a, *ta).total_cmp(&distance(b, *tb))
                })
                .map_or(0., |(road, t)| {
                    let tangent = road.segment.tangent_at(t);
                    let lane = road
                        .definition
                        .elements
                        .iter()
                        .position(|e| e.kind == opencity::RoadElementKind::CarLane)
                        .unwrap();
                    tangent.y.atan2(tangent.x)
                        + if road.definition.lane_direction(lane)
                            == Some(opencity::RoadDirection::Reverse)
                        {
                            std::f32::consts::PI
                        } else {
                            0.
                        }
                })
        });
        let forward = Vec2::new(yaw.cos(), yaw.sin());
        let left = Vec2::new(-forward.y, forward.x);
        let gradient = Vec2::from_array(support.gradient);
        Ok(Self {
            definition: None,
            sound_controls: super::Controls::default(),
            sound_running: false,
            route_signal: crate::hermes::RouteSignal::Off,
            signal_time: 0.,
            sound_impacts: [crate::audio::Impact::default(); 3],
            sound_impact_count: 0,
            sound_cooldown: 0.,
            position: Vec3::new(
                start.point[0] * METERS_PER_UNIT,
                start.point[1] * METERS_PER_UNIT,
                support.height * METERS_PER_UNIT + 0.65,
            ),
            velocity: Vec3::ZERO,
            yaw,
            pitch: gradient.dot(forward).atan(),
            roll: gradient.dot(left).atan(),
            steering: 0.,
            wheel_angle: 0.,
            metro_doors: 0.,
            pedal_angle: 0.,
            riding_stand: 0.,
            surface: support.kind,
            grounded: true,
            collisions: 0,
            angular_momentum: Vec3::ZERO,
            body_lean: Vec2::ZERO,
            body_lean_rate: Vec2::ZERO,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_start_is_optional_and_rejects_invalid_or_duplicate_metadata() {
        let read = |s| SpawnPoint::from_program(&opencity::parse(s).unwrap());
        assert_eq!(read("").unwrap(), None);
        assert_eq!(
            read("player_start(2,3,90);").unwrap(),
            Some(SpawnPoint {
                point: [2., 3.],
                heading: Some(std::f32::consts::FRAC_PI_2),
            })
        );
        assert_eq!(
            SpawnPoint::walking_from_program(
                &opencity::parse("pedestrian_start(2,3,90);").unwrap()
            )
            .unwrap(),
            Some(SpawnPoint {
                point: [2., 3.],
                heading: Some(std::f32::consts::FRAC_PI_2),
            })
        );
        for invalid in [
            "player_start(2);",
            "player_start(a,2);",
            "player_start(1,2); player_start(3,4);",
        ] {
            assert!(read(invalid).is_err());
        }
    }
    #[test]
    fn explicit_coordinates_and_heading_are_checked() {
        assert_eq!(
            "-24,3,90".parse::<SpawnPoint>().unwrap(),
            SpawnPoint {
                point: [-24., 3.],
                heading: Some(std::f32::consts::FRAC_PI_2)
            }
        );
        for invalid in ["1", "1,2,3,4", "NaN,2", "1,inf", "a,2"] {
            assert!(invalid.parse::<SpawnPoint>().is_err());
        }
    }
    fn city(source: &str) -> City {
        City::from_program(&opencity::parse(source).unwrap(), None).unwrap()
    }
    #[test]
    fn start_uses_exact_coordinates_support_grade_and_reverse_lane() {
        let city = city(
            "road street: carlane; direction reverse; end low=street(-1,0,1,\
                0); grade(low,0.1,0.3);",
        );
        let car = Vehicle::spawn_at(&city, "0,0".parse().unwrap()).unwrap();
        assert!(car.position.x.abs() < 1e-6 && car.position.y.abs() < 1e-6);
        assert!((car.position.z - 20.65).abs() < 0.001);
        assert!((car.yaw - std::f32::consts::PI).abs() < 0.001);
        assert!((car.pitch + 0.1f32.atan()).abs() < 0.001);
        assert!(car.grounded && car.velocity == Vec3::ZERO);
        let explicit = Vehicle::spawn_at(&city, "0,0,90".parse().unwrap()).unwrap();
        assert!((explicit.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(Vehicle::spawn_at(&city, "20,20".parse().unwrap()).is_err());
    }
    #[test]
    fn invalid_building_and_water_starts_are_rejected() {
        let building = city(
            "road street: carlane; end street(-1,0,1,0); building(0.3,-0.2,\
                -0.2,0.2,-0.2,0.2,0.2,-0.2,0.2);",
        );
        assert!(Vehicle::spawn_at(&building, "0,0".parse().unwrap())
            .err()
            .unwrap()
            .contains("building"));
        let water = city("lake(-1,-1,1,-1,1,1,-1,1);");
        assert!(Vehicle::spawn_at(&water, "0,0".parse().unwrap())
            .err()
            .unwrap()
            .contains("water"));
    }
}
