use super::*;
fn bridge() -> Arc<Bridge> {
    Arc::new(Bridge {
        latest: Mutex::new(Frame::default()),
        active: AtomicBool::new(true),
        revision: AtomicU64::new(1),
        reset: AtomicBool::new(false),
    })
}
fn publish(bridge: &Bridge, frame: Frame) {
    *bridge.latest.lock().unwrap() = frame;
    bridge.revision.fetch_add(1, Ordering::Release);
}
fn render(mix: &mut Mix, frames: usize) -> f32 {
    let mut energy = 0.;
    for sample in mix.take(frames * 2) {
        assert!(sample.is_finite() && sample.abs() < 1.);
        energy += sample * sample;
    }
    energy
}
#[test]
fn maximum_scene_is_bounded_finite_and_gears_interrupt_load() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    let mut frame = Frame {
        settings: Settings {
            master: 1.,
            engines: 1.,
            tires: 1.,
            ambience: 1.,
            impacts: 1.,
            horns: 1.,
        },
        wind_speed: 70.,
        foliage: 1.,
        ..Frame::default()
    };
    for i in 0..CAR_VOICES {
        frame.cars[i] = Car {
            id: i,
            position: Vec3::new(i as f32, -2., 0.),
            velocity: Vec3::new(16., 8., 0.),
            throttle: 1.,
            handbrake: true,
            grounded: true,
            engine_on: true,
            player: i == 0,
            horn: true,
            ..Car::default()
        };
    }
    let mut energy = 0.;
    for tick in 0..30 {
        for i in 0..IMPACT_VOICES {
            frame.impacts[i] = Impact {
                serial: (tick * 3 + i + 1) as u64,
                position: Vec3::new(1., 0., 0.),
                speed: 50.,
                broken: true,
                material: Material::Fence,
                ..Impact::default()
            };
        }
        publish(&bridge, frame);
        energy += render(&mut mix, RATE as usize / 10);
    }
    assert!(energy > 1.);
    assert!(mix.voices[0].gear > 0);
    assert_eq!(mix.voices.len(), 15);
    assert!(mix.impact_life.iter().filter(|life| **life > 0.).count() <= 3);
    assert!(mix.voices[0].shifting <= 0.22);
}
#[test]
fn pause_is_exact_silence_and_old_impacts_do_not_retrigger() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    render(&mut mix, BLOCK);
    let mut frame = Frame::default();
    frame.impacts[0] = Impact {
        serial: 3,
        speed: 10.,
        position: Vec3::X,
        broken: true,
        ..Impact::default()
    };
    publish(&bridge, frame);
    assert!(render(&mut mix, BLOCK * 3) > 0.);
    let remembered = mix.last_impacts;
    bridge.active.store(false, Ordering::Release);
    assert_eq!(render(&mut mix, 1000), 0.);
    assert_eq!(mix.impact_life, [0.; 3]);
    bridge.active.store(true, Ordering::Release);
    publish(&bridge, frame);
    render(&mut mix, BLOCK * 4);
    assert_eq!(mix.last_impacts, remembered);
    assert_eq!(mix.impact_life, [0.; 3]);
}
#[test]
fn latest_frame_has_no_backlog_and_callback_never_waits_for_publisher() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    for n in 0..10000 {
        publish(
            &bridge,
            Frame {
                wind_speed: n as f32,
                ..Frame::default()
            },
        );
    }
    render(&mut mix, BLOCK);
    assert_eq!(mix.frame.wind_speed, 9999.);
    let lock = bridge.latest.lock().unwrap();
    // A blocking callback lock would deadlock this same-thread test.
    render(&mut mix, BLOCK * 2);
    drop(lock);
    render(&mut mix, RATE as usize);
    assert_eq!(render(&mut mix, 100), 0., "stale scene must time out");
    let updates = mix.control_updates;
    assert_eq!(render(&mut mix, RATE as usize * 20), 0.);
    assert_eq!(
        updates, mix.control_updates,
        "stalled output must not enqueue more controller commands"
    );
    publish(&bridge, Frame::default());
    assert!(render(&mut mix, BLOCK * 8) > 0.);
}
#[test]
fn category_mute_updates_existing_breakage_tail() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    render(&mut mix, BLOCK);
    let mut frame = Frame::default();
    frame.impacts[0] = Impact {
        serial: 3,
        speed: 12.,
        position: Vec3::X,
        broken: true,
        ..Impact::default()
    };
    publish(&bridge, frame);
    render(&mut mix, BLOCK);
    assert!(mix.voices[CAR_VOICES..CAR_VOICES + 3]
        .iter()
        .any(|v| get(&v.parameters.gain) > 0.));
    frame.settings.impacts = 0.;
    publish(&bridge, frame);
    render(&mut mix, BLOCK);
    assert!(mix.voices[CAR_VOICES..CAR_VOICES + 3]
        .iter()
        .all(|v| get(&v.parameters.gain) == 0.));
}
fn sine(position: [f32; 3], velocity: [f32; 3]) -> Vec<f32> {
    let (stream, _) = bstream(
        rodio::source::SineWave::new(440),
        BstreamConfig::new()
            .with_position(position)
            .with_velocity(velocity),
    );
    BstreamStereoRenderer::new(stream, StereoConfig::default())
        .take(48000)
        .collect()
}
fn channel_energy(samples: &[f32], channel: usize) -> f32 {
    samples.iter().skip(channel).step_by(2).map(|x| x * x).sum()
}
fn crossings(samples: &[f32]) -> usize {
    let mono: Vec<_> = samples.iter().step_by(2).collect();
    mono.windows(2)
        .filter(|s| *s[0] <= 0. && *s[1] > 0.)
        .count()
}
#[test]
fn ambisonic_axes_distance_and_single_doppler_follow_real_motion() {
    let right = sine([10., 0., 0.], [0.; 3]);
    let left = sine([-10., 0., 0.], [0.; 3]);
    assert!(channel_energy(&right, 1) > channel_energy(&right, 0));
    assert!(channel_energy(&left, 0) > channel_energy(&left, 1));
    let near = sine([0., 1., 0.], [0.; 3]);
    let far = sine([0., 10., 0.], [0.; 3]);
    assert!((channel_energy(&near, 0) / channel_energy(&far, 0) - 100.).abs() < 0.01);
    let approaching = sine([0., 10., 0.], [0., -40., 0.]);
    let receding = sine([0., 10., 0.], [0., 40., 0.]);
    assert!(crossings(&approaching) > crossings(&near));
    assert!(crossings(&receding) < crossings(&near));
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    let mut frame = Frame::default();
    frame.cars[0] = Car {
        id: 1,
        position: Vec3::ZERO,
        engine_on: true,
        ..Car::default()
    };
    publish(&bridge, frame);
    render(&mut mix, BLOCK * 3); // zero distance must stay finite
}
#[test]
fn overtaking_keeps_car_voice_and_gear_identity() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    let mut frame = Frame::default();
    frame.cars[0] = Car {
        id: 7,
        position: Vec3::X * 10.,
        velocity: Vec3::X * 18.,
        engine_on: true,
        ..Car::default()
    };
    frame.cars[1] = Car {
        id: 9,
        position: Vec3::X * 12.,
        velocity: Vec3::X * 18.,
        engine_on: true,
        ..Car::default()
    };
    publish(&bridge, frame);
    render(&mut mix, BLOCK * 5);
    let gear = mix.voices[0].gear;
    let shifting = mix.voices[0].shifting;
    frame.cars.swap(0, 1);
    publish(&bridge, frame);
    render(&mut mix, BLOCK);
    assert_eq!(mix.voices[0].id, 7);
    assert_eq!(mix.voices[1].id, 9);
    assert_eq!(mix.voices[0].gear, gear);
    assert!(mix.voices[0].shifting < shifting);
}
#[test]
fn material_changes_broadband_spectrum_and_debris_duration_at_equal_gain() {
    fn texture(material: Material) -> (f32, f32) {
        let p = Arc::new(Parameters::default());
        put(&p.gain, 1.);
        impact_parameters(&p, material, true);
        p.impact.store(1, Ordering::Relaxed);
        let mut synth = Synth::new(p, CAR_VOICES, 91827);
        let mut previous = 0.;
        let mut energy = 0.;
        let mut difference = 0.;
        let mut tail = 0.;
        for i in 0..RATE {
            let sample = synth.sample();
            energy += sample * sample;
            difference += (sample - previous) * (sample - previous);
            if i > RATE / 2 {
                tail += sample * sample;
            }
            previous = sample;
        }
        (difference / energy, tail / energy)
    }
    let fence = texture(Material::Fence);
    let timber = texture(Material::Timber);
    let hard = texture(Material::Hard);
    assert!(
        fence.0 > timber.0 * 1.3,
        "wire/fence debris needs a broader noise spectrum than timber: {fence:?} {timber:?}"
    );
    assert!(
        fence.1 > hard.1 * 2.,
        "breakable fence debris must outlast a hard contact: {fence:?} {hard:?}"
    );
    let lamp = texture(Material::Lamp);
    let body = texture(Material::Body);
    assert!((lamp.0 - body.0).abs() > 0.01);
}
#[test]
fn drivetrain_shifts_reduce_load_and_authored_profiles_change_idle_and_timbre() {
    let mut voice = Spatial::new(0);
    let mut car = Car {
        id: 1,
        velocity: Vec3::X * 18.,
        throttle: 1.,
        engine_on: true,
        ..Car::default()
    };
    voice.car(car, &Frame::default());
    assert_eq!(voice.gear, 1);
    assert!(voice.shifting > 0. && get(&voice.parameters.load) < 0.2);
    car.velocity = Vec3::ZERO;
    car.throttle = 0.;
    car.sound.idle_rpm = 820.;
    car.sound.body_gain = 0.15;
    voice.car(car, &Frame::default());
    let compact = (
        get(&voice.parameters.rpm),
        get(&voice.parameters.engine_body),
    );
    car.sound.idle_rpm = 520.;
    car.sound.body_gain = 0.65;
    voice.car(car, &Frame::default());
    let heavy = (
        get(&voice.parameters.rpm),
        get(&voice.parameters.engine_body),
    );
    assert!(compact.0 > heavy.0 && compact.1 < heavy.1);
}

#[test]
fn held_horn_uses_authored_frequencies_releases_and_obeys_pause() {
    let mut voice = Spatial::new(0);
    let mut car = Car {
        id: 1,
        horn: true,
        player: true,
        position: Vec3::X,
        ..Car::default()
    };
    car.sound.horn.frequencies_hz = [185., 240.];
    car.sound.horn.kind = HornKind::Air;
    let mut frame = Frame::default();
    frame.settings.engines = 0.;
    frame.settings.tires = 0.;
    frame.settings.horns = 1.;
    frame.settings.ambience = 0.;
    voice.car(car, &frame);
    let parameters = voice.parameters.clone();
    let mut synth = Synth::new(parameters.clone(), 0, 91827);
    let samples: Vec<_> = (0..RATE / 4).map(|_| synth.sample()).collect();
    let power = |hz: f32| {
        let (mut re, mut im) = (0., 0.);
        for (i, value) in samples.iter().enumerate().skip(1000) {
            let phase = std::f32::consts::TAU * hz * i as f32 / RATE as f32;
            re += value * phase.cos();
            im += value * phase.sin();
        }
        re * re + im * im
    };
    assert!(power(185.) > power(310.) * 100.);
    assert!(power(240.) > power(310.) * 100.);
    assert!(samples.iter().all(|sample| sample.is_finite()));
    let held = synth.horn_level;
    car.horn = false;
    voice.car(car, &frame);
    for _ in 0..RATE / 4 {
        synth.sample();
    }
    assert!(
        synth.horn_level < held * 0.001,
        "release must not leave a held horn"
    );
    car.horn = true;
    voice.car(car, &frame);
    frame.settings.horns = 0.;
    voice.car(car, &frame);
    assert_eq!(get(&parameters.horn_gain), 0.);
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    frame.cars[0] = car;
    frame.settings.horns = 1.;
    publish(&bridge, frame);
    assert!(render(&mut mix, RATE as usize / 5) > 0.);
    bridge.active.store(false, Ordering::Release);
    assert_eq!(render(&mut mix, RATE as usize / 5), 0.);
    frame.cars[0].horn = false;
    publish(&bridge, frame);
    bridge.active.store(true, Ordering::Release);
    assert_eq!(
        render(&mut mix, RATE as usize / 5),
        0.,
        "resuming must not replay the held horn's frozen envelope"
    );
}

#[test]
fn engine_presence_increases_without_raising_road_noise_and_electric_avoids_shifts() {
    fn energy(sound: VehicleSound, settings: Settings) -> f32 {
        let bridge = bridge();
        let mut mix = Mix::new(bridge.clone());
        let mut frame = Frame {
            settings,
            ..Frame::default()
        };
        frame.cars[0] = Car {
            id: 1,
            position: Vec3::X * 10.,
            throttle: 0.7,
            engine_on: true,
            player: true,
            sound,
            ..Car::default()
        };
        publish(&bridge, frame);
        render(&mut mix, RATE as usize / 5)
    }
    let mut settings = Settings::default();
    settings.ambience = 0.;
    settings.tires = 0.;
    let louder = energy(VehicleSound::default(), settings);
    settings.master = 0.55;
    settings.engines = 0.7;
    let quieter = energy(
        VehicleSound {
            engine_gain: 1.,
            ..VehicleSound::default()
        },
        settings,
    );
    assert!(
        louder > quieter * 4.,
        "default motor should gain over 6 dB of energy against the old gain settings"
    );
    let mut voice = Spatial::new(0);
    let mut car = Car {
        id: 1,
        velocity: Vec3::X * 18.,
        throttle: 1.,
        engine_on: true,
        ..Car::default()
    };
    car.sound.engine = EngineKind::Electric;
    voice.car(car, &Frame::default());
    assert_eq!(voice.gear, 0);
    assert_eq!(voice.shifting, 0.);
    car.sound.engine = EngineKind::Diesel;
    voice.car(car, &Frame::default());
    assert!(voice.shifting > 0.);
}

#[test]
fn suspended_device_reset_clears_horn_without_a_silent_callback() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    let mut frame = Frame::default();
    frame.settings.engines = 0.;
    frame.settings.tires = 0.;
    frame.settings.ambience = 0.;
    frame.cars[0] = Car {
        id: 1,
        position: Vec3::X,
        horn: true,
        ..Car::default()
    };
    publish(&bridge, frame);
    assert!(render(&mut mix, RATE as usize / 5) > 0.);
    // The device was suspended, so it never consumed an active=false sample.
    frame.cars[0].horn = false;
    publish(&bridge, frame);
    bridge.reset.store(true, Ordering::Release);
    assert_eq!(render(&mut mix, RATE as usize / 5), 0.);
}

#[test]
fn combustion_orders_stand_above_brown_noise_at_idle() {
    let mut voice = Spatial::new(0);
    let mut car = Car {
        id: 1,
        engine_on: true,
        ..Car::default()
    };
    car.sound.idle_rpm = 900.;
    car.sound.roughness = 0.;
    voice.car(car, &Frame::default());
    let mut synth = Synth::new(voice.parameters.clone(), 0, 12783);
    for _ in 0..RATE {
        synth.sample();
    }
    let samples: Vec<_> = (0..RATE).map(|_| synth.sample()).collect();
    let total: f32 = samples.iter().map(|v| v * v).sum::<f32>() / RATE as f32;
    let mut periodic = 0.;
    for hz in [15., 30., 60.] {
        let (mut re, mut im) = (0., 0.);
        for (i, sample) in samples.iter().enumerate() {
            let angle = std::f32::consts::TAU * hz * i as f32 / RATE as f32;
            re += sample * angle.cos();
            im += sample * angle.sin();
        }
        periodic += 2. * (re * re + im * im) / (RATE as f32).powi(2);
    }
    let residue = (total - periodic).max(1e-12);
    assert!(
        periodic / residue > 200.,
        "combustion should dominate its brown-noise bed: {}",
        periodic / residue
    );
}

#[test]
fn bus_retains_engine_tone_with_a_quieter_noise_bed() {
    let bus = crate::dynamic_world::catalog::definitions()
        .iter()
        .find(|definition| definition.config.category == "bus")
        .unwrap()
        .config
        .sound;
    let measure = |sound: VehicleSound| {
        let mut voice = Spatial::new(0);
        voice.car(
            Car {
                id: 1,
                engine_on: true,
                sound,
                ..Car::default()
            },
            &Frame::default(),
        );
        let mut synth = Synth::new(voice.parameters.clone(), 0, 12783);
        for _ in 0..RATE {
            synth.sample();
        }
        let samples: Vec<_> = (0..RATE).map(|_| synth.sample()).collect();
        let total = samples.iter().map(|sample| sample * sample).sum::<f32>() / RATE as f32;
        let mut periodic = 0.;
        for hz in [
            sound.idle_rpm / 60.,
            sound.idle_rpm / 60. * sound.firing_order,
            sound.idle_rpm / 60. * sound.firing_order * 2.,
        ] {
            let (mut re, mut im) = (0., 0.);
            for (i, sample) in samples.iter().enumerate() {
                let angle = std::f32::consts::TAU * hz * i as f32 / RATE as f32;
                re += sample * angle.cos();
                im += sample * angle.sin();
            }
            periodic += 2. * (re * re + im * im) / (RATE as f32).powi(2);
        }
        (periodic, (total - periodic).max(1e-12))
    };
    let (tone, noise) = measure(bus);
    let (old_tone, old_noise) = measure(VehicleSound {
        roughness: 0.6,
        machinery_gain: 0.14,
        ..bus
    });
    assert!(
        noise < old_noise * 0.6,
        "noise {noise} vs previous {old_noise}"
    );
    assert!(
        tone > old_tone * 0.9,
        "engine tone {tone} vs previous {old_tone}"
    );
}

#[test]
fn traffic_slots_fade_to_silence_before_changing_source() {
    let bridge = bridge();
    let mut mix = Mix::new(bridge.clone());
    let mut frame = Frame::default();
    for (id, car) in frame.cars.iter_mut().enumerate() {
        *car = Car {
            id,
            engine_on: true,
            position: Vec3::X * (id + 2) as f32,
            ..Car::default()
        };
    }
    publish(&bridge, frame);
    render(&mut mix, BLOCK * 8);
    assert_eq!(mix.voices[0].presence, 1.);
    frame.cars[0].id = 100;
    frame.cars[0].sound.idle_rpm = 1500.;
    publish(&bridge, frame);
    render(&mut mix, BLOCK);
    assert_eq!(mix.voices[0].id, 0, "outgoing engine must keep its profile");
    assert!(mix.voices[0].presence > 0. && mix.voices[0].presence < 1.);
    assert!(!mix.voices[0].present);
    render(&mut mix, BLOCK);
    assert_eq!(mix.voices[0].presence, 0.);
    render(&mut mix, BLOCK);
    assert_eq!(mix.voices[0].id, 100);
    assert!(mix.voices[0].present);
    assert!(mix.voices[0].presence > 0. && mix.voices[0].presence < 1.);
    assert_eq!(get(&mix.voices[0].parameters.rpm), 1500.);
}

#[test]
fn reused_engine_starts_at_requested_pitch_in_web_renderer() {
    let mut voice = Spatial::new(0);
    let mut car = Car {
        id: 1,
        engine_on: true,
        position: Vec3::X * 2.,
        ..Car::default()
    };
    voice.car(car, &Frame::default());
    let mut synth = Synth::new(voice.parameters.clone(), 0, 1789);
    for _ in 0..RATE / 10 {
        synth.sample();
    }
    car.id = 2;
    car.sound.idle_rpm = 1600.;
    voice.car(car, &Frame::default());
    synth.sample();
    assert_eq!(synth.rpm, 1600., "no inherited engine pitch sweep");
    assert!(synth.gain < 0.01, "replacement engine must fade in");
    let (mut renderer, mut controller) = super::super::spatial_web::Renderer::new(synth);
    controller.adjust_position([2., 1., 0.]);
    controller.set_velocity([0., -20., 0.]);
    let samples: Vec<_> = renderer.by_ref().take(RATE as usize / 5).collect();
    assert!(samples
        .iter()
        .all(|value| value.is_finite() && value.abs() < 1.));
    assert!(samples.iter().any(|value| value.abs() > 0.01));
}
