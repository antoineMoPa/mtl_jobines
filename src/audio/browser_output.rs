//! Queue short PCM buffers against the browser's audio clock.
use super::{
    output::{buffer_start, WorldSoundtrack},
    scene::Bridge,
};
use rodio::source::UniformSourceIterator;
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    sync::{atomic::Ordering, Arc},
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    AudioBufferSourceNode, AudioContext, AudioContextState, AudioScheduledSourceNode, Window,
};

const QUEUE_SECONDS: f64 = 0.080;

pub(super) struct BrowserOutput {
    pub context: AudioContext,
    window: Window,
    queue: Rc<RefCell<Queue>>,
    timer: i32,
    _refill: Closure<dyn FnMut()>,
}

impl BrowserOutput {
    pub fn new(source: WorldSoundtrack, bridge: Arc<Bridge>) -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("Missing window"))?;
        let context = AudioContext::new()?;
        let queue = Rc::new(RefCell::new(Queue::new(source, context.sample_rate())));
        let audio = context.clone();
        let refill_queue = queue.clone();
        let refill = Closure::new(move || {
            if audio.state() == AudioContextState::Running && bridge.active.load(Ordering::Acquire)
            {
                if let Err(error) = refill_queue.borrow_mut().refill(&audio) {
                    web_sys::console::warn_1(&error);
                }
            }
        });
        let timer = window.set_interval_with_callback_and_timeout_and_arguments_0(
            refill.as_ref().unchecked_ref(),
            10,
        )?;
        Ok(Self {
            context,
            window,
            queue,
            timer,
            _refill: refill,
        })
    }

    pub fn clear(&self) {
        self.queue.borrow_mut().clear();
    }
}

impl Drop for BrowserOutput {
    fn drop(&mut self) {
        self.window.clear_interval_with_handle(self.timer);
        self.clear();
        let _ = self.context.close();
    }
}

struct Queue {
    source: UniformSourceIterator<WorldSoundtrack, f32>,
    pending: VecDeque<(f64, AudioBufferSourceNode)>,
    rate: f32,
    samples: [Vec<f32>; 2],
}

impl Queue {
    fn new(source: WorldSoundtrack, rate: f32) -> Self {
        // Keep resampling continuous across chunk boundaries.
        Self {
            source: UniformSourceIterator::new(source, 2, rate as u32),
            pending: VecDeque::new(),
            rate,
            samples: std::array::from_fn(|_| vec![0.; rate as usize / 50]),
        }
    }

    fn refill(&mut self, context: &AudioContext) -> Result<(), JsValue> {
        let now = context.current_time();
        while self.pending.front().is_some_and(|(end, _)| *end <= now) {
            self.pending.pop_front();
        }
        let horizon = now + QUEUE_SECONDS;
        let frames = self.samples[0].len();
        let fade_frames = frames / 4;
        let duration = frames as f64 / f64::from(self.rate);
        // Bound work per timer tick. A stalled page resumes from the current
        // clock and latest world snapshot, with no obsolete buffers to catch up.
        for _ in 0..5 {
            let previous_end = self.pending.back().map(|(end, _)| *end);
            if previous_end.is_some_and(|end| end >= horizon) {
                break;
            }
            let buffer = context.create_buffer(2, frames as u32, self.rate)?;
            let source = context.create_buffer_source()?;
            source.set_buffer(Some(&buffer));
            source.connect_with_audio_node(&context.destination())?;
            for i in 0..frames {
                for channel in &mut self.samples {
                    channel[i] = self.source.next().unwrap_or(0.);
                }
            }
            let (start, restart) = buffer_start(previous_end, context.current_time());
            for (channel, samples) in self.samples.iter_mut().enumerate() {
                if restart {
                    for (i, sample) in samples[..fade_frames].iter_mut().enumerate() {
                        *sample *= i as f32 / fade_frames as f32;
                    }
                }
                buffer.copy_to_channel(samples, channel as i32)?;
            }
            source.start_with_when(start)?;
            self.pending.push_back((start + duration, source));
        }
        Ok(())
    }

    fn clear(&mut self) {
        for (_, source) in self.pending.drain(..) {
            let _ = AsRef::<AudioScheduledSourceNode>::as_ref(&source).stop();
            let _ = source.disconnect();
        }
    }
}
