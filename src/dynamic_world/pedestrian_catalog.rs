//! Shared skinned assets. Source coordinates are metres, +Y up, +Z forward.
use glam::{Mat4, Quat, Vec3, Vec4};
use gltf::animation::util::ReadOutputs;
use opencity::runtime::{SurfaceKind, Vertex};
use std::sync::OnceLock;

pub const JOINT_LIMIT: usize = 32;
pub const CROWD_MODEL_COUNT: usize = 6;
pub const PLAYER_SUMMER_MODEL: usize = 6;
pub const PLAYER_WINTER_MODEL: usize = 7;
pub const POLICE_OFFICER_MODEL: usize = 8;
fn assets() -> [&'static [u8]; 9] {
    [
        crate::web_asset!("../../data/pedestrian/01.glb", "data/pedestrian/01.glb"),
        crate::web_asset!("../../data/pedestrian/02.glb", "data/pedestrian/02.glb"),
        crate::web_asset!("../../data/pedestrian/03.glb", "data/pedestrian/03.glb"),
        crate::web_asset!("../../data/pedestrian/04.glb", "data/pedestrian/04.glb"),
        crate::web_asset!("../../data/pedestrian/05.glb", "data/pedestrian/05.glb"),
        crate::web_asset!("../../data/pedestrian/06.glb", "data/pedestrian/06.glb"),
        crate::web_asset!(
            "../../data/pedestrian/player.glb",
            "data/pedestrian/player.glb"
        ),
        crate::web_asset!(
            "../../data/pedestrian/player-winter.glb",
            "data/pedestrian/player-winter.glb"
        ),
        crate::web_asset!(
            "../../data/pedestrian/officer.glb",
            "data/pedestrian/officer.glb"
        ),
    ]
}

pub fn player_model(season: crate::season::Season) -> usize {
    match season {
        crate::season::Season::Summer => PLAYER_SUMMER_MODEL,
        crate::season::Season::Winter => PLAYER_WINTER_MODEL,
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkinVertex {
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}
#[derive(Clone, Copy)]
pub(super) struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}
impl Transform {
    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
    fn blend(self, other: Self, amount: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, amount),
            rotation: self.rotation.slerp(other.rotation, amount),
            scale: self.scale.lerp(other.scale, amount),
        }
    }
}
enum Values {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}
struct Track {
    node: usize,
    times: Vec<f32>,
    values: Values,
    interpolation: gltf::animation::Interpolation,
}
struct Clip {
    duration: f32,
    tracks: Vec<Track>,
}
impl Clip {
    fn apply(&self, time: f32, pose: &mut [Transform]) {
        let time = time.rem_euclid(self.duration);
        for track in &self.tracks {
            let upper = track.times.partition_point(|t| *t <= time);
            let a = upper.saturating_sub(1);
            let b = upper.min(track.times.len() - 1);
            let span = track.times[b] - track.times[a];
            let t = if track.interpolation == gltf::animation::Interpolation::Step {
                0.
            } else if span > 0. {
                (time - track.times[a]) / span
            } else {
                0.
            };
            let target = &mut pose[track.node];
            match &track.values {
                Values::Translation(v) => target.translation = v[a].lerp(v[b], t),
                Values::Rotation(v) => target.rotation = v[a].slerp(v[b], t),
                Values::Scale(v) => target.scale = v[a].lerp(v[b], t),
            }
        }
    }
}

pub struct Definition {
    pub vertices: Vec<Vertex>,
    pub skin: Vec<SkinVertex>,
    pub texture_png: Vec<u8>,
    pub walk_speed: f32,
    pub walk_duration: f32,
    pub(super) rest: Vec<Transform>,
    parents: Vec<Option<usize>>,
    pub(super) joints: Vec<usize>,
    pub(super) inverse_bind: Vec<Mat4>,
    names: Vec<String>,
    idle: Clip,
    walk: Clip,
}
impl Definition {
    pub(super) fn node(&self, name: &str) -> usize {
        self.joints[self.joint(name)]
    }
    pub(super) fn idle_locals(&self, time: f32) -> Vec<Transform> {
        let mut pose = self.rest.clone();
        self.idle.apply(time, &mut pose);
        pose
    }
    pub(super) fn worlds(
        &self,
        pose: &[Transform],
        root: Mat4,
        overrides: &[Option<Mat4>],
    ) -> Vec<Mat4> {
        fn resolve(
            def: &Definition,
            pose: &[Transform],
            root: Mat4,
            overrides: &[Option<Mat4>],
            cache: &mut [Option<Mat4>],
            node: usize,
        ) -> Mat4 {
            if let Some(world) = cache[node] {
                return world;
            }
            let world = overrides[node].unwrap_or_else(|| {
                def.parents[node].map_or(root, |p| resolve(def, pose, root, overrides, cache, p))
                    * pose[node].matrix()
            });
            cache[node] = Some(world);
            world
        }
        let mut cache = vec![None; pose.len()];
        (0..pose.len())
            .map(|node| resolve(self, pose, root, overrides, &mut cache, node))
            .collect()
    }
    pub(super) fn rest_worlds(&self) -> Vec<Mat4> {
        self.worlds(&self.rest, Mat4::IDENTITY, &vec![None; self.rest.len()])
    }
    fn read(bytes: &[u8]) -> Self {
        let source = gltf::Gltf::from_slice(bytes).expect("embedded pedestrian GLB");
        let blob = source.blob.as_deref().expect("embedded pedestrian buffer");
        let buffer = |_: gltf::Buffer<'_>| Some(blob);
        let mesh_node = source
            .nodes()
            .find(|n| n.skin().is_some())
            .expect("pedestrian skin node");
        assert_eq!(
            mesh_node.transform().matrix(),
            Mat4::IDENTITY.to_cols_array_2d()
        );
        let source_skin = mesh_node.skin().unwrap();
        let joints: Vec<_> = source_skin.joints().map(|n| n.index()).collect();
        let names = source_skin
            .joints()
            .map(|n| n.name().unwrap_or("").to_owned())
            .collect();
        assert!(joints.len() <= JOINT_LIMIT);
        let inverse_bind = source_skin
            .reader(buffer)
            .read_inverse_bind_matrices()
            .expect("inverse bind matrices")
            .map(|m| Mat4::from_cols_array_2d(&m))
            .collect();
        let rest: Vec<_> = source
            .nodes()
            .map(|node| {
                let (translation, rotation, scale) = node.transform().decomposed();
                Transform {
                    translation: Vec3::from(translation),
                    rotation: Quat::from_array(rotation),
                    scale: Vec3::from(scale),
                }
            })
            .collect();
        let mut parents = vec![None; rest.len()];
        for node in source.nodes() {
            for child in node.children() {
                parents[child.index()] = Some(node.index());
            }
        }
        let mut vertices = Vec::new();
        let mut skin = Vec::new();
        for primitive in mesh_node.mesh().unwrap().primitives() {
            assert_eq!(primitive.mode(), gltf::mesh::Mode::Triangles);
            let reader = primitive.reader(buffer);
            let positions: Vec<_> = reader.read_positions().unwrap().collect();
            let normals: Vec<_> = reader.read_normals().unwrap().collect();
            let uvs: Vec<_> = reader.read_tex_coords(0).unwrap().into_f32().collect();
            let joint_ids: Vec<_> = reader.read_joints(0).unwrap().into_u16().collect();
            let weights: Vec<_> = reader.read_weights(0).unwrap().into_f32().collect();
            let material = primitive.material();
            let pbr = material.pbr_metallic_roughness();
            let [r, g, b, _] = pbr.base_color_factor();
            let colors: Option<Vec<_>> = reader.read_colors(0).map(|c| c.into_rgb_f32().collect());
            // Encoded in surface.y for the pedestrian shader.
            let finish = match (pbr.base_color_texture(), material.name()) {
                (Some(_), _) => 0.,
                (_, Some("Mullet")) => 2.,
                _ => 1.,
            };
            for index in reader.read_indices().unwrap().into_u32() {
                let i = index as usize;
                let tint = colors.as_ref().map_or([1.; 3], |c| c[i]);
                vertices.push(Vertex {
                    position: positions[i],
                    normal: normals[i],
                    color: [r * tint[0], g * tint[1], b * tint[2]],
                    surface: [
                        SurfaceKind::Prop as u32 as f32,
                        finish,
                        uvs[i][0],
                        uvs[i][1],
                    ],
                });
                skin.push(SkinVertex {
                    joints: joint_ids[i],
                    weights: weights[i],
                });
            }
        }
        let image = source.images().next().expect("pedestrian albedo");
        let gltf::image::Source::View {
            view,
            mime_type: "image/png",
        } = image.source()
        else {
            panic!("pedestrian albedo must be embedded PNG");
        };
        let texture_png = blob[view.offset()..view.offset() + view.length()].to_vec();
        let read_clip = |name| {
            let animation = source
                .animations()
                .find(|a| a.name() == Some(name))
                .expect("pedestrian clip");
            let mut duration = 0f32;
            let tracks = animation
                .channels()
                .map(|channel| {
                    let interpolation = channel.sampler().interpolation();
                    assert!(matches!(
                        interpolation,
                        gltf::animation::Interpolation::Linear
                            | gltf::animation::Interpolation::Step
                    ));
                    let reader = channel.reader(buffer);
                    let times: Vec<_> = reader.read_inputs().unwrap().collect();
                    duration = duration.max(*times.last().unwrap());
                    let values = match reader.read_outputs().unwrap() {
                        ReadOutputs::Translations(v) => {
                            Values::Translation(v.map(Vec3::from).collect())
                        }
                        ReadOutputs::Rotations(v) => {
                            Values::Rotation(v.into_f32().map(Quat::from_array).collect())
                        }
                        ReadOutputs::Scales(v) => Values::Scale(v.map(Vec3::from).collect()),
                        _ => panic!("unsupported pedestrian animation channel"),
                    };
                    Track {
                        node: channel.target().node().index(),
                        times,
                        values,
                        interpolation,
                    }
                })
                .collect();
            assert!(duration > 0.);
            Clip { duration, tracks }
        };
        let idle = read_clip("Idle");
        let walk = read_clip("Walk");
        // Match travel to the baked foot excursion rather than playing every
        // character at one crowd-wide cadence. Metadata remains inside the GLB.
        let extras: serde_json::Value = serde_json::from_str(
            source
                .scenes()
                .next()
                .unwrap()
                .extras()
                .as_ref()
                .unwrap()
                .get(),
        )
        .unwrap();
        let stride = extras["motion"]["stride"].as_f64().unwrap() as f32;
        let height = extras["heightMetres"].as_f64().unwrap() as f32;
        let walk_speed = 4. * stride.sin() * height * 0.52 / walk.duration;
        Self {
            vertices,
            skin,
            texture_png,
            walk_speed,
            walk_duration: walk.duration,
            rest,
            parents,
            joints,
            inverse_bind,
            names,
            idle,
            walk,
        }
    }
    fn world_matrix(&self, node: usize, pose: &[Transform], cache: &mut [Option<Mat4>]) -> Mat4 {
        if let Some(matrix) = cache[node] {
            return matrix;
        }
        let local = pose[node].matrix();
        let world = self.parents[node].map_or(local, |parent| {
            self.world_matrix(parent, pose, cache) * local
        });
        cache[node] = Some(world);
        world
    }
    pub fn pose(&self, idle_time: f32, walk_time: f32, movement: f32) -> [Mat4; JOINT_LIMIT] {
        let mut idle = self.rest.clone();
        let mut walk = self.rest.clone();
        self.idle.apply(idle_time, &mut idle);
        self.walk.apply(walk_time, &mut walk);
        for (a, b) in idle.iter_mut().zip(walk) {
            *a = a.blend(b, movement.clamp(0., 1.));
        }
        let mut cache = vec![None; self.rest.len()];
        let mut result = [Mat4::IDENTITY; JOINT_LIMIT];
        for (i, &joint) in self.joints.iter().enumerate() {
            result[i] = self.world_matrix(joint, &idle, &mut cache) * self.inverse_bind[i];
        }
        result
    }
    pub(super) fn joint(&self, name: &str) -> usize {
        self.names
            .iter()
            .position(|n| n == name)
            .expect("pedestrian joint contract")
    }
    pub(super) fn joint_count(&self) -> usize {
        self.joints.len()
    }
    pub(super) fn joint_parent(&self, index: usize) -> Option<usize> {
        self.parents[self.joints[index]]
            .and_then(|parent| self.joints.iter().position(|&n| n == parent))
    }
    pub(super) fn joint_worlds(&self, skin: &[Mat4; JOINT_LIMIT]) -> [Mat4; JOINT_LIMIT] {
        let mut result = [Mat4::IDENTITY; JOINT_LIMIT];
        for (i, inverse) in self.inverse_bind.iter().enumerate() {
            result[i] = skin[i] * inverse.inverse();
        }
        result
    }
    pub(super) fn skin_from_worlds(&self, worlds: &[Mat4; JOINT_LIMIT]) -> [Mat4; JOINT_LIMIT] {
        let mut result = [Mat4::IDENTITY; JOINT_LIMIT];
        for (i, inverse) in self.inverse_bind.iter().enumerate() {
            result[i] = worlds[i] * *inverse;
        }
        result
    }
    pub(super) fn crouching_worlds(&self, crouch: f32) -> [Mat4; JOINT_LIMIT] {
        let mut target = self.rest.clone();
        self.idle.apply(0.3, &mut target);
        let node = |name: &str| self.joints[self.joint(name)];
        target[node("Hips")].translation.y -= self.rest[node("Hips")].translation.y * 0.25 * crouch;
        for side in ["L", "R"] {
            for (name, angle) in [("Thigh", -0.65), ("Shin", 1.3), ("Foot", -0.65)] {
                target[node(&format!("{name}_{side}"))].rotation *=
                    Quat::from_rotation_x(angle * crouch);
            }
        }
        target[node("Spine")].rotation *= Quat::from_rotation_x(0.22 * crouch);
        let mut cache = vec![None; self.rest.len()];
        let mut target_worlds = [Mat4::IDENTITY; JOINT_LIMIT];
        for (i, &joint) in self.joints.iter().enumerate() {
            target_worlds[i] = self.world_matrix(joint, &target, &mut cache);
        }
        target_worlds
    }
    pub(super) fn get_up_pose(
        &self,
        fallen: &[Mat4; JOINT_LIMIT],
        progress: f32,
    ) -> [Mat4; JOINT_LIMIT] {
        let crouch = (1. - (progress - 0.35).max(0.) / 0.65).clamp(0., 1.);
        let target_worlds = self.crouching_worlds(crouch);
        let start = self.joint_worlds(fallen);
        let t = (progress / 0.42).clamp(0., 1.);
        let amount = t * t * (3. - 2. * t);
        self.blend_worlds(&start, &target_worlds, amount)
    }
    pub(super) fn blend_worlds(
        &self,
        start: &[Mat4; JOINT_LIMIT],
        target_worlds: &[Mat4; JOINT_LIMIT],
        amount: f32,
    ) -> [Mat4; JOINT_LIMIT] {
        let mut locals = [Mat4::IDENTITY; JOINT_LIMIT];
        for i in 0..self.joints.len() {
            let parent = self.joint_parent(i);
            let a = parent.map_or(start[i], |p| start[p].inverse() * start[i]);
            let b = parent.map_or(target_worlds[i], |p| {
                target_worlds[p].inverse() * target_worlds[i]
            });
            let (sa, qa, ta) = a.to_scale_rotation_translation();
            let (sb, qb, tb) = b.to_scale_rotation_translation();
            locals[i] = Mat4::from_scale_rotation_translation(
                sa.lerp(sb, amount),
                qa.slerp(qb, amount),
                ta.lerp(tb, amount),
            );
        }
        fn resolve(
            def: &Definition,
            index: usize,
            locals: &[Mat4; JOINT_LIMIT],
            worlds: &mut [Option<Mat4>; JOINT_LIMIT],
        ) -> Mat4 {
            if let Some(world) = worlds[index] {
                return world;
            }
            let world = def.joint_parent(index).map_or(locals[index], |p| {
                resolve(def, p, locals, worlds) * locals[index]
            });
            worlds[index] = Some(world);
            world
        }
        let mut cache = [None; JOINT_LIMIT];
        let worlds = std::array::from_fn(|i| {
            if i < self.joints.len() {
                resolve(self, i, &locals, &mut cache)
            } else {
                Mat4::IDENTITY
            }
        });
        self.skin_from_worlds(&worlds)
    }
}
pub fn definitions() -> &'static [Definition] {
    static MODELS: OnceLock<Vec<Definition>> = OnceLock::new();
    MODELS.get_or_init(|| assets().into_iter().map(Definition::read).collect())
}
pub fn instance_transform(position: Vec3, yaw: f32) -> Mat4 {
    let axes = Mat4::from_cols(Vec4::Y, Vec4::Z, Vec4::X, Vec4::W);
    Mat4::from_translation(position / super::METERS_PER_UNIT)
        * Mat4::from_rotation_z(yaw)
        * Mat4::from_scale(Vec3::splat(1. / super::METERS_PER_UNIT))
        * axes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pedestrian_assets_have_valid_skin_bind_pose_and_distinct_moving_clips() {
        let models = definitions();
        assert_eq!(models.len(), CROWD_MODEL_COUNT + 3);
        let mut cadences = Vec::new();
        for (index, model) in models.iter().enumerate() {
            assert_eq!(model.joints.len(), 22);
            let triangle_budget = if matches!(index, PLAYER_SUMMER_MODEL | PLAYER_WINTER_MODEL) {
                22_000
            } else {
                8_000
            };
            assert!((3_000..triangle_budget).contains(&(model.vertices.len() / 3)));
            assert_eq!(model.skin.len(), model.vertices.len());
            assert!(model.texture_png.starts_with(b"\x89PNG"));
            let mut cache = vec![None; model.rest.len()];
            for (i, &joint) in model.joints.iter().enumerate() {
                let bind =
                    model.world_matrix(joint, &model.rest, &mut cache) * model.inverse_bind[i];
                assert!(bind.abs_diff_eq(Mat4::IDENTITY, 0.00001));
            }
            for vertex in &model.skin {
                assert!((vertex.weights.iter().sum::<f32>() - 1.).abs() < 0.0001);
                assert!(vertex
                    .joints
                    .iter()
                    .all(|&j| usize::from(j) < model.joints.len()));
            }
            let a = model.pose(0.3, 0.1, 1.);
            let b = model.pose(0.3, 0.1 + model.walk_duration * 0.25, 1.);
            assert!(a.iter().all(|m| m.is_finite()));
            assert!(a.iter().zip(b).any(|(a, b)| !a.abs_diff_eq(b, 0.01)));
            assert!((0.6..1.8).contains(&model.walk_speed));
            if cadences.len() < CROWD_MODEL_COUNT {
                cadences.push(model.walk_duration.to_bits());
            }
        }
        cadences.sort();
        cadences.dedup();
        assert_eq!(cadences.len(), 6);
        let summer = &models[PLAYER_SUMMER_MODEL];
        let winter = &models[PLAYER_WINTER_MODEL];
        for (index, outfit) in [(PLAYER_SUMMER_MODEL, summer), (PLAYER_WINTER_MODEL, winter)] {
            assert!(assets()[index].len() < 2_000_000);
            let head = outfit.joint("Head");
            let head_weighted_hair = outfit.vertices.iter().zip(&outfit.skin).any(|(v, skin)| {
                v.surface[1] == 2.
                    && skin
                        .joints
                        .iter()
                        .zip(skin.weights)
                        .any(|(&joint, weight)| joint as usize == head && weight > 0.5)
            });
            assert!(
                head_weighted_hair,
                "missing strand-shaded skinned hair on outfit {index}"
            );
        }
        assert_eq!(summer.names, winter.names);
        assert_eq!(summer.walk_duration, winter.walk_duration);
        for (a, b) in summer.inverse_bind.iter().zip(&winter.inverse_bind) {
            assert!(a.abs_diff_eq(*b, 0.00001));
        }
        let transform = instance_transform(Vec3::new(20., 30., 4.), 0.);
        assert!(transform
            .transform_point3(Vec3::Y)
            .abs_diff_eq(Vec3::new(0.2, 0.3, 0.05), 1e-6));
        assert!(transform
            .transform_vector3(Vec3::Z)
            .abs_diff_eq(Vec3::X * 0.01, 1e-6));
    }
}
