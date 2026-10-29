use web_time::{Duration, Instant};

#[derive(Default)]
pub(super) struct FrameRate {
    last_present: Option<Instant>,
    elapsed: Duration,
    frames: u32,
    pub(super) value: Option<f32>,
}
impl FrameRate {
    pub(super) fn presented(&mut self, now: Instant) {
        if let Some(last) = self.last_present {
            self.elapsed += now - last;
            self.frames += 1;
            if self.elapsed >= Duration::from_millis(500) {
                self.value = Some(self.frames as f32 / self.elapsed.as_secs_f32());
                self.elapsed = Duration::ZERO;
                self.frames = 0;
            }
        }
        self.last_present = Some(now);
    }
}
