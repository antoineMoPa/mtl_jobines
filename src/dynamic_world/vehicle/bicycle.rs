//! Two inline tire contacts with arcade rider balance assistance.
use super::*;

impl Vehicle {
    pub(super) fn balance_bicycle(&mut self, speed: f32, dt: f32) {
        if !self.grounded {
            return;
        }
        // Lean into the actual turn. At rest the rider holds the bike upright;
        // airborne motion keeps its existing angular momentum.
        let omega = self.angular_velocity();
        let target = -(speed * omega.z / 9.81).atan().clamp(-0.42, 0.42);
        let axis = self.orientation() * Vec3::X;
        let acceleration = (target - self.roll) * 100. - omega.dot(axis) * 20.;
        let response = self.inverse_inertia(axis).dot(axis);
        self.angular_momentum += axis * (acceleration / response * dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (City, Vehicle) {
        let city = City::from_program(
            &opencity::parse(
                "road r: carlane; end r(-30,0,30,0); land(-31,-10,31,-10,31,10,-31,10);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let mut bike = Vehicle::spawn(&city);
        bike.set_catalog_model(
            super::super::super::catalog::definitions()
                .iter()
                .position(|d| d.config.research_id == super::super::super::catalog::BICYCLE_ID),
        );
        bike.position = Vec3::new(0., 0., bike.ride_height());
        bike.yaw = 0.;
        (city, bike)
    }

    #[test]
    fn bicycle_has_two_contacts_pedals_and_service_brakes() {
        for dt in [1. / 120., 1. / 30.] {
            let (city, mut bike) = setup();
            assert_eq!(
                bike.wheel_contacts(&city)
                    .iter()
                    .filter(|(_, s)| s.is_some())
                    .count(),
                2
            );
            for _ in 0..(20. / dt) as usize {
                bike.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    dt,
                );
            }
            assert!(
                (8. ..12.1).contains(&bike.velocity.x),
                "{:?}",
                bike.velocity
            );
            assert!(bike.roll.abs() < 0.05);
            assert!((bike.position.z - bike.ride_height()).abs() < 0.02);
            assert!(bike.wheel_angle > 10.);
            for _ in 0..(4. / dt) as usize {
                bike.step(
                    &city,
                    Controls {
                        brake: true,
                        ..Default::default()
                    },
                    dt,
                );
            }
            assert!(bike.velocity.length() < 0.1, "{:?}", bike.velocity);
            assert!(bike.velocity.x > -0.01);
        }
    }

    #[test]
    fn bicycle_turns_and_balances_at_rest() {
        let (city, mut bike) = setup();
        bike.velocity.x = 4.;
        for _ in 0..240 {
            bike.step(
                &city,
                Controls {
                    throttle: 0.5,
                    steering: 0.45,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(bike.yaw > 0.2, "yaw {}", bike.yaw);
        assert!(bike.roll < -0.02 && bike.roll > -0.6, "roll {}", bike.roll);
        assert!(bike.position.is_finite() && bike.grounded);
        let (_, mut bike) = setup();
        bike.roll = 0.15;
        for _ in 0..360 {
            bike.step(&city, Controls::default(), 1. / 120.);
        }
        assert!(bike.roll.abs() < 0.03, "parked roll {}", bike.roll);
    }
}

#[cfg(test)]
mod asset_tests {
    use crate::dynamic_world::{catalog, DynamicWorld};
    use crate::hermes::Hermes;

    #[test]
    fn startup_bicycle_has_embedded_alpha_spokes_and_no_basket_or_plate() {
        let world = DynamicWorld::new(&Hermes::test_map());
        assert!(world.player().is_bicycle());
        let definition = catalog::definitions()
            .iter()
            .find(|d| d.is_bicycle())
            .unwrap();
        let bytes = include_bytes!("../../../data/vehicle/bicycle/velo-partage/model.glb");
        let glb = gltf::Gltf::from_slice(bytes).unwrap();
        assert!(!glb
            .nodes()
            .any(|n| n.name().unwrap_or("").starts_with("basket_")));
        assert_eq!(
            glb.nodes()
                .filter(|n| matches!(n.name(), Some("wheel_front" | "wheel_rear")))
                .count(),
            2
        );
        let spokes: Vec<_> = glb
            .meshes()
            .flat_map(|m| m.primitives())
            .filter(|p| {
                p.material()
                    .pbr_metallic_roughness()
                    .base_color_texture()
                    .is_some()
            })
            .collect();
        assert_eq!(spokes.len(), 2);
        for p in spokes {
            assert_eq!(p.indices().unwrap().count(), 6);
            assert!(p.material().double_sided());
            assert_eq!(p.material().alpha_mode(), gltf::material::AlphaMode::Blend);
        }
        let mut png = png::Decoder::new(definition.texture_png.as_deref().unwrap())
            .read_info()
            .unwrap();
        let mut pixels = vec![0; png.output_buffer_size()];
        let info = png.next_frame(&mut pixels).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert!(pixels[..info.buffer_size()]
            .chunks_exact(4)
            .any(|p| p[3] == 0));
        assert!(pixels[..info.buffer_size()]
            .chunks_exact(4)
            .any(|p| p[3] == 255));
        assert_eq!(definition.config.sound.engine_gain, 0.);
        assert_eq!(definition.config.sound.horn.gain, 0.);
    }
}

#[cfg(test)]
mod sprint_tests {
    use super::*;
    #[test]
    fn bicycle_shift_sprint_is_faster_and_blends_to_standing_then_back() {
        let city = City::from_program(
            &opencity::parse("road r: carlane; end r(-30,0,30,0);").unwrap(),
            None,
        )
        .unwrap();
        let model = super::super::super::catalog::definitions()
            .iter()
            .position(|d| d.is_bicycle())
            .unwrap();
        let ride = |boost| {
            let mut bike = Vehicle::spawn(&city);
            bike.set_catalog_model(Some(model));
            bike.position = Vec3::new(-2000., 0., bike.ride_height());
            bike.yaw = 0.;
            for _ in 0..1200 {
                bike.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        boost,
                        ..Default::default()
                    },
                    1. / 60.,
                );
            }
            bike
        };
        let normal = ride(false);
        let mut sprint = ride(true);
        assert!(
            normal.velocity.x > 9.,
            "normal cycling speed {}",
            normal.velocity.x
        );
        assert!(
            sprint.velocity.x > normal.velocity.x * 1.3,
            "{} vs {}",
            sprint.velocity.x,
            normal.velocity.x
        );
        assert!(sprint.riding_stand > 0.99);
        assert!(normal.riding_stand < 0.01);
        for _ in 0..90 {
            sprint.step(&city, Controls::default(), 1. / 60.);
        }
        assert!(sprint.riding_stand < 0.01);
    }
}

#[cfg(test)]
mod backpedal_tests {
    use super::*;
    #[test]
    fn bicycle_can_backpedal_reverse_and_change_direction_after_braking() {
        let city = City::from_program(
            &opencity::parse("road r: carlane; end r(-30,0,30,0);").unwrap(),
            None,
        )
        .unwrap();
        for dt in [1. / 120., 1. / 30.] {
            let mut bike = Vehicle::spawn(&city);
            bike.set_catalog_model(
                super::super::super::catalog::definitions()
                    .iter()
                    .position(|d| d.is_bicycle()),
            );
            bike.position = Vec3::new(0., 0., bike.ride_height());
            bike.yaw = 0.;
            for _ in 0..(6. / dt) as usize {
                bike.step(
                    &city,
                    Controls {
                        throttle: -1.,
                        ..Default::default()
                    },
                    dt,
                );
            }
            assert!(bike.velocity.x < -2. && bike.velocity.x > -5.);
            assert!(bike.pedal_angle < -5. && bike.wheel_angle < -5.);
            let phase = bike.pedal_angle;
            for _ in 0..(10. / dt) as usize {
                bike.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    dt,
                );
            }
            assert!(bike.velocity.x > 5.);
            assert!(bike.pedal_angle > phase);
        }
    }
}
