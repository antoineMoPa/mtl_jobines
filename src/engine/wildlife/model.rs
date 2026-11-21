//! Merge rigid geometry below each animated joint once. Only joint matrices change during play.
use crate::dynamic_world::wildlife::{CatCoat, Species};
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::Mesh;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
struct Pose {
    scale: Vec3,
    rotation: Quat,
    position: Vec3,
}
impl Pose {
    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}
struct Joint {
    parent: usize,
    prefix: Mat4,
    rest: Pose,
}
enum Values {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}
struct Track {
    joint: usize,
    times: Vec<f32>,
    values: Values,
}
struct Clip {
    duration: f32,
    tracks: Vec<Track>,
}
pub(super) struct Model {
    poses: Vec<Pose>,
    joints: Vec<Joint>,
    clips: Vec<Clip>,
}
struct Builder<'a> {
    binary: &'a [u8],
    targets: HashSet<usize>,
    indices: HashMap<usize, usize>,
    joints: Vec<Joint>,
    meshes: Vec<Mesh>,
}
impl Builder<'_> {
    fn visit(
        &mut self,
        node: gltf::Node<'_>,
        parent: usize,
        mut prefix: Mat4,
    ) -> Result<(), String> {
        let mut owner = parent;
        if self.targets.contains(&node.index()) {
            let (t, r, s) = node.transform().decomposed();
            owner = self.joints.len();
            self.indices.insert(node.index(), owner);
            self.joints.push(Joint {
                parent,
                prefix,
                rest: Pose {
                    position: t.into(),
                    rotation: Quat::from_array(r),
                    scale: s.into(),
                },
            });
            self.meshes.push(Mesh::default());
            prefix = Mat4::IDENTITY;
        } else {
            prefix *= Mat4::from_cols_array_2d(&node.transform().matrix());
        }
        if node.mesh().is_some() {
            crate::hermes::external_models::append_rigid_mesh(
                &node,
                prefix,
                self.binary,
                &mut self.meshes[owner].vertices,
            )?;
        }
        for child in node.children() {
            self.visit(child, owner, prefix)?;
        }
        Ok(())
    }
}
impl Model {
    pub(super) fn read(
        species: Species,
        coat: Option<CatCoat>,
    ) -> Result<(Self, Vec<Mesh>), String> {
        let file =
            gltf::Gltf::from_slice(species.model_with_coat(coat)).map_err(|e| e.to_string())?;
        let binary = file
            .blob
            .as_deref()
            .ok_or("wildlife GLB needs binary data")?;
        let targets = file
            .animations()
            .flat_map(|a| {
                a.channels()
                    .map(|c| c.target().node().index())
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut builder = Builder {
            binary,
            targets,
            indices: HashMap::new(),
            joints: vec![Joint {
                parent: 0,
                prefix: Mat4::IDENTITY,
                rest: Pose {
                    position: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            }],
            meshes: vec![Mesh::default()],
        };
        for root in file
            .default_scene()
            .or_else(|| file.scenes().next())
            .ok_or("missing wildlife scene")?
            .nodes()
        {
            builder.visit(root, 0, Mat4::IDENTITY)?;
        }
        let mut clips = Vec::new();
        for &name in species.clips() {
            let anim = file
                .animations()
                .find(|a| a.name() == Some(name))
                .ok_or_else(|| format!("missing {name}"))?;
            let mut clip = Clip {
                duration: 0.,
                tracks: Vec::new(),
            };
            for ch in anim.channels() {
                if ch.sampler().interpolation() != gltf::animation::Interpolation::Linear {
                    return Err("wildlife clips require linear samples".into());
                }
                let reader = ch.reader(|_| Some(binary));
                let times: Vec<_> = reader
                    .read_inputs()
                    .ok_or("missing sample times")?
                    .collect();
                use gltf::animation::util::ReadOutputs;
                let values = match reader.read_outputs().ok_or("missing joint poses")? {
                    ReadOutputs::Translations(v) => {
                        Values::Translation(v.map(Vec3::from).collect())
                    }
                    ReadOutputs::Rotations(v) => {
                        Values::Rotation(v.into_f32().map(Quat::from_array).collect())
                    }
                    ReadOutputs::Scales(v) => Values::Scale(v.map(Vec3::from).collect()),
                    _ => return Err("unsupported wildlife channel".into()),
                };
                let n = match &values {
                    Values::Translation(v) | Values::Scale(v) => v.len(),
                    Values::Rotation(v) => v.len(),
                };
                if times.is_empty() || times.len() != n || times.windows(2).any(|w| w[0] >= w[1]) {
                    return Err("invalid wildlife clip samples".into());
                }
                clip.duration = clip.duration.max(*times.last().unwrap());
                let joint = *builder
                    .indices
                    .get(&ch.target().node().index())
                    .ok_or("joint outside active scene")?;
                clip.tracks.push(Track {
                    joint,
                    times,
                    values,
                });
            }
            if clip.duration <= 0. {
                return Err("empty wildlife clip".into());
            }
            clips.push(clip);
        }
        Ok((
            Self {
                poses: Vec::with_capacity(builder.joints.len()),
                joints: builder.joints,
                clips,
            },
            builder.meshes,
        ))
    }
    pub(super) fn sample(&mut self, clip: usize, seconds: f32, matrices: &mut Vec<Mat4>) {
        let clip = &self.clips[clip];
        let t = seconds.rem_euclid(clip.duration);
        self.poses.clear();
        self.poses.extend(self.joints.iter().map(|j| j.rest));
        for track in &clip.tracks {
            let hi = track
                .times
                .partition_point(|v| *v <= t)
                .min(track.times.len() - 1);
            let lo = hi.saturating_sub(1);
            let f = if lo == hi {
                0.
            } else {
                ((t - track.times[lo]) / (track.times[hi] - track.times[lo])).clamp(0., 1.)
            };
            let pose = &mut self.poses[track.joint];
            match &track.values {
                Values::Translation(v) => pose.position = v[lo].lerp(v[hi], f),
                Values::Rotation(v) => pose.rotation = v[lo].slerp(v[hi], f),
                Values::Scale(v) => pose.scale = v[lo].lerp(v[hi], f),
            }
        }
        matrices.clear();
        matrices.push(Mat4::IDENTITY);
        for (joint, pose) in self.joints.iter().zip(&self.poses).skip(1) {
            matrices.push(matrices[joint.parent] * joint.prefix * pose.matrix());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walking_paws_push_back_on_the_ground_and_recover_forward() {
        for (species, coat) in [
            (Species::Cat, Some(CatCoat::White)),
            (Species::Cat, Some(CatCoat::Black)),
            (Species::Fox, None),
            (Species::Skunk, None),
        ] {
            let (mut model, meshes) = Model::read(species, coat).unwrap();
            let mut pose = Vec::new();
            model.sample(1, 0., &mut pose);

            // These three rigs stand on all fours at the start of their stationary clip.
            // Track the lowest actual mesh vertex in each quadrant through the moving clip.
            let mut soles = [(f32::INFINITY, 0, Vec3::ZERO); 4];
            for (joint, mesh) in meshes.iter().enumerate() {
                for vertex in &mesh.vertices {
                    let [x, y, z] = vertex.position;
                    let local = Vec3::new(x, z, -y); // GPU coordinates back to model space.
                    let rest = pose[joint].transform_point3(local);
                    let quadrant = 2 * usize::from(rest.x > 0.) + usize::from(rest.z > 0.);
                    if rest.y < soles[quadrant].0 {
                        soles[quadrant] = (rest.y, joint, local);
                    }
                }
            }

            let duration = model.clips[0].duration;
            for (quadrant, &(height, joint, vertex)) in soles.iter().enumerate() {
                assert!(height.abs() < 0.005, "{species:?} sole {quadrant} at rest");
                let mut grounded = 0.;
                let mut lifted = 0.;
                model.sample(0, 0., &mut pose);
                let mut previous = pose[joint].transform_point3(vertex);
                for frame in 1..=128 {
                    model.sample(0, duration * frame as f32 / 128., &mut pose);
                    let current = pose[joint].transform_point3(vertex);
                    if previous.y.max(current.y) < 0.004 {
                        grounded += current.x - previous.x;
                    } else if previous.y.min(current.y) > 0.010 {
                        lifted += current.x - previous.x;
                    }
                    previous = current;
                }
                assert!(
                    grounded < -0.025 && lifted > 0.025,
                    "{species:?} sole {quadrant}: grounded travel {grounded} m, lifted travel {lifted} m"
                );
            }
        }
    }
}
