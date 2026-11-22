//! Arcade flight uses metres and seconds, with the vehicle's +X axis forward.
use super::{catalog, Controls, Vehicle, METERS_PER_UNIT};
use crate::audio::{EngineKind, VehicleSound};
use glam::Vec3;
use opencity::runtime::{City, SurfaceKind};

pub(super) const MAIN_GEAR_X: f32 = -0.055; // Fraction of fuselage length, aft of the origin.
pub(super) const NOSE_GEAR_X: f32 = 0.32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Model {
    Estuaire,
    Fleche,
}

impl Model {
    pub(super) const ALL: [Self; 2] = [Self::Estuaire, Self::Fleche];

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Estuaire => "vehicle-air-estuaire",
            Self::Fleche => "vehicle-air-fleche",
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|model| model.id() == id)
    }

    pub(super) fn door_fraction(self) -> f32 {
        match self {
            Self::Estuaire => 0.28,
            Self::Fleche => 0.20,
        }
    }

    pub(super) fn catalog_index(self) -> usize {
        catalog::definitions()
            .iter()
            .position(|definition| definition.config.research_id == self.id())
            .expect("bundled aircraft")
    }

    pub(super) fn physics(self) -> AircraftPhysics {
        match self {
            Self::Estuaire => AircraftPhysics {
                reference_speed: 54.,
                max_speed: 110.,
                thrust: 5.2,
                response_time: 0.86,
            },
            Self::Fleche => AircraftPhysics {
                reference_speed: 36.,
                max_speed: 125.,
                thrust: 7.8,
                response_time: 0.56,
            },
        }
    }

    pub(super) fn definition(self, texture_layer: u32) -> catalog::Definition {
        macro_rules! mesh {
            ($path:literal) => {
                crate::web_asset!(
                    concat!("../../data/vehicle/aircraft/", $path, "/model.glb"),
                    concat!("data/vehicle/aircraft/", $path, "/model.glb")
                )
            };
        }
        let (name, bytes, length, width, height, ride, radius, mass) = match self {
            Self::Estuaire => (
                "Estuaire Airliner",
                mesh!("estuaire-airliner"),
                34.,
                33.,
                8.5,
                3.14,
                1.89,
                42000.,
            ),
            Self::Fleche => (
                "Flèche Jet",
                mesh!("fleche-jet"),
                14.,
                13.,
                3.8,
                1.35,
                0.87,
                5400.,
            ),
        };
        let physics = self.physics();
        catalog::Definition::load_config(
            catalog::Config {
                schema_version: 1,
                name: name.into(),
                research_id: self.id().into(),
                category: "Aircraft".into(),
                road_vehicle: false,
                vehicle_kind: catalog::VehicleKind::Aircraft,
                model: "model.glb".into(),
                dimensions: catalog::Dimensions {
                    length,
                    width,
                    height,
                },
                physics: catalog::Physics {
                    mass,
                    max_speed: physics.max_speed,
                    acceleration: physics.thrust,
                    friction: 0.8,
                    max_steer_angle: 0.92,
                    height_offset: ride,
                    wheel_radius: 0.24,
                    wheelbase: length * (NOSE_GEAR_X - MAIN_GEAR_X),
                    track: radius * 1.78,
                },
                sound: VehicleSound {
                    engine: EngineKind::Electric,
                    idle_rpm: 1200.,
                    redline_rpm: 9400.,
                    engine_gain: 0.95,
                    roughness: 0.33,
                    exhaust_gain: 0.25,
                    body_gain: 0.17,
                    ..VehicleSound::default()
                },
                qa: serde_json::json!({"claydashExport": "completed"}),
            },
            bytes,
            texture_layer,
        )
        .expect("valid bundled Claydash aircraft")
    }
}

pub(super) fn gear_target(height: f32) -> f32 {
    ((height - 8.) / 17.).clamp(0., 1.)
}

pub(super) struct AircraftPhysics {
    reference_speed: f32,
    max_speed: f32,
    thrust: f32,
    response_time: f32,
}

impl AircraftPhysics {
    pub(super) fn step(&self, plane: &mut Vehicle, city: &City, input: Controls, dt: f32) {
        let command = Vec3::new(input.steering, input.aircraft_pitch, input.aircraft_yaw);
        plane.aircraft_surfaces += (command - plane.aircraft_surfaces) * (1. - (-dt * 7.).exp());
        let surfaces = plane.aircraft_surfaces;
        let throttle = input.throttle.max(0.);
        let slowing = input.brake || input.throttle < 0.;
        let speed = if plane.grounded {
            let forward = Vec3::new(plane.yaw.cos(), plane.yaw.sin(), 0.);
            let rolling = plane.velocity.dot(forward);
            let resistance = 0.45 + 0.00032 * rolling.powi(2);
            let (target, acceleration) = if input.handbrake {
                (0., 12. + resistance)
            } else if slowing && rolling > 0. {
                (0., 5. + resistance)
            } else if slowing {
                (-3., 1.4) // Powered pushback, limited to walking pace.
            } else if throttle > 0. {
                (
                    self.max_speed,
                    self.thrust * throttle - resistance * rolling.signum(),
                )
            } else {
                (0., resistance)
            };
            let change = acceleration.max(0.) * dt;
            rolling + (target - rolling).clamp(-change, change)
        } else {
            let speed = plane.velocity.truncate().length();
            let brake = if input.handbrake {
                12.
            } else if slowing {
                5.
            } else {
                0.
            };
            let flight_angle = plane.velocity.z.atan2(speed.max(1.));
            let incidence = plane.pitch - flight_angle;
            let drag = (0.00040 + 0.003 * incidence.powi(2)) * speed * speed;
            // Convert climb/descent to a speed change while the wing carries load.
            let wing_load = (speed / self.reference_speed).powi(2).min(1.);
            let climb_cost = 9.81 * flight_angle.sin() * wing_load;
            (speed + (self.thrust * throttle - drag - climb_cost - brake * 0.24) * dt)
                .clamp(0., self.max_speed)
        };
        let on_wheels = plane.grounded && plane.surface != SurfaceKind::Water;
        let steering_target = if on_wheels {
            let lock = plane
                .definition()
                .expect("aircraft definition")
                .config
                .physics
                .max_steer_angle;
            let command = (input.steering + input.aircraft_yaw).clamp(-1., 1.);
            command * lock / (1. + (speed / 12.).powi(2))
        } else {
            0.
        };
        plane.steering += (steering_target - plane.steering) * (1. - (-4. * dt).exp());
        let pressure = (speed.max(0.) / self.reference_speed).powi(2);
        let authority = pressure.clamp(0.12, 1.45);
        let omega = 2. / self.response_time;
        let attitude_blend = 1. - (-3.8 * dt).exp();
        if plane.grounded {
            // The nose gear supports the aircraft at low speed. Elevator
            // authority grows with dynamic pressure during the ground roll.
            let pitch_target = 0.18 * surfaces.y.max(0.) * pressure.min(1.);
            plane.aircraft_rates.y += (omega * omega * authority * (pitch_target - plane.pitch)
                - 2. * omega * plane.aircraft_rates.y)
                * dt;
            plane.pitch += plane.aircraft_rates.y * dt;
        } else {
            // Neutral controls damp rotation without demanding a horizon-relative attitude.
            let rate_blend = 1. - (-omega * dt).exp();
            let roll_rate = -surfaces.x * authority * 0.60 / self.response_time;
            let pitch_rate = surfaces.y * authority * 0.19 / self.response_time;
            plane.aircraft_rates.x += (roll_rate - plane.aircraft_rates.x) * rate_blend;
            plane.aircraft_rates.y += (pitch_rate - plane.aircraft_rates.y) * rate_blend;
            plane.roll += plane.aircraft_rates.x * dt;
            // This flight-path model uses horizontal speed and cannot integrate a loop.
            let pitch = plane.pitch + plane.aircraft_rates.y * dt;
            plane.pitch = pitch.clamp(-std::f32::consts::FRAC_PI_3, std::f32::consts::FRAC_PI_3);
            if pitch != plane.pitch {
                plane.aircraft_rates.y = 0.;
            }
        }

        let incidence = plane.pitch - plane.velocity.z.atan2(speed.max(1.)) - 0.035;
        let separation = ((incidence.abs() - 0.22) / 0.17).clamp(0., 1.);
        let lift_slope = 4.2 - 2.8 * separation;
        let lift_g = ((pressure * 0.80).min(1.) + pressure.min(2.5) * lift_slope * incidence)
            .clamp(-0.8, 3.2);
        if plane.grounded {
            let wheel_load = (1. - lift_g).clamp(0., 1.);
            let wheel_turn = if on_wheels {
                speed * plane.steering.tan() / plane.wheelbase()
            } else {
                0.
            };
            let turn = wheel_load * wheel_turn + (1. - wheel_load) * surfaces.z * 0.15 * authority;
            // Lateral acceleration loads the outside main gear. Positive roll
            // lowers the starboard wing when the tires accelerate the plane to port.
            let lateral_g = speed * wheel_load * wheel_turn / 9.81;
            let roll_target = lateral_g.clamp(-1., 1.) * 5_f32.to_radians();
            let roll_response = 2. * omega;
            plane.aircraft_rates.x += (roll_response.powi(2) * (roll_target - plane.roll)
                - 2. * roll_response * plane.aircraft_rates.x)
                * dt;
            plane.roll += plane.aircraft_rates.x * dt;
            let side = Vec3::new(-plane.yaw.sin(), plane.yaw.cos(), 0.);
            // The main gear tracks the fuselage heading. The origin ahead of it
            // moves sideways as the nose comes around.
            let axle_offset = -MAIN_GEAR_X * plane.length();
            let lateral = plane.velocity.dot(side) - axle_offset * plane.aircraft_rates.z;
            let tire_grip = wheel_load * (0.8 + 5. * lateral.abs()) * dt;
            let sideways = axle_offset * turn + lateral - lateral.clamp(-tire_grip, tire_grip);
            plane.aircraft_rates.z = turn;
            plane.yaw += turn * dt;
            plane.velocity = Vec3::new(
                plane.yaw.cos() * speed,
                plane.yaw.sin() * speed,
                plane.velocity.z,
            ) + Vec3::new(-plane.yaw.sin(), plane.yaw.cos(), 0.) * sideways;
        } else {
            // Horizontal lift turns the flight path, including through knife-edge bank.
            let turn =
                -9.81 * lift_g * plane.roll.sin() / speed.max(12.) + surfaces.z * 0.15 * authority;
            plane.aircraft_rates.z += (turn - plane.aircraft_rates.z) * (1. - (-dt * omega).exp());
            plane.yaw += plane.aircraft_rates.z * dt;

            // Flight path lags the nose. Automatic cruise trim provides level
            // lift, with reduced lift at low speed, high incidence, and in a bank.
            let track = plane.velocity.y.atan2(plane.velocity.x);
            let slip = (plane.yaw - track).sin().atan2((plane.yaw - track).cos());
            let course = track + slip * (1. - (-dt * authority * 1.7).exp());
            plane.velocity.x = course.cos() * speed;
            plane.velocity.y = course.sin() * speed;
        }
        let vertical_accel = 9.81 * (lift_g * plane.roll.cos() - 1.);
        let ceiling_rate = ((530. - plane.position.z) * 0.4).max(0.);
        plane.velocity.z = (plane.velocity.z + vertical_accel * dt).min(ceiling_rate);
        let next = move_against_buildings(plane, city, dt);
        let xy = (next.truncate() / METERS_PER_UNIT).to_array();
        let water_level = city
            .elevation
            .as_ref()
            .and_then(|e| e.spec.water_level)
            .unwrap_or(0.);
        // Beyond authored terrain, keep the flight path continuous and use
        // sea level for ditching if the aircraft descends that far.
        let (kind, terrain_height) = match city.surface_at(xy, f32::INFINITY) {
            Some(surface) if surface.kind != SurfaceKind::Water => {
                (surface.kind, surface.height * METERS_PER_UNIT)
            }
            _ => (SurfaceKind::Water, water_level * METERS_PER_UNIT),
        };
        let resting = terrain_height + plane.ride_height();
        let follow_ground = plane.grounded && plane.velocity.z <= 0. && kind != SurfaceKind::Water;
        let airborne = !plane.grounded;
        plane.position = next;
        plane.grounded = next.z <= resting || follow_ground;
        if plane.grounded {
            if airborne
                && kind != SurfaceKind::Water
                && -plane.velocity.z >= super::crash::HARD_LANDING_MPS
            {
                plane.sound_impact(-plane.velocity.z, crate::audio::Material::Hard, false);
                plane.record_severe_contact(
                    next.truncate().extend(terrain_height + 0.35),
                    -plane.velocity.z,
                    super::crash::HARD_LANDING_MPS,
                );
            }
            plane.position.z = resting;
            plane.velocity.z = 0.;
            plane.surface = kind;
            if kind == SurfaceKind::Water {
                plane.velocity *= (-1.25 * dt).exp();
            }
        }
        // Allow the gear time to extend on a steep approach.
        let approach_height = plane.position.z - plane.ride_height() - terrain_height
            + plane.velocity.z.min(0.) * 0.8;
        let target = gear_target(approach_height);
        plane.aircraft_gear += (target - plane.aircraft_gear) * attitude_blend;
    }
}

/// Sample the fuselage and swept wings against the city's indexed solid
/// volumes. All measurements outside the City query are in metres.
fn move_against_buildings(plane: &mut Vehicle, city: &City, dt: f32) -> Vec3 {
    let start = plane.position;
    let travel = plane.velocity * dt;
    let end = start + travel;
    let length = plane.length();
    let span = plane.width();
    let reach = length.max(span) * 0.6;
    let hits = |center: Vec3, radius: f32, half_height: f32| {
        city.building_collision(
            (center.truncate() / METERS_PER_UNIT).to_array(),
            radius / METERS_PER_UNIT,
            (center.z - half_height) / METERS_PER_UNIT,
            (center.z + half_height) / METERS_PER_UNIT,
        )
    };
    if !hits(end, reach, reach) {
        return end;
    }

    let orientation = plane.orientation();
    let body_radius = plane.half_track() / 0.89;
    let mut first = 1.;
    let mut outward = Vec3::ZERO;
    let mut contact = end;
    // Rounded nose and tail, cabin, then outer wings at their mounted height.
    for (x, y, z, radius, half_height) in [
        (0.46, 0., 0., length * 0.039, body_radius * 0.5),
        (0.23, 0., 0., body_radius, body_radius),
        (0., 0., 0., body_radius, body_radius),
        (-0.24, 0., 0., body_radius, body_radius),
        (-0.44, 0., 0., length * 0.035, body_radius),
        (-0.08, 0.27, -0.6, length * 0.045, 0.18),
        (-0.08, -0.27, -0.6, length * 0.045, 0.18),
        (-0.16, 0.48, -0.6, span * 0.021, 0.17),
        (-0.16, -0.48, -0.6, span * 0.021, 0.17),
    ] {
        let offset = orientation * Vec3::new(x * length, y * span, z);
        let overlaps = |t: f32| hits(start + offset + travel * t, radius, half_height);
        if !overlaps(first) {
            continue;
        }
        let mut clear = 0.;
        let mut blocked = first;
        for _ in 0..10 {
            let mid = (clear + blocked) * 0.5;
            if overlaps(mid) {
                blocked = mid;
            } else {
                clear = mid;
            }
        }
        let point = start + offset + travel * blocked;
        // Estimate the local clearance gradient through the same volume query.
        // This also finds roof normals and recessed/rotated facade faces.
        let clearance = |delta: Vec3| {
            let (mut lo, mut hi) = (-0.08, 0.08);
            for _ in 0..8 {
                let pad = (lo + hi) * 0.5;
                if hits(point + delta, radius + pad, half_height + pad) {
                    hi = pad;
                } else {
                    lo = pad;
                }
            }
            (lo + hi) * 0.5
        };
        let gradient = |axis: Vec3| clearance(axis * 0.04) - clearance(axis * -0.04);
        let g = Vec3::new(gradient(Vec3::X), gradient(Vec3::Y), gradient(Vec3::Z));
        let magnitude = g.length();
        if magnitude > 0.001 {
            let n = g / magnitude;
            if travel.dot(n) >= 0. {
                continue;
            }
            outward = n;
        } else if !overlaps(0.) && travel.length_squared() > 1e-10 {
            outward = -travel / travel.length();
        } else {
            continue;
        }
        contact = point;
        first = clear;
    }
    if first < 1. {
        let impact = -plane.velocity.dot(outward);
        plane.record_severe_contact(contact, impact, super::crash::ROAD_IMPACT_MPS);
        let rebound = if impact > 2. { 0.25 } else { 0. };
        plane.velocity += outward * (impact * (1. + rebound));
        plane.sound_impact(impact, crate::audio::Material::Hard, false);
        plane.collisions += 1;
        start + travel * first + outward * 0.003
    } else {
        end
    }
}
