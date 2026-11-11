//! Native source watching with debounced background compilation and atomic replacement.
use super::*;
use std::{path::Path, sync::mpsc};

const INTERVAL: Duration = Duration::from_millis(250);
#[cfg(test)]
mod tests;

#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    source: Result<Vec<u8>, String>,
    elevation: Result<Vec<u8>, String>,
    elevation_path: Result<Option<PathBuf>, String>,
    model_files: Vec<(PathBuf, Result<u64, String>)>,
}
fn model_fingerprint(path: &Path) -> Result<u64, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = 0xcbf29ce484222325_u64;
    let mut total = 0;
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(hash);
        }
        total += count;
        if total > 64 * 1024 * 1024 {
            return Err("GLB exceeds 64 MiB".into());
        }
        for byte in &buffer[..count] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
}
impl Snapshot {
    fn read(path: &Path, previous: Option<&Self>) -> Self {
        let source = std::fs::read(path).map_err(|e| e.to_string());
        let elevation_path = if let Some(previous) = previous.filter(|p| p.source == source) {
            previous.elevation_path.clone()
        } else {
            (|| {
                let bytes = source.as_ref().map_err(Clone::clone)?;
                let program =
                    opencity::parse(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                Ok(
                    opencity::heightmap::ElevationMapSpec::from_program(&program)?
                        .map(|spec| path.parent().unwrap_or(Path::new(".")).join(spec.path)),
                )
            })()
        };
        let elevation = match &elevation_path {
            Ok(Some(path)) => std::fs::read(path).map_err(|e| e.to_string()),
            Ok(None) => Ok(Vec::new()),
            Err(error) => Err(error.clone()),
        };
        let model_paths = if let Some(previous) = previous.filter(|p| p.source == source) {
            previous
                .model_files
                .iter()
                .map(|(p, _)| p.clone())
                .collect()
        } else {
            source
                .as_ref()
                .ok()
                .and_then(|s| std::str::from_utf8(s).ok())
                .and_then(|s| opencity::parse(s).ok())
                .and_then(|p| {
                    crate::hermes::external_models::paths(
                        &p,
                        path.parent().unwrap_or(Path::new(".")),
                    )
                    .ok()
                })
                .unwrap_or_default()
        };
        let model_files = model_paths
            .into_iter()
            .map(|p| {
                let contents = model_fingerprint(&p);
                (p, contents)
            })
            .collect();
        Self {
            source,
            elevation,
            elevation_path,
            model_files,
        }
    }
}

pub(super) struct LiveReload {
    path: PathBuf,
    observed: Snapshot,
    changed_at: Option<Instant>,
    next_check: Instant,
    worker: Option<mpsc::Receiver<(Snapshot, Result<Hermes, String>)>>,
}
impl LiveReload {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            observed: Snapshot::read(&path, None),
            path,
            changed_at: None,
            next_check: Instant::now() + INTERVAL,
            worker: None,
        }
    }
    pub(super) fn next_check(&self) -> Instant {
        self.next_check
    }
    fn poll(&mut self, now: Instant) -> Option<Result<Hermes, String>> {
        if now < self.next_check {
            return None;
        }
        self.next_check = now + INTERVAL;
        let snapshot = Snapshot::read(&self.path, Some(&self.observed));
        if snapshot != self.observed {
            self.observed = snapshot;
            self.changed_at = Some(now);
        }
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok((snapshot, result)) => {
                    self.worker = None;
                    // An edit arriving during compilation supersedes this result.
                    if snapshot == self.observed {
                        return Some(result);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    return Some(Err("Map reload worker stopped unexpectedly".into()));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.worker.is_none() && self.changed_at.is_some_and(|at| now - at >= INTERVAL) {
            self.changed_at = None;
            let snapshot = self.observed.clone();
            let path = self.path.clone();
            let (sender, receiver) = mpsc::channel();
            self.worker = Some(receiver);
            std::thread::spawn(move || {
                let result = (|| {
                    Hermes::from_map_bytes(
                        snapshot.source.as_ref().map_err(Clone::clone)?,
                        snapshot.elevation.as_ref().map_err(Clone::clone)?,
                        None,
                        path.parent(),
                    )
                })()
                .map(|mut hermes| {
                    hermes.prepare_for_reload();
                    hermes
                });
                let _ = sender.send((snapshot, result));
            });
        }
        None
    }
}

impl Game {
    pub(super) fn poll_map_reload(&mut self, now: Instant) {
        let Some(result) = self
            .live_reload
            .as_mut()
            .and_then(|watcher| watcher.poll(now))
        else {
            return;
        };
        let result = result.and_then(|hermes| self.apply_reloaded_map(hermes));
        if let Err(error) = &result {
            eprintln!("Map reload failed; keeping the current map: {error}");
        } else {
            eprintln!("Map reloaded");
        }
        if let Some(window) = &self.window {
            window.set_title(if result.is_ok() {
                "MTL Jobines · map reloaded"
            } else {
                "MTL Jobines · map reload failed (see terminal)"
            });
            window.request_redraw();
        }
    }

    fn apply_reloaded_map(&mut self, mut hermes: Hermes) -> Result<(), String> {
        let position = self.world.player_position() / METERS_PER_UNIT;
        let start = SpawnPoint {
            point: [position.x, position.y],
            heading: Some(self.world.player_yaw()),
        };
        // Validate a supported player spawn before touching any active state.
        let mut world = DynamicWorld::with_start(&hermes, Some(start))
            .or_else(|_| DynamicWorld::with_start(&hermes, None))?;
        world.retain_player_vehicle(&self.world);
        if !self.world.is_driving() {
            world.toggle_vehicle(&hermes);
        }
        let p = world.player_position() / METERS_PER_UNIT;
        let mesh_center = [p.x, p.y];
        if let Some(renderer) = &mut self.renderer {
            self.scenery = Some(Scenery::new(renderer, &mut hermes));
            self.dynamic_vertices = Mesh::default();
            world.write_moving_mesh(&mut self.dynamic_vertices);
            self.dynamic_mesh = Some(renderer.upload_dynamic(&self.dynamic_vertices));
            self.prop_scenery = PropScenery::default();
            self.prop_scenery.update(renderer, &world);
            let mut vehicles = VehicleScenery::new(renderer);
            vehicles.update(renderer, &world);
            self.vehicle_scenery = Some(vehicles);
            if let Some(pixels) = hermes.update_minimap(mesh_center) {
                renderer.update_minimap(pixels);
            }
            renderer.update_route(&[]);
        }
        if let Some(palette) = &mut self.palette {
            palette.update_landmarks(&hermes.landmarks());
        }
        self.audio.silence();
        self.sound_eye = None;
        self.hermes = hermes;
        self.world = world;
        self.mesh_center = mesh_center;
        self.eye = p - Vec3::new(
            self.world.player_yaw().cos(),
            self.world.player_yaw().sin(),
            0.,
        ) * 0.15
            + Vec3::Z * 0.075;
        self.route_revision = 0;
        self.keys.clear();
        self.dragging = false;
        self.accumulator = 0.;
        self.last = Instant::now();
        self.scene_dirty = true;
        Ok(())
    }
}
