//! A complete scene buffer runs on the browser audio clock without refill callbacks.
use super::{scene::RATE, VoiceClip};
use web_sys::{AudioBuffer, AudioBufferSourceNode, AudioContext, AudioScheduledSourceNode};

pub(super) struct BrowserCutscene {
    buffer: AudioBuffer,
    source: Option<AudioBufferSourceNode>,
    offset: f64,
    started: f64,
}
impl BrowserCutscene {
    pub fn new(
        context: &AudioContext,
        clip: Option<&VoiceClip>,
        start: f32,
        duration: f32,
        gain: f32,
    ) -> Result<Self, wasm_bindgen::JsValue> {
        let length = (duration * RATE as f32).ceil() as usize;
        let buffer = context.create_buffer(2, length as u32, RATE as f32)?;

        for channel in 0..2 {
            let samples = VoiceClip::scene_channel(clip, channel, start, duration, gain);
            buffer.copy_to_channel(&samples, channel as i32)?;
        }
        Ok(Self {
            buffer,
            source: None,
            offset: 0.,
            started: 0.,
        })
    }
    pub fn time(&self, context: &AudioContext) -> f64 {
        (self.offset
            + if self.source.is_some() {
                context.current_time() - self.started
            } else {
                0.
            })
        .min(self.buffer.duration())
    }
    pub fn set_playing(
        &mut self,
        context: &AudioContext,
        active: bool,
    ) -> Result<(), wasm_bindgen::JsValue> {
        if active && self.source.is_none() && self.offset < self.buffer.duration() {
            let source = context.create_buffer_source()?;
            source.set_buffer(Some(&self.buffer));
            source.connect_with_audio_node(&context.destination())?;
            source.start_with_when_and_grain_offset(0., self.offset)?;
            self.started = context.current_time();
            self.source = Some(source);
        } else if !active {
            self.offset = self.time(context);
            self.stop();
        }
        Ok(())
    }
    fn stop(&mut self) {
        if let Some(source) = self.source.take() {
            let _ = AsRef::<AudioScheduledSourceNode>::as_ref(&source).stop();
            let _ = source.disconnect();
        }
    }
}
impl Drop for BrowserCutscene {
    fn drop(&mut self) {
        self.stop();
    }
}
