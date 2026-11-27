//! A non-spatial voice and its scene clock, carried by the existing CPAL stream.
use super::scene::RATE;
use rodio::Source;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct VoiceClip {
    frames: Vec<[f32; 2]>,
}
impl VoiceClip {
    /// PCM16 mono/stereo WAV. Decode and resample once, outside the audio callback.
    pub fn from_wav(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("Voice asset is not a WAV file".into());
        }
        let mut format = None;
        let mut data = None;
        let mut at = 12usize;
        while at + 8 <= bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let start = at + 8;
            // Streaming encoders can leave a near-2 GiB placeholder in the
            // final data chunk. Its actual length is the remaining complete PCM
            // frames; ordinary declared chunk lengths must still fit the file.
            let streaming_data = &bytes[at..at + 4] == b"data" && size >= 0x7fff_0000;
            let end = if streaming_data {
                bytes.len()
            } else {
                start
                    .checked_add(size)
                    .filter(|end| *end <= bytes.len())
                    .ok_or("Truncated WAV chunk")?
            };
            match &bytes[at..at + 4] {
                b"fmt " if size >= 16 => {
                    let u16_at =
                        |n| u16::from_le_bytes(bytes[start + n..start + n + 2].try_into().unwrap());
                    format = Some((
                        u16_at(0),
                        u16_at(2),
                        u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap()),
                        u16_at(12),
                        u16_at(14),
                    ));
                }
                b"data" => data = Some(&bytes[start..end]),
                _ => {}
            }
            at = end + (size & 1);
        }
        let (encoding, channels, rate, align, bits) = format.ok_or("Missing WAV format")?;
        if encoding != 1
            || bits != 16
            || !(1..=2).contains(&channels)
            || rate == 0
            || align != channels * 2
        {
            return Err("Voice WAV must be PCM16 mono or stereo".into());
        }
        let data = data.ok_or("Missing WAV samples")?;
        if data.is_empty() || data.len() % align as usize != 0 {
            return Err("Invalid WAV sample count".into());
        }
        let input: Vec<[f32; 2]> = data
            .chunks_exact(align as usize)
            .map(|frame| {
                let left = i16::from_le_bytes([frame[0], frame[1]]) as f32 / 32768.;
                let right = if channels == 2 {
                    i16::from_le_bytes([frame[2], frame[3]]) as f32 / 32768.
                } else {
                    left
                };
                [left, right]
            })
            .collect();
        let count = (input.len() as u64 * RATE as u64 / rate as u64) as usize;
        if count == 0 {
            return Err("Empty voice clip".into());
        }
        let frames = (0..count)
            .map(|i| {
                let position = i as f64 * rate as f64 / RATE as f64;
                let a = position as usize;
                let b = (a + 1).min(input.len() - 1);
                let t = (position - a as f64) as f32;
                std::array::from_fn(|ch| input[a][ch] + (input[b][ch] - input[a][ch]) * t)
            })
            .collect();
        Ok(Self { frames })
    }
    /// Full scene timeline, including voice delay and the silent tail.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(super) fn scene_channel(
        clip: Option<&Self>,
        channel: usize,
        start: f32,
        duration: f32,
        gain: f32,
    ) -> Vec<f32> {
        let mut samples = vec![0.; (duration * RATE as f32).ceil() as usize];
        let first = (start * RATE as f32).round() as usize;
        if let Some(clip) = clip {
            for (sample, frame) in samples.iter_mut().skip(first).zip(&clip.frames) {
                *sample = super::protect_peaks(frame.map(|s| s * gain.clamp(0., 1.)))[channel];
            }
        }
        samples
    }
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / RATE as f32
    }
}
struct Track {
    clip: Option<Arc<VoiceClip>>,
    cursor: usize,
    start: usize,
    end: usize,
    gain: f32,
    playing: bool,
}
#[derive(Default)]
pub(super) struct VoiceBus(Mutex<Option<Track>>);
impl VoiceBus {
    pub fn load(&self, clip: Option<Arc<VoiceClip>>, start: f32, duration: f32, gain: f32) {
        *self.0.lock().unwrap() = Some(Track {
            clip,
            cursor: 0,
            start: (start * RATE as f32).round() as usize,
            end: (duration * RATE as f32).ceil() as usize,
            gain: gain.clamp(0., 1.),
            playing: false,
        });
    }
    pub fn set_playing(&self, playing: bool) {
        if let Some(track) = self.0.lock().unwrap().as_mut() {
            track.playing = playing;
        }
    }
    pub fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
    pub fn time(&self) -> Option<f32> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .map(|t| t.cursor as f32 / RATE as f32)
    }
    fn sample(&self) -> Option<[f32; 2]> {
        // Control operations can own the track briefly. Keep the callback and
        // scene clock together by emitting one silent frame during contention.
        let Ok(mut state) = self.0.try_lock() else {
            return Some([0.; 2]);
        };
        let t = state.as_mut()?;
        if !t.playing || t.cursor >= t.end {
            return Some([0.; 2]);
        }
        let sample = t
            .cursor
            .checked_sub(t.start)
            .and_then(|i| t.clip.as_ref()?.frames.get(i))
            .copied()
            .unwrap_or([0.; 2]);
        t.cursor += 1;
        Some(super::protect_peaks(sample.map(|s| s * t.gain)))
    }
}
/// Cutscene output replaces world sounds while the world is paused.
pub(super) struct Soundtrack<S> {
    pub world: S,
    pub voice: Arc<VoiceBus>,
    pub right: Option<f32>,
}
impl<S: Iterator<Item = f32>> Iterator for Soundtrack<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if let Some([left, right]) = self.voice.sample() {
            self.right = Some(right);
            Some(left)
        } else {
            let left = self.world.next();
            self.right = self.world.next();
            left
        }
    }
}
impl<S: Source<Item = f32>> Source for Soundtrack<S> {
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
mod tests {
    use super::*;
    #[test]
    fn complete_scene_buffer_preserves_delay_stereo_gain_and_silent_tail() {
        let clip = VoiceClip {
            frames: vec![[0.8, -0.4]; 2],
        };
        for (channel, value) in [(0, 0.4), (1, -0.2)] {
            assert_eq!(
                VoiceClip::scene_channel(
                    Some(&clip),
                    channel,
                    1. / RATE as f32,
                    4. / RATE as f32,
                    0.5
                ),
                vec![0., value, value, 0.]
            );
        }
        assert_eq!(
            VoiceClip::scene_channel(None, 0, 0., 4. / RATE as f32, 1.),
            vec![0.; 4]
        );
    }
    #[test]

    fn voice_clock_pause_skip_and_delay() {
        let bus = VoiceBus::default();
        let clip = Arc::new(VoiceClip {
            frames: vec![[0.8, -0.4]; 2],
        });
        bus.load(Some(clip), 1. / RATE as f32, 4. / RATE as f32, 0.5);
        assert_eq!(bus.sample(), Some([0., 0.]));
        assert_eq!(bus.time(), Some(0.));
        bus.set_playing(true);
        {
            let _guard = bus.0.lock().unwrap();
            assert_eq!(bus.sample(), Some([0.; 2]));
        }
        assert_eq!(bus.time(), Some(0.));
        assert_eq!(bus.sample(), Some([0., 0.]));
        assert_eq!(bus.sample(), Some([0.4, -0.2]));
        bus.set_playing(false);
        let paused = bus.time();
        assert_eq!(bus.sample(), Some([0., 0.]));
        assert_eq!(bus.time(), paused);
        bus.set_playing(true);
        assert_eq!(bus.sample(), Some([0.4, -0.2]));
        bus.clear();
        assert_eq!(bus.sample(), None);
        assert_eq!(bus.time(), None);
    }
    #[test]
    fn bundled_pcm_decodes_and_rejects_truncation() {
        let bytes = include_bytes!("../../assets/cutscenes/opening/voice.wav");
        let clip = VoiceClip::from_wav(bytes).unwrap();
        assert!((clip.duration() - 4.4).abs() < 0.001);
        assert!(clip.frames.iter().all(|f| f[0] == f[1] && f[0].is_finite()));
        assert!(VoiceClip::from_wav(&bytes[..bytes.len() - 1]).is_err());
        let mut declared = bytes.to_vec();
        declared[40..44].copy_from_slice(&((bytes.len() - 44) as u32).to_le_bytes());
        assert_eq!(VoiceClip::from_wav(&declared).unwrap().frames, clip.frames);
        assert!(VoiceClip::from_wav(&declared[..declared.len() - 2]).is_err());
    }
}
