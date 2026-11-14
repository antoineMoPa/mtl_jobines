//! The same CPAL output path on native and wasm; no browser-only synthesizer.
use super::{
    scene::{Bridge, Mix},
    Frame,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::source::UniformSourceIterator;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub(super) struct Backend {
    bridge: Arc<Bridge>,
    stream: cpal::Stream,
    playing: Rc<Cell<bool>>,
    #[cfg(target_arch = "wasm32")]
    browser_start: BrowserStart,
    #[cfg(target_arch = "wasm32")]
    _visibility: VisibilityGuard,
}
impl Backend {
    pub fn open() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("No audio output device")?;
        let supported = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        #[cfg(target_arch = "wasm32")]
        let config = cpal::StreamConfig {
            channels: 2,
            // CPAL's web backend refills two alternating buffers from main-thread
            // onended callbacks. 512 frames leave only ~11 ms at 48 kHz, less
            // than one city render frame. Keep ~85 ms ahead for render jitter.
            buffer_size: cpal::BufferSize::Fixed(4096),
            ..config
        };
        if config.channels == 0 || config.sample_rate.0 == 0 {
            return Err("Invalid audio device format".into());
        }
        let bridge = Arc::new(Bridge {
            latest: Mutex::new(Frame::default()),
            active: AtomicBool::new(false),
            revision: AtomicU64::new(0),
            reset: AtomicBool::new(false),
        });
        let mixer = Mix::new(bridge.clone());
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, mixer, bridge.clone()),
            cpal::SampleFormat::F64 => build::<f64>(&device, &config, mixer, bridge.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, mixer, bridge.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, mixer, bridge.clone()),
            cpal::SampleFormat::I32 => build::<i32>(&device, &config, mixer, bridge.clone()),
            _ => return Err(format!("Unsupported audio format: {format:?}")),
        }?;
        let playing = Rc::new(Cell::new(false));
        #[cfg(target_arch = "wasm32")]
        let visibility = VisibilityGuard::new(&stream, bridge.clone(), playing.clone())?;
        Ok(Self {
            bridge,
            stream,
            playing,
            #[cfg(target_arch = "wasm32")]
            browser_start: BrowserStart::default(),
            #[cfg(target_arch = "wasm32")]
            _visibility: visibility,
        })
    }
    pub fn update(&self, frame: Frame, active: bool) {
        #[cfg(target_arch = "wasm32")]
        let active = active && !self._visibility.document.hidden();
        if let Ok(mut latest) = self.bridge.latest.lock() {
            *latest = frame;
        }
        self.bridge.revision.fetch_add(1, Ordering::Release);
        self.bridge.active.store(active, Ordering::Release);
        if active && !self.playing.get() {
            self.resume();
        } else if !active {
            self.silence();
        }
    }
    pub fn resume(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        let result = self.stream.play().map_err(|error| error.to_string());
        #[cfg(target_arch = "wasm32")]
        let result = self.browser_start.resume(
            || self.stream.play().map_err(|error| error.to_string()),
            || settle_web_change(web_context(&self.stream).resume()),
        );
        match result {
            Ok(()) => self.playing.set(true),
            Err(error) => eprintln!("Sound unavailable: {error}"),
        }
    }
    pub fn silence(&self) {
        self.bridge.active.store(false, Ordering::Release);
        if self.playing.get() {
            // Pause can stop callbacks before Mix sees active=false. Keep a
            // reset pending until its next sample, even if gameplay resumes first.
            self.bridge.reset.store(true, Ordering::Release);
            if self.stream.pause().is_ok() {
                self.playing.set(false);
            }
        }
    }
}

fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mixer: Mix,
    bridge: Arc<Bridge>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let mut samples = OutputSamples {
        source: UniformSourceIterator::new(mixer, 2, config.sample_rate.0),
    };
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                if bridge.active.load(Ordering::Acquire) {
                    samples.write(data, channels);
                } else {
                    data.fill(T::EQUILIBRIUM);
                }
            },
            |error| eprintln!("Audio output error: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}

/// Explicit stereo -> device-channel mapping, preserving stereo resampler phase.
struct OutputSamples<S> {
    source: S,
}
impl<S: Iterator<Item = f32>> OutputSamples<S> {
    fn write<T: cpal::Sample + cpal::FromSample<f32>>(&mut self, data: &mut [T], channels: usize) {
        if channels == 0 {
            data.fill(T::EQUILIBRIUM);
            return;
        }
        for frame in data.chunks_mut(channels) {
            if frame.len() != channels {
                frame.fill(T::EQUILIBRIUM);
                continue;
            }
            let left = self.source.next().unwrap_or(0.);
            let right = self.source.next().unwrap_or(0.);
            if channels == 1 {
                frame[0] = T::from_sample((left + right) * 0.5);
            } else {
                frame[0] = T::from_sample(left);
                frame[1] = T::from_sample(right);
                frame[2..].fill(T::EQUILIBRIUM);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_resume_installs_one_scheduler_and_retries_initial_failure() {
        let transport = BrowserStart::default();
        let starts = Cell::new(0);
        let resumes = Cell::new(0);
        assert!(transport
            .resume(|| Err("initial failure".into()), || panic!("not started"))
            .is_err());
        transport
            .resume(
                || {
                    starts.set(starts.get() + 1);
                    Ok(())
                },
                || panic!("not started"),
            )
            .unwrap();
        for _ in 0..1000 {
            transport
                .resume(
                    || panic!("must not install more workers"),
                    || {
                        resumes.set(resumes.get() + 1);
                        Ok(())
                    },
                )
                .unwrap();
        }
        assert_eq!(starts.get(), 1);
        assert_eq!(resumes.get(), 1000);
        assert!(transport
            .resume(
                || panic!("already started"),
                || Err("blocked resume".into())
            )
            .is_err());
        transport
            .resume(|| panic!("resume must reuse existing context"), || Ok(()))
            .unwrap();
    }
    #[test]
    fn shared_resampler_keeps_tone_pitch_at_device_sample_rate() {
        let source = rodio::source::SineWave::new(440);
        let mut output = OutputSamples {
            source: UniformSourceIterator::new(source, 2, 44100),
        };
        let mut pcm = vec![0f32; 44100 * 2];
        output.write(&mut pcm, 2);
        let left: Vec<_> = pcm.iter().step_by(2).copied().collect();
        let count = left.windows(2).filter(|p| p[0] <= 0. && p[1] > 0.).count();
        assert!((439..=441).contains(&count));
        assert!(pcm
            .chunks_exact(2)
            .all(|p| p[0] == p[1] && p[0].is_finite()));
    }
    #[test]
    fn device_channels_and_formats_preserve_stereo_pairs() {
        let mut output = OutputSamples {
            source: [0.25, -0.5, 0.5, 0.25].into_iter(),
        };
        let mut surround = [9.; 6];
        output.write(&mut surround, 6);
        assert_eq!(surround, [0.25, -0.5, 0., 0., 0., 0.]);
        let mut mono = [0.];
        output.write(&mut mono, 1);
        assert_eq!(mono, [0.375]);
        let mut output = OutputSamples {
            source: [0.5, -0.5].into_iter(),
        };
        let mut pcm = [0i16; 2];
        output.write(&mut pcm, 2);
        assert_eq!(pcm, [16384, -16384]);
        let mut output = OutputSamples {
            source: [0.25, -0.25].into_iter(),
        };
        let mut malformed = [9.; 3];
        output.write(&mut malformed, 2);
        assert_eq!(malformed, [0.25, -0.25, 0.]);
    }
}

/// CPAL's initial scheduler is installed once. Later gestures resume the same
/// context, including after pause or an autoplay block; they never add workers.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
struct BrowserStart {
    started: Cell<bool>,
}
#[cfg(any(target_arch = "wasm32", test))]
impl BrowserStart {
    fn resume(
        &self,
        start: impl FnOnce() -> Result<(), String>,
        resume_context: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        if self.started.get() {
            resume_context()
        } else {
            start()?;
            self.started.set(true);
            Ok(())
        }
    }
}
#[cfg(target_arch = "wasm32")]
fn web_context(stream: &cpal::Stream) -> web_sys::AudioContext {
    let cpal::platform::StreamInner::WebAudio(stream) = stream.as_inner();
    stream.audio_context().clone()
}
#[cfg(target_arch = "wasm32")]
fn settle_web_change(result: Result<js_sys::Promise, wasm_bindgen::JsValue>) -> Result<(), String> {
    result
        .map(|promise| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(error) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    web_sys::console::warn_1(&error);
                }
            })
        })
        .map_err(|error| format!("Could not change browser audio state: {error:?}"))
}
#[cfg(target_arch = "wasm32")]
struct VisibilityGuard {
    document: web_sys::Document,
    callback: wasm_bindgen::closure::Closure<dyn FnMut()>,
}
#[cfg(target_arch = "wasm32")]
impl VisibilityGuard {
    fn new(
        stream: &cpal::Stream,
        bridge: Arc<Bridge>,
        playing: Rc<Cell<bool>>,
    ) -> Result<Self, String> {
        use wasm_bindgen::JsCast;
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or("No browser document for audio")?;
        let observed = document.clone();
        let context = web_context(stream);
        let callback = wasm_bindgen::closure::Closure::new(move || {
            if observed.hidden() {
                bridge.active.store(false, Ordering::Release);
                bridge.reset.store(true, Ordering::Release);
                playing.set(false);
                let _ = settle_web_change(context.suspend());
            }
        });
        document
            .add_event_listener_with_callback("visibilitychange", callback.as_ref().unchecked_ref())
            .map_err(|error| format!("Could not register audio visibility handling: {error:?}"))?;
        Ok(Self { document, callback })
    }
}
#[cfg(target_arch = "wasm32")]
impl Drop for VisibilityGuard {
    fn drop(&mut self) {
        use wasm_bindgen::JsCast;
        let _ = self.document.remove_event_listener_with_callback(
            "visibilitychange",
            self.callback.as_ref().unchecked_ref(),
        );
    }
}
