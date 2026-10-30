//! Codec-free first-order stereo adapter for wasm. Native keeps the ambisonic
//! crate; its mandatory MP3 dependency cannot compile on wasm. Both adapters use
//! X right/Y forward/Z up, +/-45 degree cardioid decoding, inverse distance and
//! a single relative-velocity Doppler resampling stage. No HRTF is implied.
use rodio::Source;
use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
struct Control {
    position: [AtomicU32; 3],
    velocity: [AtomicU32; 3],
    revision: AtomicU32,
}
pub(super) struct Controller {
    control: Arc<Control>,
}
impl Controller {
    pub fn adjust_position(&mut self, position: [f32; 3]) {
        for (slot, value) in self.control.position.iter().zip(position) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        self.control.revision.fetch_add(1, Ordering::Release);
    }
    pub fn set_velocity(&mut self, velocity: [f32; 3]) {
        for (slot, value) in self.control.velocity.iter().zip(velocity) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        self.control.revision.fetch_add(1, Ordering::Release);
    }
}
pub(super) struct Renderer<S> {
    source: S,
    control: Arc<Control>,
    revision: u32,
    gains: [f32; 2],
    targets: [f32; 2],
    speed: f32,
    offset: f32,
    previous: f32,
    next: f32,
    right: Option<f32>,
}
impl<S: Source<Item = f32>> Renderer<S> {
    pub fn new(mut source: S) -> (Self, Controller) {
        let control = Arc::new(Control {
            position: [0., 1., 0.].map(|v: f32| AtomicU32::new(v.to_bits())),
            velocity: std::array::from_fn(|_| AtomicU32::new(0)),
            revision: AtomicU32::new(1),
        });
        let previous = source.next().unwrap_or(0.);
        let next = source.next().unwrap_or(0.);
        let initial = 0.5 + 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        (
            Self {
                source,
                control: control.clone(),
                revision: 0,
                gains: [initial; 2],
                targets: [initial; 2],
                speed: 1.,
                offset: 0.,
                previous,
                next,
                right: None,
            },
            Controller { control },
        )
    }
    fn read_controls(&mut self) {
        let revision = self.control.revision.load(Ordering::Acquire);
        if revision == self.revision {
            return;
        }
        self.revision = revision;
        let mut p = self
            .control
            .position
            .each_ref()
            .map(|v| f32::from_bits(v.load(Ordering::Relaxed)));
        let mut distance = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if distance < 0.0001 {
            p = [0., 1., 0.];
            distance = 1.;
        }
        let falloff = 1. / distance.max(1.);
        let side = std::f32::consts::FRAC_1_SQRT_2 * 0.5 / distance;
        self.targets = [
            falloff * (0.5 + side * (-p[0] + p[1])),
            falloff * (0.5 + side * (p[0] + p[1])),
        ];
        let velocity = self
            .control
            .velocity
            .each_ref()
            .map(|v| f32::from_bits(v.load(Ordering::Relaxed)));
        let radial = (p[0] * velocity[0] + p[1] * velocity[1] + p[2] * velocity[2]) / distance;
        self.speed = (343. / (343. + radial)).clamp(0.5, 2.);
    }
}
impl<S: Source<Item = f32>> Iterator for Renderer<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        self.read_controls();
        for (gain, target) in self.gains.iter_mut().zip(self.targets) {
            *gain += (target - *gain).clamp(-0.001, 0.001);
        }
        while self.offset >= 1. {
            self.previous = self.next;
            self.next = self.source.next()?;
            self.offset -= 1.;
        }
        let sample = self.previous * (1. - self.offset) + self.next * self.offset;
        self.offset += self.speed;
        self.right = Some(sample * self.gains[1]);
        Some(sample * self.gains[0])
    }
}
impl<S: Source<Item = f32>> Source for Renderer<S> {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    fn render(position: [f32; 3], velocity: [f32; 3]) -> Vec<f32> {
        let (mut source, mut control) = Renderer::new(rodio::source::SineWave::new(440));
        control.adjust_position(position);
        control.set_velocity(velocity);
        source.by_ref().skip(24000).take(48000).collect()
    }
    fn energy(samples: &[f32], channel: usize) -> f32 {
        samples.iter().skip(channel).step_by(2).map(|v| v * v).sum()
    }
    fn crossings(samples: &[f32]) -> usize {
        samples
            .iter()
            .step_by(2)
            .copied()
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|p| p[0] <= 0. && p[1] > 0.)
            .count()
    }
    #[test]
    fn wasm_adapter_matches_native_static_decoder_and_doppler_direction() {
        for position in [
            [1., 0., 0.],
            [-1., 0., 0.],
            [0., 1., 0.],
            [0., -1., 0.],
            [0., 10., 0.],
            [0., 0., 0.],
        ] {
            let samples = render(position, [0.; 3]);
            assert!(samples.iter().all(|v| v.is_finite()));
            if position == [0.; 3] {
                continue;
            }
            let (stream, _) = ambisonic::bstream(
                rodio::source::SineWave::new(440),
                ambisonic::BstreamConfig::new().with_position(position),
            );
            let native: Vec<_> =
                ambisonic::BstreamStereoRenderer::new(stream, ambisonic::StereoConfig::default())
                    .skip(24000)
                    .take(48000)
                    .collect();
            for channel in 0..2 {
                assert!((energy(&samples, channel) - energy(&native, channel)).abs() < 0.05);
            }
        }
        let stationary = crossings(&render([0., 10., 0.], [0.; 3]));
        assert!(crossings(&render([0., 10., 0.], [0., -40., 0.])) > stationary);
        assert!(crossings(&render([0., 10., 0.], [0., 40., 0.])) < stationary);
    }
}
