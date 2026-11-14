//! One fallible output stream; fixed spatial voices owned by its callback.
//! ambisonic's builder unwraps device errors. Its lower-level public API lets
//! us preserve device failure as a Result and avoid its dynamic mixer queues.
use super::*;
#[cfg(not(target_arch = "wasm32"))]
use ambisonic::{
    bstream, Bstream, BstreamConfig, BstreamStereoRenderer, SoundController, StereoConfig,
};
#[cfg(test)]
use rodio;
use rodio::Source;
#[cfg(not(target_arch = "wasm32"))]
type SpatialRenderer = BstreamStereoRenderer<Bstream>;
#[cfg(target_arch = "wasm32")]
use super::spatial_web::Controller as SoundController;
#[cfg(target_arch = "wasm32")]
type SpatialRenderer = super::spatial_web::Renderer<Synth>;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
pub(super) const RATE: u32 = 24_000;
const BLOCK: usize = 256;
const OUTPUT_GAIN: f32 = 1.6;
const VOICES: usize = CAR_VOICES + IMPACT_VOICES + 2;

pub(super) struct Bridge {
    pub(super) latest: Mutex<Frame>,
    pub(super) active: AtomicBool,
    pub(super) revision: AtomicU64,
    pub(super) reset: AtomicBool,
}
#[derive(Default)]
struct Parameters {
    rpm: AtomicU32,
    load: AtomicU32,
    rolling: AtomicU32,
    scrub: AtomicU32,
    wind: AtomicU32,
    gain: AtomicU32,
    profile: AtomicU32,
    impact: AtomicU32,
    broken: AtomicBool,
    pressure_decay: AtomicU32,
    crush_decay: AtomicU32,
    debris_decay: AtomicU32,
    debris_filter: AtomicU32,
    engine_body: AtomicU32,
    engine_kind: AtomicU32,
    firing_order: AtomicU32,
    roughness: AtomicU32,
    machinery_gain: AtomicU32,
    muffler_step: AtomicU32,
    horn_gain: AtomicU32,
    horn_reset: AtomicBool,
    engine_reset: AtomicBool,
    horn_frequency_a: AtomicU32,
    horn_frequency_b: AtomicU32,
    air_horn: AtomicBool,
}
#[derive(Clone, Copy)]
struct ImpactTexture {
    pressure_decay: f32,
    crush_decay: f32,
    debris_decay: f32,
    debris_filter: f32,
}
impl Material {
    fn texture(self) -> ImpactTexture {
        let (pressure_decay, crush_decay, debris_decay, debris_filter) = match self {
            Self::Hard => (28., 18., 9., 0.13),
            Self::Body => (18., 11., 6., 0.19),
            Self::Fence => (34., 14., 3.8, 0.29),
            Self::Lamp => (26., 20., 5., 0.22),
            Self::Timber => (22., 10., 4.4, 0.12),
        };
        ImpactTexture {
            pressure_decay,
            crush_decay,
            debris_decay,
            debris_filter,
        }
    }
}
fn impact_parameters(p: &Parameters, material: Material, broken: bool) {
    let texture = material.texture();
    put(&p.pressure_decay, texture.pressure_decay);
    put(&p.crush_decay, texture.crush_decay);
    put(&p.debris_decay, texture.debris_decay);
    put(&p.debris_filter, texture.debris_filter);
    p.broken.store(broken, Ordering::Relaxed);
}
fn put(slot: &AtomicU32, value: f32) {
    slot.store(value.to_bits(), Ordering::Relaxed);
}
fn get(slot: &AtomicU32) -> f32 {
    f32::from_bits(slot.load(Ordering::Relaxed))
}
struct Synth {
    parameters: Arc<Parameters>,
    kind: usize,
    seed: u32,
    phase: f32,
    slow: f32,
    low: f32,
    mid: f32,
    output: f32,
    gain: f32,
    rpm: f32,
    impact: u32,
    age: f32,
    pulse: f32,
    horn_phase: [f32; 2],
    horn_level: f32,
}
impl Synth {
    fn new(parameters: Arc<Parameters>, kind: usize, seed: u32) -> Self {
        Self {
            parameters,
            kind,
            seed: seed.max(1),
            phase: 0.,
            slow: 0.,
            low: 0.,
            mid: 0.,
            output: 0.,
            gain: 0.,
            rpm: 750.,
            impact: 0,
            age: 5.,
            pulse: 0.,
            horn_phase: [0.; 2],
            horn_level: 0.,
        }
    }
    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2. - 1.
    }
    fn sample(&mut self) -> f32 {
        if self.parameters.engine_reset.swap(false, Ordering::Relaxed) {
            // A recycled slot starts at the new engine's pitch, not a glide
            // from the previous car. Slot reuse happens only after fade-out.
            self.rpm = get(&self.parameters.rpm);
            self.gain = 0.;
            self.output = 0.;
        }
        if self.parameters.horn_reset.load(Ordering::Relaxed) {
            self.horn_level = 0.;
            self.parameters.horn_reset.store(false, Ordering::Relaxed);
        }
        if self.gain < 0.000001 && get(&self.parameters.gain) == 0. {
            return 0.;
        }
        let noise = self.noise();
        self.slow += (noise - self.slow) * 0.008;
        self.low += (noise - self.low) * 0.055;
        self.mid += (noise - self.mid) * 0.22;
        let target_gain = get(&self.parameters.gain);
        self.gain += (target_gain - self.gain) * 0.0015;
        let mut horn = 0.;
        let raw = if self.kind < CAR_VOICES {
            let rpm = get(&self.parameters.rpm);
            self.rpm += (rpm - self.rpm) * 0.001;
            let load = get(&self.parameters.load);
            let profile = get(&self.parameters.profile);
            self.phase = (self.phase + self.rpm / 60. / RATE as f32).fract();
            let a = self.phase * std::f32::consts::TAU;
            // Low combustion orders with nonperiodic exhaust texture, damped
            // twice below. Tire branches contain noise only: no skid oscillator.
            let firing = get(&self.parameters.firing_order);
            let roughness = get(&self.parameters.roughness);
            let engine_kind = self.parameters.engine_kind.load(Ordering::Relaxed);
            let cycle = a * firing + self.slow * roughness * 0.9;
            let engine = cycle.sin() * 0.36
                + a.sin() * get(&self.parameters.engine_body)
                + (cycle * 2.).sin() * (0.05 + load * 0.04);
            let combustion = if engine_kind == 1 {
                engine * (1. + self.mid * roughness) + self.low * roughness * (0.10 + load * 0.18)
            } else if engine_kind == 2 {
                (a * 6.).sin() * 0.12 + self.low * 0.015
            } else {
                engine
            };
            let machinery = (self.mid - self.slow)
                * get(&self.parameters.machinery_gain)
                * (0.25 + load * 0.75)
                * (0.65 + 0.35 * a.sin().abs());
            let motor =
                (combustion * (0.28 + load * 0.24) + self.low * (0.025 + load * 0.045) + machinery)
                    * profile;
            let horn_target = get(&self.parameters.horn_gain);
            self.horn_level += (horn_target - self.horn_level)
                * if horn_target > self.horn_level {
                    0.005
                } else {
                    0.0012
                };
            if self.horn_level > 0.00001 {
                let air = self.parameters.air_horn.load(Ordering::Relaxed);
                for (index, frequency) in [
                    get(&self.parameters.horn_frequency_a),
                    get(&self.parameters.horn_frequency_b),
                ]
                .into_iter()
                .enumerate()
                {
                    self.horn_phase[index] =
                        (self.horn_phase[index] + frequency / RATE as f32).fract();
                    let angle = self.horn_phase[index] * std::f32::consts::TAU;
                    horn += angle.sin()
                        + (angle * 2.).sin() * if air { 0.35 } else { 0.16 }
                        + (angle * 3.).sin() * if air { 0.15 } else { 0.05 };
                }
                horn = (horn * 0.16
                    + if air {
                        (self.mid - self.low) * 0.10
                    } else {
                        0.
                    })
                    * self.horn_level;
            }
            let road = self.low * get(&self.parameters.rolling) * 0.65
                + (self.mid - self.low) * get(&self.parameters.scrub) * 0.5;
            motor + road
        } else if self.kind < CAR_VOICES + IMPACT_VOICES {
            let serial = self.parameters.impact.load(Ordering::Relaxed);
            if serial != self.impact {
                self.impact = serial;
                self.age = 0.;
                self.pulse = self.low;
            }
            self.age += 1. / RATE as f32;
            if self.age > 1.4 {
                0.
            } else {
                let broken = self.parameters.broken.load(Ordering::Relaxed);
                // Pressure, crush, then gritty scrape/debris. No pitched clank.
                self.pulse += (self.low - self.pulse) * 0.015;
                let pressure =
                    self.pulse * (-self.age * get(&self.parameters.pressure_decay)).exp() * 7.;
                let crush = (self.mid - self.slow)
                    * (-self.age * get(&self.parameters.crush_decay)).exp()
                    * 1.1;
                let tail = (self.mid - self.low)
                    * (-self.age
                        * if broken {
                            get(&self.parameters.debris_decay)
                        } else {
                            10.
                        })
                    .exp()
                    * if broken { 0.33 } else { 0.10 };
                let attack = (self.age * 1000.).min(1.);
                (pressure + crush + tail) * attack
            }
        } else {
            let wind = get(&self.parameters.wind);
            let foliage = get(&self.parameters.rolling);
            self.slow * (0.45 + wind * 1.2) + (self.low - self.slow) * (0.06 + foliage * 0.13)
        };
        // Muffler/air damping; conservative source gains and final mix limiter.
        let cutoff = if self.kind < CAR_VOICES {
            get(&self.parameters.muffler_step)
        } else if self.kind < CAR_VOICES + IMPACT_VOICES {
            get(&self.parameters.debris_filter)
        } else {
            0.3
        };
        self.output += (raw - self.output) * cutoff;
        (self.output + horn) * self.gain
    }
}
impl Iterator for Synth {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        Some(self.sample())
    }
}
impl Source for Synth {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        1
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Spatial {
    renderer: SpatialRenderer,
    controller: SoundController,
    parameters: Arc<Parameters>,
    id: usize,
    gear: usize,
    shifting: f32,
    presence: f32,
    present: bool,
}
impl Spatial {
    fn new(index: usize) -> Self {
        let parameters = Arc::new(Parameters::default());
        let synth = Synth::new(parameters.clone(), index, 1789 + index as u32 * 103);
        #[cfg(not(target_arch = "wasm32"))]
        let (stream, mut controller) =
            bstream(synth, BstreamConfig::new().with_position([0., 1., 0.]));
        #[cfg(not(target_arch = "wasm32"))]
        let mut renderer = BstreamStereoRenderer::new(stream, StereoConfig::default());
        #[cfg(target_arch = "wasm32")]
        let (mut renderer, mut controller) = super::spatial_web::Renderer::new(synth);
        // Warm the crate's command vector before handing ownership to callback.
        controller.adjust_position([0., 1., 0.]);
        controller.set_velocity([0.; 3]);
        let _ = renderer.next();
        let _ = renderer.next();
        Self {
            renderer,
            controller,
            parameters,
            id: usize::MAX,
            gear: 0,
            shifting: 0.,
            presence: 0.,
            present: false,
        }
    }
    fn position(&mut self, position: Vec3, velocity: Vec3, frame: &Frame) {
        let mut p = listener_vector(position - frame.eye, frame.forward);
        // ambisonic 0.4.1 divides by distance at the origin; never send zero.
        if p.iter().map(|v| v * v).sum::<f32>() < 1. {
            p = [0., 1., 0.];
        }
        self.controller.adjust_position(p);
        self.controller.set_velocity(listener_vector(
            (velocity - frame.velocity).clamp_length_max(120.),
            frame.forward,
        ));
    }
    fn car(&mut self, car: Car, frame: &Frame) {
        if car.id == usize::MAX {
            put(&self.parameters.gain, 0.);
            return;
        }
        if car.id != self.id {
            self.id = car.id;
            self.gear = 0;
            self.shifting = 0.;
            self.parameters.horn_reset.store(true, Ordering::Relaxed);
            self.parameters.engine_reset.store(true, Ordering::Relaxed);
        }
        let speed = car.velocity.truncate().length();
        let sound = car.sound;
        let ratios = sound.gear_ratios;
        let wheel_rpm = speed * 60. / (std::f32::consts::TAU * car.wheel_radius);
        let coupled = wheel_rpm * ratios[self.gear] * sound.final_drive;
        if sound.engine == EngineKind::Electric {
            self.gear = 0;
            self.shifting = 0.;
        } else if self.shifting <= 0. {
            let old = self.gear;
            if coupled > sound.redline_rpm * 0.78 && self.gear < ratios.len() - 1 {
                self.gear += 1;
            } else if coupled < sound.idle_rpm * 1.65 && self.gear > 0 {
                self.gear -= 1;
            }
            if old != self.gear {
                self.shifting = 0.22;
            }
        }
        self.shifting = (self.shifting - BLOCK as f32 / RATE as f32).max(0.);
        let load = car.throttle.abs() * if self.shifting > 0. { 0.12 } else { 1. };
        let (idle, redline, exhaust, body) = (
            sound.idle_rpm,
            sound.redline_rpm,
            sound.exhaust_gain,
            sound.body_gain,
        );
        let rpm =
            (wheel_rpm * ratios[self.gear] * sound.final_drive + load * 380.).clamp(idle, redline);
        let right = Vec3::new(-car.yaw.sin(), car.yaw.cos(), 0.);
        let slip = car.velocity.dot(right).abs();
        let rough = match car.surface {
            SurfaceKind::Grass => 1.3,
            SurfaceKind::Sand => 1.6,
            SurfaceKind::Water => 0.7,
            SurfaceKind::Sidewalk => 1.15,
            _ => 0.8,
        };
        let distance = car.position.distance(frame.eye);
        let boost = if car.player { 4. } else { 1.8 };
        put(&self.parameters.rpm, rpm);
        put(&self.parameters.load, load);
        put(
            &self.parameters.profile,
            if car.engine_on {
                frame.settings.engines
                    * exhaust
                    * sound.engine_gain
                    * if sound.engine == EngineKind::Electric {
                        (speed / 3.).min(1.)
                    } else {
                        1.
                    }
            } else {
                0.
            },
        );
        put(&self.parameters.engine_body, body);
        self.parameters.engine_kind.store(
            match sound.engine {
                EngineKind::Petrol => 0,
                EngineKind::Diesel => 1,
                EngineKind::Electric => 2,
            },
            Ordering::Relaxed,
        );
        put(&self.parameters.firing_order, sound.firing_order);
        put(&self.parameters.roughness, sound.roughness);
        put(&self.parameters.machinery_gain, sound.machinery_gain);
        put(
            &self.parameters.muffler_step,
            1. - (-std::f32::consts::TAU * sound.muffler_hz * (1. + load * 0.3) / RATE as f32)
                .exp(),
        );
        put(
            &self.parameters.horn_frequency_a,
            sound.horn.frequencies_hz[0],
        );
        put(
            &self.parameters.horn_frequency_b,
            sound.horn.frequencies_hz[1],
        );
        put(
            &self.parameters.horn_gain,
            if car.horn {
                frame.settings.horns * sound.horn.gain
            } else {
                0.
            },
        );
        self.parameters
            .air_horn
            .store(sound.horn.kind == HornKind::Air, Ordering::Relaxed);
        put(
            &self.parameters.rolling,
            if car.grounded {
                (speed / 32.).min(1.4) * rough * frame.settings.tires * 0.3
            } else {
                0.
            },
        );
        put(
            &self.parameters.scrub,
            if car.grounded {
                ((slip / 10.).min(1.)
                    + if car.handbrake || car.brake {
                        (speed / 35.).min(0.7)
                    } else {
                        0.
                    })
                    * frame.settings.tires
                    * 0.28
            } else {
                0.
            },
        );
        put(
            &self.parameters.gain,
            boost * frame.settings.master * ((140. - distance) / 35.).clamp(0., 1.),
        );
        self.position(car.position, car.velocity, frame);
    }
}

pub(super) struct Mix {
    bridge: Arc<Bridge>,
    voices: [Spatial; VOICES],
    frame: Frame,
    block_left: usize,
    right: Option<f32>,
    stale_blocks: usize,
    last_impacts: [u64; IMPACT_VOICES],
    impact_position: [Vec3; IMPACT_VOICES],
    impact_life: [f32; IMPACT_VOICES],
    started: bool,
    revision: u64,
    impact_strength: [f32; IMPACT_VOICES],
    #[cfg(test)]
    control_updates: usize,
}
impl Mix {
    fn clear_transients(&mut self) {
        for voice in &self.voices[..CAR_VOICES] {
            put(&voice.parameters.horn_gain, 0.);
            voice.parameters.horn_reset.store(true, Ordering::Relaxed);
        }
        for i in 0..IMPACT_VOICES {
            self.impact_life[i] = 0.;
            put(&self.voices[CAR_VOICES + i].parameters.gain, 0.);
        }
    }
    pub(super) fn new(bridge: Arc<Bridge>) -> Self {
        let mut voices = std::array::from_fn(Spatial::new);
        for (i, voice) in voices
            .iter_mut()
            .enumerate()
            .skip(CAR_VOICES + IMPACT_VOICES)
        {
            voice
                .controller
                .adjust_position([if i % 2 == 0 { -1. } else { 1. }, 1., 0.]);
            let _ = voice.renderer.next();
            let _ = voice.renderer.next();
        }
        Self {
            bridge,
            voices,
            frame: Frame::default(),
            block_left: 0,
            right: None,
            stale_blocks: 0,
            last_impacts: [0; IMPACT_VOICES],
            impact_position: [Vec3::ZERO; IMPACT_VOICES],
            impact_life: [0.; IMPACT_VOICES],
            started: false,
            revision: 0,
            impact_strength: [0.; IMPACT_VOICES],
            #[cfg(test)]
            control_updates: 0,
        }
    }
    fn control(&mut self) -> bool {
        let revision = self.bridge.revision.load(Ordering::Acquire);
        if revision == self.revision {
            self.stale_blocks += 1;
        } else {
            self.stale_blocks = 0;
            self.revision = revision;
        }
        // Stop before sending any controller commands. Stalled main threads
        // cannot accumulate ambisonic's internal command vectors.
        if self.stale_blocks > RATE as usize / BLOCK / 2 {
            if self.started {
                self.clear_transients();
            }
            self.started = false;
            return false;
        }
        #[cfg(test)]
        {
            self.control_updates += 1;
        }
        // try_lock never blocks output; one fixed-size snapshot overwrites old work.
        if let Ok(frame) = self.bridge.latest.try_lock() {
            self.frame = *frame;
        }
        let frame = self.frame;
        for voice in &mut self.voices[..CAR_VOICES] {
            voice.present = frame
                .cars
                .iter()
                .any(|car| car.id != usize::MAX && car.id == voice.id);
            // Preserve the old profile and spatial state until it is silent.
            // Reassigning immediately changes timbre/pitch at audible gain.
            if !voice.present && voice.presence == 0. {
                voice.id = usize::MAX;
                put(&voice.parameters.gain, 0.);
            }
        }
        // Source identity survives nearest-distance rank swaps and overtaking.
        for car in frame
            .cars
            .iter()
            .copied()
            .filter(|car| car.id != usize::MAX)
        {
            let slot = self.voices[..CAR_VOICES]
                .iter()
                .position(|voice| voice.id == car.id)
                .or_else(|| {
                    self.voices[..CAR_VOICES]
                        .iter()
                        .position(|voice| voice.id == usize::MAX)
                });
            if let Some(slot) = slot {
                self.voices[slot].car(car, &frame);
                self.voices[slot].present = true;
            }
        }
        for i in 0..IMPACT_VOICES {
            let hit = frame.impacts[i];
            if hit.serial != 0 && hit.serial != self.last_impacts[i] {
                self.last_impacts[i] = hit.serial;
                if self.started && hit.age < 0.15 && hit.position.distance(frame.eye) < 100. {
                    let slot = self
                        .impact_life
                        .iter()
                        .enumerate()
                        .min_by(|a, b| a.1.total_cmp(b.1))
                        .unwrap()
                        .0;
                    self.impact_position[slot] = hit.position;
                    self.impact_life[slot] = 1.4;
                    self.impact_strength[slot] = (hit.speed / 14.).clamp(0.05, 1.) * 3.;
                    let p = &self.voices[CAR_VOICES + slot].parameters;
                    impact_parameters(p, hit.material, hit.broken);
                    p.impact.store(hit.serial as u32, Ordering::Relaxed);
                }
            }
        }
        for i in 0..IMPACT_VOICES {
            self.impact_life[i] = (self.impact_life[i] - BLOCK as f32 / RATE as f32).max(0.);
            self.voices[CAR_VOICES + i].position(self.impact_position[i], Vec3::ZERO, &frame);
            put(
                &self.voices[CAR_VOICES + i].parameters.gain,
                frame.settings.master * frame.settings.impacts * self.impact_strength[i],
            );
            if self.impact_life[i] == 0. {
                put(&self.voices[CAR_VOICES + i].parameters.gain, 0.);
            }
        }
        for i in CAR_VOICES + IMPACT_VOICES..VOICES {
            let p = &self.voices[i].parameters;
            put(
                &p.gain,
                frame.settings.master * frame.settings.ambience * 0.2,
            );
            put(&p.wind, (frame.wind_speed / 35.).min(1.5));
            put(&p.rolling, frame.foliage);
            // Two broad independent noise fields, no voices or bird calls.
        }
        if !self.started {
            // Bstream retains two interpolation samples while we are silent.
            // Flush those after resetting controls so an old horn cannot click
            // at resume. Four stereo frames cover the bounded Doppler rates.
            for voice in &mut self.voices {
                for _ in 0..8 {
                    let _ = voice.renderer.next();
                }
            }
        }
        self.started = true;
        true
    }
}
impl Iterator for Mix {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.bridge.reset.swap(false, Ordering::AcqRel) {
            self.clear_transients();
            self.right = None;
            self.block_left = 0;
            self.started = false;
        }
        if !self.bridge.active.load(Ordering::Acquire) {
            self.right = None;
            self.block_left = 0;
            if self.started {
                self.clear_transients();
            }
            self.started = false;
            return Some(0.);
        }
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.block_left == 0 {
            self.control();
            self.block_left = BLOCK;
        }
        self.block_left -= 1;
        // A blocked/minimized main thread cannot leave the previous scene playing.
        if self.stale_blocks > RATE as usize / BLOCK / 2 {
            self.right = Some(0.);
            return Some(0.);
        }
        let mut l = 0.;
        let mut r = 0.;
        for (index, voice) in self.voices.iter_mut().enumerate() {
            let envelope = if index < CAR_VOICES {
                let target = if voice.present { 1. } else { 0. };
                let step = 1. / (RATE as f32 * 0.02);
                voice.presence += (target - voice.presence).clamp(-step, step);
                voice.presence
            } else {
                1.
            };
            l += voice.renderer.next().unwrap_or(0.) * envelope;
            r += voice.renderer.next().unwrap_or(0.) * envelope;
        }
        // Smooth bounded saturation, unity at low level, no hard clipping.
        let l = l * OUTPUT_GAIN;
        let r = r * OUTPUT_GAIN;
        self.right = Some(r / (1. + r.abs()));
        Some(l / (1. + l.abs()))
    }
}
impl Source for Mix {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
