//! Native GPU snapshots and synchronized driving benchmarks.
use super::app::DT;
use super::frame_rate::FrameRate;
use super::hud::Overlay;
use super::prop_scenery::PropScenery;
use super::render::Renderer;
use super::scenery::Scenery;
use crate::dynamic_world::{
    Controls, DynamicWorld, SpawnPoint, StepProfile, Vehicle, WalkControls, METERS_PER_UNIT,
};
use crate::hermes::Hermes;
use glam::Vec3;
use opencity::runtime::Mesh;
use std::path::PathBuf;
use web_time::Instant;

#[derive(Default)]
pub(super) struct SkyOverrides {
    hour: Option<f32>,
    haze: Option<f32>,
    automatic: bool,
    clock_start: f32,
}
impl SkyOverrides {
    pub(super) fn from_args(args: &[String]) -> Result<Self, String> {
        let read = |flag: &str, minimum: f32, maximum: f32| {
            args.iter()
                .position(|arg| arg == flag)
                .map(|index| {
                    let value = args
                        .get(index + 1)
                        .ok_or_else(|| format!("{flag} requires a value"))?
                        .parse::<f32>()
                        .map_err(|_| format!("invalid {flag} value"))?;
                    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                        return Err(format!("{flag} must be between {minimum} and {maximum}"));
                    }
                    Ok(value)
                })
                .transpose()
        };
        let hour = read("--sky-hour", 0., 24.)?;
        let automatic = args.iter().any(|arg| arg == "--sky-auto");
        if automatic && hour.is_some() {
            return Err("--sky-auto and --sky-hour are mutually exclusive".into());
        }
        Ok(Self {
            hour,
            haze: read("--sky-haze", 1., 10.)?,
            automatic,
            clock_start: read("--sky-clock", 0., 1200.)?.unwrap_or(0.),
        })
    }
}
pub(super) struct SmokeOptions {
    pub(super) overview: bool,
    pub(super) vehicle_lineup: bool,
    pub(super) vehicle_lamps: bool,
    pub(super) hood: bool,
    pub(super) profile: bool,
    pub(super) profile_cpu: bool,
    pub(super) realtime: bool,
    pub(super) target_120: bool,
    pub(super) night: bool,
    pub(super) start: Option<SpawnPoint>,
    pub(super) size: (u32, u32),
    pub(super) sky: SkyOverrides,
}
pub(super) fn run(
    hermes: &mut Hermes,
    output: PathBuf,
    mut options: SmokeOptions,
) -> Result<(), String> {
    options.start = options.start.or_else(|| hermes.default_start());
    let city = hermes.city();
    let spawn_car = || match options.start {
        Some(start) => Vehicle::spawn_at(city, start),
        None => Ok(Vehicle::spawn(city)),
    };
    let mut car = spawn_car()?;
    let origin = car.position;
    for _ in 0..600 {
        car.step(
            city,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            DT,
        );
    }
    if !car.position.is_finite() || car.position.distance(origin) < 5. {
        return Err(format!(
            "vehicle smoke did not travel: {:?}",
            car.position - origin
        ));
    }
    eprintln!(
        "Driving smoke: moved {:.1} m, {:.1} km/h, {:?}, collisions {}",
        car.position.distance(origin),
        car.velocity.length() * 3.6,
        car.surface,
        car.collisions
    );
    if options.profile_cpu {
        let mut world =
            DynamicWorld::with_vehicle_lineup(hermes, options.start, options.vehicle_lineup)?;
        if std::env::args().any(|arg| arg == "--profile-full-traffic") {
            world.use_full_traffic_physics();
        }
        let (frames, ticks) = if options.target_120 {
            (1440, 1)
        } else if options.realtime {
            (720, 2)
        } else {
            (240, 8)
        };
        let mut mesh = Mesh::default();
        let mut step_profile = StepProfile::default();
        let mut step_times = Vec::with_capacity(frames);
        let mut mesh_times = Vec::with_capacity(frames);
        let mut instance_times = Vec::with_capacity(frames);
        let mut prop_times = Vec::with_capacity(frames);
        let mut total_times = Vec::with_capacity(frames);
        let mut counts = [0_usize; 3];
        for _ in 0..frames {
            let frame = Instant::now();
            for _ in 0..ticks {
                world.step_profiled(
                    hermes,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    WalkControls::default(),
                    DT,
                    &mut step_profile,
                );
            }
            step_times.push(frame.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            world.write_moving_mesh(&mut mesh);
            std::hint::black_box(&mesh);
            mesh_times.push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            let instances: Vec<_> = world.vehicle_instances().collect();
            counts[0] += instances.len();
            std::hint::black_box(instances);
            instance_times.push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            let props = world.visible_prop_batches();
            counts[1] += props.len();
            std::hint::black_box(props);
            prop_times.push(started.elapsed().as_secs_f64() * 1000.);
            total_times.push(frame.elapsed().as_secs_f64() * 1000.);
            counts[2] += mesh.vertices.len();
        }
        for (label, mut times) in [
            ("simulation", step_times),
            ("moving mesh", mesh_times),
            ("vehicle transforms", instance_times),
            ("prop selection", prop_times),
            ("CPU frame total", total_times),
        ] {
            times.sort_by(f64::total_cmp);
            eprintln!(
                "{label} ({ticks} ticks/frame): median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
                times[frames / 2],
                times[frames * 95 / 100],
                times[frames - 1]
            );
        }
        eprintln!(
            "Average per frame: {:.1} vehicles, {:.1} prop batches, {:.1} moving vertices",
            counts[0] as f64 / frames as f64,
            counts[1] as f64 / frames as f64,
            counts[2] as f64 / frames as f64
        );
        for (label, ns) in [
            "setup and physics LOD",
            "traffic controls",
            "vehicle stepping",
            "vehicle contacts",
            "sound and player pedestrian",
            "props",
            "crowd",
            "debris, snow and remaining",
        ]
        .into_iter()
        .zip(step_profile.stages_ns)
        {
            eprintln!(
                "  {label}: {:.3} ms/frame mean",
                ns as f64 / frames as f64 / 1e6
            );
        }
        for (label, ns) in [
            ("crowd setup", step_profile.crowd_ns[0]),
            (
                "crowd walking excluding gait",
                step_profile.crowd_ns[1].saturating_sub(step_profile.crowd_ns[2]),
            ),
            ("crowd gait", step_profile.crowd_ns[2]),
            ("crowd obstruction check", step_profile.crowd_ns[3]),
            ("crowd ground and path movement", step_profile.crowd_ns[4]),
            ("crowd path preview", step_profile.crowd_ns[5]),
            ("crowd vehicle impact", step_profile.crowd_ns[6]),
        ] {
            eprintln!(
                "  {label}: {:.3} ms/frame mean",
                ns as f64 / frames as f64 / 1e6
            );
        }
        return Ok(());
    }
    let car = spawn_car()?;
    let p = car.position / METERS_PER_UNIT;
    let (width, height) = options.size;
    let snapshot_time = options.sky.clock_start;
    let mut renderer = pollster::block_on(Renderer::new(None, width, height))?;
    renderer.sky_settings.solar_hour = options.sky.hour;
    if let Some(haze) = options.sky.haze {
        renderer.sky_settings.turbidity = haze;
    }
    let started = Instant::now();
    let mut scenery = Scenery::new(&renderer, hermes);
    eprintln!(
        "{}: static scenery cached in {:?}",
        renderer.adapter_name,
        started.elapsed()
    );
    let mut smoke_world =
        DynamicWorld::with_vehicle_lineup(hermes, options.start, options.vehicle_lineup)?;
    if options.profile && std::env::args().any(|arg| arg == "--profile-full-traffic") {
        smoke_world.use_full_traffic_physics();
    }
    renderer.vehicle_name = smoke_world.vehicle_name_notice().map(str::to_owned);
    let vehicle_started = Instant::now();
    let mut vehicles = super::vehicle_scenery::VehicleScenery::new(&renderer);
    eprintln!(
        "Vehicle render variants cached in {:?}",
        vehicle_started.elapsed()
    );
    vehicles.update(&renderer, &smoke_world);
    let mut moving = Mesh::default();
    smoke_world.write_moving_mesh(&mut moving);
    let carmesh = renderer.upload_dynamic(&moving);
    let mut prop_scenery = PropScenery::default();
    prop_scenery.update(&renderer, &smoke_world);
    if let Some(pixels) = hermes.update_minimap([p.x, p.y]) {
        renderer.update_minimap(pixels);
    }
    // Exercise the route pipeline in the actual driving and expanded-map GPU snapshots.
    let target_uv = hermes
        .minimap_view([p.x + 4.2, p.y - 4.2], car.yaw)
        .player_uv;
    hermes.select_destination(target_uv, p.to_array());
    renderer.update_route(&hermes.route_uvs());
    let navigation = hermes.minimap_view([p.x, p.y], car.yaw);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke frame"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let f = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
    let eye = if options.hood {
        p + f * 0.009 + Vec3::Z * 0.011
    } else if options.vehicle_lineup {
        p + Vec3::new(-0.28, -0.38, 0.40)
    } else if options.overview {
        p + Vec3::new(-1.4, -1.7, 1.5)
    } else {
        p - f * 0.15 + Vec3::Z * 0.075
    };
    let look = if options.hood {
        p + f * 0.045 + Vec3::Z * 0.014
    } else if options.overview || options.vehicle_lineup {
        p
    } else {
        p + f * 0.04 + Vec3::Z * 0.014
    };
    scenery.update_graffiti(&renderer, [p.x, p.y]);
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.push(&carmesh);
    meshes.extend(prop_scenery.visible());
    meshes.extend(vehicles.visible());
    renderer.render(
        &texture.create_view(&Default::default()),
        &meshes,
        eye,
        look,
        snapshot_time,
        navigation,
    );
    renderer.snapshot(&texture, &output)?;
    eprintln!("Rendered {}", output.display());
    renderer.validate_sky_reflections()?;
    renderer.daylight = Some(0.);
    renderer.sky_settings.solar_hour = None;
    renderer.render(
        &texture.create_view(&Default::default()),
        &meshes,
        eye,
        look,
        snapshot_time,
        navigation,
    );
    let night_path = output.with_file_name(format!(
        "{}-night.ppm",
        output.file_stem().unwrap().to_string_lossy()
    ));
    renderer.snapshot(&texture, &night_path)?;
    eprintln!("Rendered {}", night_path.display());
    renderer.validate_sky_reflections()?;
    renderer.daylight = None;
    renderer.sky_settings.solar_hour = options.sky.hour;
    for (overlay, suffix) in [
        (Overlay::Start, "start"),
        (Overlay::Menu, "menu"),
        (Overlay::Map, "map"),
    ] {
        renderer.overlay = overlay;
        renderer.render(
            &texture.create_view(&Default::default()),
            &meshes,
            eye,
            look,
            snapshot_time,
            navigation,
        );
        let path = output.with_file_name(format!(
            "{}-{suffix}.ppm",
            output.file_stem().unwrap().to_string_lossy()
        ));
        renderer.snapshot(&texture, &path)?;
        eprintln!("Rendered {}", path.display());
    }

    // Keep a zoomed map snapshot to exercise route/marker projection with the real shader.
    let span = (width as f32 - 80.).min(height as f32 - 120.);
    let player_on_map = [
        width as f32 * 0.5 - (navigation.player_uv[1] - 0.5) * span,
        height as f32 * 0.5 + 20. + (navigation.player_uv[0] - 0.5) * span,
    ];
    renderer
        .full_map
        .zoom_at(width as f32, height as f32, player_on_map, 5.);
    renderer.render(
        &texture.create_view(&Default::default()),
        &meshes,
        eye,
        look,
        snapshot_time,
        navigation,
    );
    let zoomed_path = output.with_file_name(format!(
        "{}-map-zoom.ppm",
        output.file_stem().unwrap().to_string_lossy()
    ));
    renderer.snapshot(&texture, &zoomed_path)?;
    eprintln!("Rendered {}", zoomed_path.display());
    for (selection, suffix) in [
        (super::hud::MapSelection::Jump, "map-jump"),
        (super::hud::MapSelection::JumpBlocked, "map-jump-blocked"),
    ] {
        renderer.full_map.selection = selection;
        renderer.render(
            &texture.create_view(&Default::default()),
            &meshes,
            eye,
            look,
            snapshot_time,
            navigation,
        );
        let path = output.with_file_name(format!(
            "{}-{suffix}.ppm",
            output.file_stem().unwrap().to_string_lossy()
        ));
        renderer.snapshot(&texture, &path)?;
        eprintln!("Rendered {}", path.display());
    }
    renderer.full_map = Default::default();
    renderer.overlay = Overlay::Driving;
    renderer.render(
        &texture.create_view(&Default::default()),
        &meshes,
        eye,
        look,
        snapshot_time,
        navigation,
    );
    super::palette::smoke(&renderer, &texture.create_view(&Default::default()));
    let palette_path = output.with_file_name(format!(
        "{}-palette.ppm",
        output.file_stem().unwrap().to_string_lossy()
    ));
    renderer.snapshot(&texture, &palette_path)?;
    eprintln!("Rendered {}", palette_path.display());

    drop(meshes);
    if smoke_world.toggle_vehicle(hermes) {
        renderer.vehicle_name = None;
        smoke_world.write_moving_mesh(&mut moving);
        let walking_mesh = renderer.upload_dynamic(&moving);
        let mut walking_props = PropScenery::default();
        walking_props.update(&renderer, &smoke_world);
        scenery.update_graffiti(&renderer, [p.x, p.y]);
        let mut walking_meshes = scenery.visible([p.x, p.y]);
        walking_meshes.push(&walking_mesh);
        walking_meshes.extend(walking_props.visible());
        walking_meshes.extend(vehicles.visible());
        renderer.overlay = Overlay::Driving;
        renderer.render(
            &texture.create_view(&Default::default()),
            &walking_meshes,
            eye,
            look,
            snapshot_time,
            navigation,
        );
        let path = output.with_file_name(format!(
            "{}-walking.ppm",
            output.file_stem().unwrap().to_string_lossy()
        ));
        renderer.snapshot(&texture, &path)?;
        eprintln!("Rendered {}", path.display());
    }
    renderer.overlay = Overlay::Driving;
    // Close rear views exercise the actual dynamic lamp and name HUD paths.
    // Keep these separate from the benchmark's vehicle/scene buffers.
    let mut lamp_world = match options.start {
        Some(start) => DynamicWorld::with_start(hermes, Some(start))?,
        None => DynamicWorld::new(hermes),
    };
    let mut lamp_vehicles = super::vehicle_scenery::VehicleScenery::new(&renderer);
    let model_count = if options.vehicle_lamps {
        crate::dynamic_world::catalog::definitions().len()
    } else {
        1
    };
    for model in 0..model_count {
        for (suffix, controls) in [
            ("idle", Controls::default()),
            (
                "brake",
                Controls {
                    brake: true,
                    ..Default::default()
                },
            ),
            (
                "reverse",
                Controls {
                    throttle: -1.,
                    ..Default::default()
                },
            ),
        ] {
            if options.vehicle_lamps {
                lamp_world.preview_vehicle_lamps(model, controls);
            } else {
                lamp_world.step(hermes, controls, WalkControls::default(), DT);
            }
            lamp_world.write_moving_mesh(&mut moving);
            let lamp_mesh = renderer.upload_dynamic(&moving);
            lamp_vehicles.update(&renderer, &lamp_world);
            let mut lamp_props = PropScenery::default();
            lamp_props.update(&renderer, &lamp_world);
            let car = lamp_world.player();
            let at = car.position / METERS_PER_UNIT;
            let forward = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
            let rear_distance = (car.length() * 0.5 + 4.) / METERS_PER_UNIT;
            let lamp_eye = lamp_world.camera_eye(
                hermes,
                at + Vec3::Z * 0.014,
                at - forward * rear_distance + Vec3::Z * 0.025,
            );
            scenery.update_graffiti(&renderer, [at.x, at.y]);
            let mut lamp_meshes = scenery.visible([at.x, at.y]);
            lamp_meshes.push(&lamp_mesh);
            lamp_meshes.extend(lamp_props.visible());
            lamp_meshes.extend(lamp_vehicles.visible());
            renderer.vehicle_name = lamp_world.vehicle_name_notice().map(str::to_owned);
            renderer.daylight = Some(0.);
            renderer.render(
                &texture.create_view(&Default::default()),
                &lamp_meshes,
                lamp_eye,
                at + Vec3::Z * 0.004,
                snapshot_time,
                hermes.minimap_view([at.x, at.y], car.yaw),
            );
            let stem = output.file_stem().unwrap().to_string_lossy();
            let path = output.with_file_name(if options.vehicle_lamps {
                format!("{stem}-model-{model:02}-{suffix}.ppm")
            } else {
                format!("{stem}-{suffix}.ppm")
            });
            renderer.snapshot(&texture, &path)?;
            eprintln!("Rendered {}", path.display());
        }
    }
    renderer.vehicle_name = None;
    if options.profile {
        renderer.daylight = if options.sky.automatic {
            None
        } else {
            Some(if options.night { 0. } else { 1. })
        };
        let sky_updates = renderer.sky_update_count();
        scenery.update_graffiti(&renderer, [p.x, p.y]);
        let mut meshes = scenery.visible([p.x, p.y]);
        meshes.push(&carmesh);
        meshes.extend(prop_scenery.visible());
        meshes.extend(vehicles.visible());
        let stats = renderer.visibility_stats(&meshes, eye, look, snapshot_time);
        eprintln!(
            "Visibility at spawn: {} batches / {} vertices; \
             camera {} batches / {} vertices; shadow {} batches / {} vertices",
            stats.candidates,
            stats.candidate_vertices,
            stats.camera_draws,
            stats.camera_vertices,
            stats.shadow_draws,
            stats.shadow_vertices
        );
        eprintln!(
            "Render detail {:?}: camera {} / shadow {} buffer vertices \
             (including per-instance copies)",
            renderer.render_lod, stats.camera_vertex_buffers, stats.shadow_vertex_buffers
        );
        smoke_world.reset_player(hermes);
        let origin = smoke_world.player().position;
        let (frames, ticks) = if options.target_120 {
            (1440, 1)
        } else if options.realtime {
            (720, 2)
        } else {
            (240, 8)
        };
        let budget = if options.target_120 {
            1000. / 120.
        } else {
            1000. / 60.
        };
        let mut timings = Vec::new();
        let mut simulation = Vec::new();
        let mut geometry = Vec::new();
        let mut rendering = Vec::new();
        let mut sky_frames = Vec::new();
        let mut gpu_stages = [Vec::new(), Vec::new(), Vec::new()];
        drop(meshes);
        let mut profile_mesh = renderer.upload_dynamic(&moving);
        let mut profile_props = PropScenery::default();
        profile_props.update(&renderer, &smoke_world);
        let mut profile_vertices = Mesh::default();
        let mut frame_rate = FrameRate::default();
        let mut physics_counts = [0_usize; 3];
        for frame in 0..frames {
            let sky_before_frame = renderer.sky_update_count();
            let frame_started = Instant::now();
            for _ in 0..ticks {
                smoke_world.step(
                    hermes,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    WalkControls::default(),
                    DT,
                );
            }
            simulation.push(frame_started.elapsed().as_secs_f64() * 1000.);
            let car = smoke_world.player();
            let p = car.position / METERS_PER_UNIT;
            let center = [p.x, p.y];
            let started = Instant::now();
            assert!(
                hermes.update_minimap(center).is_none(),
                "atlas must never rebuild while driving"
            );
            smoke_world.write_moving_mesh(&mut profile_vertices);
            renderer.update_dynamic(&mut profile_mesh, &profile_vertices);
            vehicles.update(&renderer, &smoke_world);
            profile_props.update(&renderer, &smoke_world);
            scenery.update_graffiti(&renderer, center);
            let mut meshes = scenery.visible(center);
            meshes.push(&profile_mesh);
            meshes.extend(profile_props.visible());
            meshes.extend(vehicles.visible());
            geometry.push(started.elapsed().as_secs_f64() * 1000.);
            let render_started = Instant::now();
            let f = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
            renderer.fps = frame_rate.value;
            renderer.render(
                &texture.create_view(&Default::default()),
                &meshes,
                p - f * 0.15 + Vec3::Z * 0.075,
                p + f * 0.04 + Vec3::Z * 0.014,
                snapshot_time + frame as f32 * ticks as f32 * DT,
                hermes.minimap_view(center, car.yaw),
            );
            renderer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|e| e.to_string())?;
            rendering.push(render_started.elapsed().as_secs_f64() * 1000.);
            let frame_ms = frame_started.elapsed().as_secs_f64() * 1000.;
            timings.push(frame_ms);
            sky_frames.push((
                frame,
                frame_ms,
                renderer.sky_update_count() > sky_before_frame,
            ));
            if let Some(profile) = &renderer.gpu_profile {
                let samples =
                    profile.read(&renderer.device, renderer.queue.get_timestamp_period())?;
                for (stage, sample) in gpu_stages.iter_mut().zip(samples) {
                    stage.push(sample);
                }
            }
            frame_rate.presented(Instant::now());
            for (count, current) in physics_counts
                .iter_mut()
                .zip(smoke_world.traffic_physics_counts())
            {
                *count += current;
            }
        }
        eprintln!(
            "Traffic physics average: {:.1} full, {:.1} lane, {:.1} sleeping vehicles/frame",
            physics_counts[0] as f64 / frames as f64,
            physics_counts[1] as f64 / frames as f64,
            physics_counts[2] as f64 / frames as f64,
        );
        let path = output.with_file_name(format!(
            "{}-profile.ppm",
            output.file_stem().unwrap().to_string_lossy()
        ));
        renderer.snapshot(&texture, &path)?;
        eprintln!("Rendered {} (unpaced profiling FPS)", path.display());
        eprintln!(
            "Sky environment updates during profile: {}",
            renderer.sky_update_count() - sky_updates
        );
        for refreshed in [false, true] {
            let mut samples: Vec<_> = sky_frames
                .iter()
                .filter(|sample| sample.2 == refreshed)
                .map(|sample| sample.1)
                .collect();
            if !samples.is_empty() {
                samples.sort_by(f64::total_cmp);
                eprintln!(
                    "  sky {}: {} frames; median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                    if refreshed { "refreshed" } else { "cached" },
                    samples.len(),
                    samples[samples.len() / 2],
                    samples[samples.len() * 95 / 100],
                    samples[samples.len() - 1]
                );
            }
        }
        sky_frames.sort_by(|a, b| b.1.total_cmp(&a.1));
        eprintln!(
            "  slowest frames (index, ms, sky refreshed): {:?}",
            &sky_frames[..frames.min(5)]
        );
        timings.sort_by(f64::total_cmp);
        eprintln!(
            "Cached driving: {frames} synchronized GPU frames, {ticks} physics ticks/frame, \
             {width}x{height}, {}: {:.1} m traveled; median {:.2} ms, p95 {:.2} ms, \
             p99 {:.2} ms, max {:.2} ms; {}/{} frames over {budget:.2} ms; no static map rebuilds",
            if options.night { "night" } else { "day" },
            smoke_world.player().position.distance(origin),
            timings[frames / 2],
            timings[frames * 95 / 100],
            timings[frames * 99 / 100],
            timings[frames - 1],
            timings.iter().filter(|&&t| t > budget).count(),
            frames
        );
        for (label, mut samples) in [
            ("simulation", simulation),
            ("mesh/upload/culling", geometry),
            ("render + GPU wait", rendering),
        ] {
            samples.sort_by(f64::total_cmp);
            eprintln!(
                "  {label}: median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                samples[frames / 2],
                samples[frames * 95 / 100],
                samples[frames - 1]
            );
        }
        for (label, mut samples) in ["shadow GPU", "world GPU", "HUD GPU"]
            .into_iter()
            .zip(gpu_stages)
        {
            if !samples.is_empty() {
                samples.sort_by(f64::total_cmp);
                eprintln!(
                    "  {label}: median {:.2} ms, p95 {:.2} ms",
                    samples[frames / 2],
                    samples[frames * 95 / 100]
                );
            }
        }
    }
    Ok(())
}
