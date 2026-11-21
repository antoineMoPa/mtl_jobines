//! Shot picking uses the same moving wheel and body panels as rendering.
use super::*;
use rapier3d::prelude::{Point, Ray, RayCast, TriMesh, Vector};

impl Definition {
    pub(super) fn road_wheel_parts(config: &Config, parts: &[Part]) -> Option<[usize; 4]> {
        if !config.road_vehicle || config.vehicle_kind != VehicleKind::Motorized {
            return None;
        }
        let mut wheels = [None; 4];
        for (i, part) in parts.iter().enumerate() {
            let front = match part.role {
                PartRole::FrontWheel => true,
                PartRole::RearWheel => false,
                _ => continue,
            };
            let wheel = Wheel::from_axle(front, part.pivot.y > 0.);
            // Six-wheel trucks have two non-steering axles on the same side.
            if wheels[wheel as usize].replace(i).is_some() {
                return None;
            }
        }
        let [Some(rr), Some(rl), Some(fr), Some(fl)] = wheels else {
            return None;
        };
        Some([rr, rl, fr, fl])
    }

    pub(super) fn wheel_at_part(&self, index: usize) -> Option<Wheel> {
        let wheels = self.road_wheels?;
        Wheel::ALL
            .into_iter()
            .find(|&w| wheels[w as usize] == index)
    }

    /// Origin and unit direction are chassis-local, in metres.
    pub(in crate::dynamic_world) fn road_shot_hit(
        &self,
        vehicle: &crate::dynamic_world::Vehicle,
        origin: Vec3,
        dir: Vec3,
        mut limit: f32,
    ) -> Option<(f32, Vec3, Option<Wheel>)> {
        let shapes = self.shot_meshes.get_or_init(|| {
            self.parts
                .iter()
                .map(|part| {
                    let points: Vec<_> = part
                        .vertices
                        .iter()
                        .chain(
                            self.rear_lenses
                                .iter()
                                .filter(|_| part.role == PartRole::Body),
                        )
                        .map(|v| Point::from(v.position))
                        .collect();
                    let triangles = (0..points.len() as u32)
                        .step_by(3)
                        .map(|i| [i, i + 1, i + 2])
                        .collect();
                    TriMesh::new(points, triangles)
                })
                .collect()
        });
        let lean = vehicle.cabin_lean();
        let mut hit = None;
        for (i, shape) in shapes.iter().enumerate() {
            if vehicle
                .damage
                .as_ref()
                .is_some_and(|d| d.loose.iter().any(|p| p.part == i))
            {
                continue;
            }
            let inverse = self.part_transform(i, vehicle, lean).inverse();
            let ray = Ray::new(
                Point::from(inverse.transform_point3(origin).to_array()),
                Vector::from(inverse.transform_vector3(dir).to_array()),
            );
            let Some(contact) = shape.cast_local_ray_and_get_normal(&ray, limit, false) else {
                continue;
            };
            limit = contact.time_of_impact;
            let point = ray.point_at(limit);
            // A wheel mesh includes the hub. Only its outer rubber band can puncture.
            let wheel = self
                .wheel_at_part(i)
                .filter(|_| point.x.hypot(point.z) >= self.config.physics.wheel_radius * 0.64);
            let normal = inverse.transpose().transform_vector3(Vec3::new(
                contact.normal.x,
                contact.normal.y,
                contact.normal.z,
            ));
            hit = Some((limit, normal / normal.length(), wheel));
        }
        hit
    }
}
