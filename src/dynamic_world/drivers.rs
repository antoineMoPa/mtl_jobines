//! Occupied seats follow the sprung cabin in the same coordinate frame as its doors.
use super::{crowd::Instance, pedestrian_catalog, DynamicWorld, Vehicle, METERS_PER_UNIT};
use glam::{Mat4, Quat, Vec3, Vec4};
use std::sync::OnceLock;

pub(super) fn seat(id: &str) -> Option<Vec3> {
    let position = match id {
        "vehicle-v2-09" | "vehicle-j01" | "vehicle-j06" => [0.10, 0.35, 0.46],
        super::baggage::MODEL_ID => [-0.54, 0., 1.04],
        "vehicle-v2-01" => [0.44, 0.47, 0.97],
        "vehicle-12" => [0.13, 0.44, 0.76],
        "vehicle-j02" => [4.24, 0.66, 1.12],
        "vehicle-j03" => [0.76, 0.56, 1.72],
        "vehicle-j05" => [2.66, 0.59, 2.05],
        "vehicle-j07" => [2.98, 0.61, 1.45],
        "vehicle-r01" => [7.35, 0.58, 2.04],
        "vehicle-r02" => [7.78, 0.61, 1.75],
        "vehicle-air-estuaire" => [13.15, 0.56, 3.38],
        "vehicle-air-fleche" => [5.15, 0.24, 1.08],
        "vehicle-boat-voile" => [-0.28, 0., 0.26],
        "vehicle-boat-cabine" => [-0.24, -0.43, 1.08],
        "vehicle-boat-runabout" => [-0.31, -0.48, 0.59],
        _ => return None,
    };
    Some(position.into())
}
pub(super) fn traffic_model(id: usize, vehicle: &Vehicle) -> usize {
    if vehicle
        .definition()
        .is_some_and(|d| d.config.research_id == super::traffic::CRUISER)
    {
        pedestrian_catalog::POLICE_OFFICER_MODEL
    } else {
        id % pedestrian_catalog::CROWD_MODEL_COUNT
    }
}
impl DynamicWorld {
    pub(super) fn seated_drivers(&self) -> impl Iterator<Item = Instance<'_>> {
        let player = (self.is_driving() && self.entry.is_none()).then(|| {
            (
                self.player(),
                pedestrian_catalog::player_model(self.season),
                1.,
            )
        });
        let npc = |id: usize| id % pedestrian_catalog::CROWD_MODEL_COUNT;
        let traffic = self
            .traffic
            .driven_slots()
            .filter(|(id, _)| !self.officer_out_of(*id))
            .map(move |(id, opacity)| {
                let vehicle = &self.vehicles[id];
                (vehicle, traffic_model(id, vehicle), opacity)
            });
        let boats = self
            .skippers()
            .map(|(id, model)| (&self.vehicles[id], model, 1.));
        let transit = self
            .airport
            .planes()
            .chain(self.metro.train.iter())
            .enumerate()
            .map(move |(id, v)| (v, npc(id), 1.));
        player
            .into_iter()
            .chain(traffic)
            .chain(boats)
            .chain(transit)
            .filter_map(move |(v, model, opacity)| {
                if v.position.distance_squared(self.player_position())
                    > crate::performance::draw_radius(180.).powi(2)
                {
                    return None;
                }
                driver(v, model, opacity)
            })
            .chain(
                self.entry
                    .iter()
                    .filter(|_| self.is_driving() && !self.player().is_bicycle())
                    .map(|entry| entry.instance(self.player())),
            )
    }
}
fn driver(v: &Vehicle, model: usize, opacity: f32) -> Option<Instance<'static>> {
    let id = &v.definition()?.config.research_id;
    let seat = seat(id)?;
    let axes = Mat4::from_cols(Vec4::Y, Vec4::Z, Vec4::X, Vec4::W);
    let transform = Mat4::from_scale_rotation_translation(
        Vec3::splat(1. / METERS_PER_UNIT),
        v.orientation() * v.cabin_lean(),
        v.position / METERS_PER_UNIT,
    ) * Mat4::from_translation(seat - Vec3::Z * v.ride_height())
        * axes;
    Some(Instance {
        zombie: false,
        model,
        transform,
        opacity,
        idle_time: 0.,
        walk_time: 0.,
        movement: 0.,
        pose: Some(seat_pose(id, model)),
    })
}
pub(super) fn seat_pose(
    id: &str,
    model: usize,
) -> &'static [Mat4; pedestrian_catalog::JOINT_LIMIT] {
    &seated(low_roof(id))[model]
}
fn low_roof(id: &str) -> bool {
    matches!(
        id,
        "vehicle-v2-09" | "vehicle-j01" | "vehicle-j06" | "vehicle-air-fleche"
    )
}
fn seated(reclined: bool) -> &'static Vec<[Mat4; pedestrian_catalog::JOINT_LIMIT]> {
    static POSES: OnceLock<Vec<[Mat4; pedestrian_catalog::JOINT_LIMIT]>> = OnceLock::new();
    static RECLINED: OnceLock<Vec<[Mat4; pedestrian_catalog::JOINT_LIMIT]>> = OnceLock::new();
    let cache = if reclined { &RECLINED } else { &POSES };
    cache.get_or_init(|| {
        pedestrian_catalog::definitions()
            .iter()
            .map(|d| {
                let mut locals = d.rest.clone();
                if reclined {
                    locals[d.node("Spine")].rotation *= Quat::from_rotation_x(-0.58);
                    locals[d.node("Head")].rotation *= Quat::from_rotation_x(0.38);
                }
                for side in ["L", "R"] {
                    for (joint, angle) in [
                        ("Thigh", if reclined { -1.84 } else { -1.42 }),
                        ("Shin", if reclined { 0.51 } else { 1.48 }),
                        ("Foot", if reclined { 1.33 } else { -0.06 }),
                        ("UpperArm", -0.69),
                        ("Forearm", -0.82),
                    ] {
                        locals[d.node(&format!("{joint}_{side}"))].rotation =
                            Quat::from_rotation_x(angle);
                    }
                }
                let hip = d.rest_worlds()[d.node("Hips")].w_axis.truncate();
                let worlds = d.worlds(
                    &locals,
                    Mat4::from_translation(-hip),
                    &vec![None; locals.len()],
                );
                let mut pose = [Mat4::IDENTITY; pedestrian_catalog::JOINT_LIMIT];
                for (i, &n) in d.joints.iter().enumerate() {
                    pose[i] = worlds[n] * d.inverse_bind[i];
                }
                pose
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seated_people_fit_under_the_compact_roof_and_above_its_floor() {
        let pelvis = seat("vehicle-v2-09").unwrap();
        for (model, definition) in pedestrian_catalog::definitions().iter().enumerate() {
            let pose = &seated(true)[model];
            let mut bottom = f32::INFINITY;
            let mut top = f32::NEG_INFINITY;
            for (v, skin) in definition.vertices.iter().zip(&definition.skin) {
                let height = pelvis.z
                    + (0..4)
                        .map(|i| {
                            pose[skin.joints[i] as usize]
                                .transform_point3(v.position.into())
                                .y
                                * skin.weights[i]
                        })
                        .sum::<f32>();
                bottom = bottom.min(height);
                top = top.max(height);
            }
            assert!(
                bottom >= 0.17 && top <= 1.29,
                "model {model}: {bottom}..{top}"
            );
        }
    }
}
