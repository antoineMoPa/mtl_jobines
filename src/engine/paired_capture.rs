//! Offline gameplay dataset. Both renderers see the same live world at each tick.
use super::{
    app::DT,
    prop_scenery::PropScenery,
    render::{GpuMesh, Renderer},
    scenery::Scenery,
    vehicle_scenery::VehicleScenery,
};
use crate::{
    dynamic_world::{catalog, Controls, DynamicWorld, SpawnPoint, WalkControls, METERS_PER_UNIT},
    hermes::{scenery::Source, Hermes},
    season::Season,
};
use glam::Vec3;
use opencity::runtime::Mesh;
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    sync::Arc,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FPS: u32 = 30;
const TICKS_PER_FRAME: u32 = 4;
const WARMUP_TICKS: u32 = 120;

#[derive(Clone, Copy)]
enum Scene {
    WinterCityDriving,
    SummerBiking,
    SummerBeachWalk,
}

impl Scene {
    const ALL: [Self; 3] = [
        Self::WinterCityDriving,
        Self::SummerBiking,
        Self::SummerBeachWalk,
    ];

    fn id(self) -> &'static str {
        match self {
            Self::WinterCityDriving => "winter-city-driving",
            Self::SummerBiking => "summer-biking",
            Self::SummerBeachWalk => "summer-beach-walk",
        }
    }

    fn season(self) -> Season {
        match self {
            Self::WinterCityDriving => Season::Winter,
            Self::SummerBiking | Self::SummerBeachWalk => Season::Summer,
        }
    }

    fn world(self, hermes: &Hermes) -> Result<DynamicWorld, String> {
        let start = match self {
            Self::WinterCityDriving => Some(SpawnPoint {
                point: [8., -15.85],
                heading: Some(-std::f32::consts::FRAC_PI_2),
            }),
            Self::SummerBiking => Some(SpawnPoint {
                point: [0.30, -1.70],
                heading: Some(0.03),
            }),
            Self::SummerBeachWalk => Some(SpawnPoint {
                point: [8., 15.45],
                heading: Some(0.),
            }),
        };
        let mut world = DynamicWorld::with_start(hermes, start)?;
        let model = match self {
            Self::WinterCityDriving => "vehicle-v2-09",
            Self::SummerBiking | Self::SummerBeachWalk => catalog::BICYCLE_ID,
        };
        let model_index = catalog::definitions()
            .iter()
            .position(|definition| definition.config.research_id == model)
            .ok_or_else(|| format!("Capture vehicle {model} is unavailable"))?;
        if !world.switch_vehicle(model_index) {
            return Err(format!("Could not select {model} for {}", self.id()));
        }
        world.set_season(hermes, self.season());
        if matches!(self, Self::SummerBeachWalk) && !world.toggle_vehicle(hermes) {
            return Err("Could not dismount at the beach".into());
        }
        Ok(world)
    }

    fn step(self, world: &mut DynamicWorld, hermes: &Hermes) {
        let (throttle, forward) = match self {
            Self::WinterCityDriving => (0.48, 0.),
            Self::SummerBiking => (0.75, 0.),
            Self::SummerBeachWalk => (0., 1.),
        };
        world.step(
            hermes,
            Controls {
                throttle,
                ..Default::default()
            },
            WalkControls {
                forward,
                ..Default::default()
            },
            DT,
        );
    }

    fn camera(self, world: &DynamicWorld, hermes: &Hermes) -> (Vec3, Vec3) {
        let position = world.player_position() / METERS_PER_UNIT;
        let (sin, cos) = world.player_yaw().sin_cos();
        let forward = Vec3::new(cos, sin, 0.);
        let left = Vec3::new(-forward.y, forward.x, 0.);
        let (offset, focus) = match self {
            Self::WinterCityDriving => (-forward * 0.095 + Vec3::Z * 0.041, forward * 0.04),
            Self::SummerBiking => (
                -forward * 0.053 + left * 0.015 + Vec3::Z * 0.028,
                forward * 0.037 + Vec3::Z * 0.01,
            ),
            Self::SummerBeachWalk => (
                -forward * 0.05 - left * 0.085 + Vec3::Z * 0.045,
                forward * 0.07 + left * 0.06,
            ),
        };
        (
            world.camera_eye(hermes, position, position + offset),
            position + focus,
        )
    }
}

#[derive(Clone, Copy)]
enum Quality {
    Low,
    High,
}

impl Quality {
    fn id(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
        }
    }

    fn enabled(self) -> bool {
        matches!(self, Self::High)
    }
}

struct CaptureView {
    renderer: Renderer,
    scenery: Scenery,
    vehicles: VehicleScenery,
    props: PropScenery,
    moving: GpuMesh,
    target: wgpu::Texture,
    view: wgpu::TextureView,
}

impl CaptureView {
    fn new(hermes: &Hermes, source: Arc<Source>, quality: Quality) -> Result<Self, String> {
        let mut renderer = pollster::block_on(Renderer::new(None, WIDTH, HEIGHT))?;
        renderer.ambient_occlusion = quality.enabled();
        renderer.set_antialiasing(quality.enabled());
        if renderer.antialiasing() != quality.enabled() {
            return Err("The GPU adapter does not support the requested MSAA capture".into());
        }
        renderer.cinematic = true;
        let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("paired capture target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.config.format.add_srgb_suffix(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        Ok(Self {
            scenery: Scenery::from_source(&renderer, hermes, source),
            vehicles: VehicleScenery::new(&renderer),
            moving: renderer.upload_dynamic(&Mesh::default()),
            props: PropScenery::default(),
            renderer,
            target,
            view,
        })
    }

    fn draw(&mut self, hermes: &Hermes, world: &DynamicWorld, frame: &Frame, moving: &Mesh) {
        let renderer = &mut self.renderer;
        renderer.season = world.season();
        renderer.update_dynamic(&mut self.moving, moving);
        self.vehicles.update(renderer, world);
        self.props.update(renderer, world);
        self.scenery
            .update_winter(renderer, world, hermes.city(), frame.eye, frame.time);
        renderer.prepare_grass(
            &mut self.scenery.grass,
            hermes.city(),
            frame.eye,
            frame.look,
        );
        let center = frame.position.truncate().to_array();
        self.scenery.update_graffiti(renderer, center);
        let (mut meshes, _) = self.scenery.for_frame(renderer, center, false);
        meshes.push(&self.moving);
        meshes.extend(self.vehicles.visible());
        meshes.extend(self.props.visible());
        renderer.render_scene(&self.view, &meshes, frame.eye, frame.look, frame.time, None);
    }
}

struct Frame {
    position: Vec3,
    eye: Vec3,
    look: Vec3,
    time: f32,
}

struct Encoder(Child);

impl Encoder {
    fn start(path: &Path) -> Result<Self, String> {
        Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-n",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
            ])
            .arg(format!("{WIDTH}x{HEIGHT}"))
            .arg("-framerate")
            .arg(FPS.to_string())
            .args([
                "-i", "pipe:0", "-an", "-c:v", "ffv1", "-level", "3", "-pix_fmt", "bgr0",
                "-threads", "2",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .map(Self)
            .map_err(|error| format!("Starting FFmpeg: {error}"))
    }

    fn write(&mut self, pixels: &[u8]) -> Result<(), String> {
        self.0
            .stdin
            .as_mut()
            .expect("encoder stdin is open")
            .write_all(pixels)
            .map_err(|error| format!("Writing an FFmpeg frame: {error}"))
    }

    fn finish(&mut self) -> Result<(), String> {
        let status = self.0.wait().map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("FFmpeg exited with {status}"))
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub(super) fn run(hermes: &mut Hermes, output: &Path, seconds: u32) -> Result<(), String> {
    fs::create_dir(output).map_err(|error| format!("Creating {}: {error}", output.display()))?;
    let source = Arc::new(hermes.take_scenery_source());
    let mut low = CaptureView::new(hermes, source.clone(), Quality::Low)?;
    let mut high = CaptureView::new(hermes, source, Quality::High)?;
    let frames = seconds * FPS;
    let mut scenes = Vec::new();
    for scene in Scene::ALL {
        let mut world = scene.world(hermes)?;
        let name = scene.id();
        eprintln!("Capturing {name} ({frames} frames per quality)");
        let mut moving = Mesh::default();
        let mut metadata = Vec::with_capacity(frames as usize);
        let low_name = format!("{name}-{}.mkv", Quality::Low.id());
        let high_name = format!("{name}-{}.mkv", Quality::High.id());
        let mut low_encoder = Encoder::start(&output.join(&low_name))?;
        let mut high_encoder = Encoder::start(&output.join(&high_name))?;
        for _ in 0..WARMUP_TICKS {
            scene.step(&mut world, hermes);
        }
        let center = (world.player_position() / METERS_PER_UNIT)
            .truncate()
            .to_array();
        low.scenery.preload_capture(&low.renderer, center);
        high.scenery.preload_capture(&high.renderer, center);
        for index in 0..frames {
            let (eye, look) = scene.camera(&world, hermes);
            let frame = Frame {
                position: world.player_position() / METERS_PER_UNIT,
                eye,
                look,
                time: (WARMUP_TICKS + index * TICKS_PER_FRAME) as f32 * DT,
            };
            world.write_moving_mesh(&mut moving);
            low.draw(hermes, &world, &frame, &moving);
            high.draw(hermes, &world, &frame, &moving);
            low_encoder.write(&low.renderer.rgba_pixels(&low.target)?)?;
            high_encoder.write(&high.renderer.rgba_pixels(&high.target)?)?;
            metadata.push(json!({
                "index": index, "simulation_seconds": frame.time,
                "player": frame.position.to_array(), "eye": eye.to_array(), "look": look.to_array(),
            }));
            for _ in 0..TICKS_PER_FRAME {
                scene.step(&mut world, hermes);
            }
            if (index + 1) % FPS == 0 {
                eprintln!("{name}: {}/{} seconds", (index + 1) / FPS, seconds);
            }
        }
        low_encoder.finish()?;
        high_encoder.finish()?;
        scenes.push(json!({ "id": name, "low": low_name, "high": high_name, "frames": metadata }));
    }
    let manifest = json!({
        "width": WIDTH, "height": HEIGHT, "fps": FPS, "color": "sRGB",
        "quality": {
            "low": { "ambient_occlusion": false, "msaa_samples": 1 },
            "high": { "ambient_occlusion": true, "msaa_samples": 4 },
        },
        "scenes": scenes,
    });
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    fs::write(output.join("manifest.json"), bytes).map_err(|error| error.to_string())
}
