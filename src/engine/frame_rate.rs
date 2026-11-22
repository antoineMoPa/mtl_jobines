use web_time::{Duration, Instant};

// Bound catch-up work after a stall. Keep the substep phase for interpolation,
// but discard overdue whole ticks so they cannot stall the next frame too.
pub(super) fn physics_steps(accumulator: &mut f32, elapsed: f32, dt: f32) -> u32 {
    const MAX_STEPS_PER_FRAME: u32 = 4;
    let total = f64::from(*accumulator) + f64::from(elapsed);
    let step = f64::from(dt);
    let due = (total / step).floor();
    *accumulator = (total - due * step) as f32;
    (due as u32).min(MAX_STEPS_PER_FRAME)
}

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

#[cfg(test)]
mod tests {
    use super::physics_steps;

    #[test]
    fn catch_up_drops_excess_ticks_and_keeps_fractional_time() {
        for dt in [1. / 60., 1. / 120.] {
            let mut accumulator = 0.;
            assert_eq!(physics_steps(&mut accumulator, dt * 0.5, dt), 0);
            assert_eq!(physics_steps(&mut accumulator, dt * 16., dt), 4);
            assert_eq!(physics_steps(&mut accumulator, 0., dt), 0);
            assert_eq!(physics_steps(&mut accumulator, dt * 0.5, dt), 1);
            assert_eq!(physics_steps(&mut accumulator, dt * 2., dt), 2);
            assert_eq!(accumulator, 0.);
        }
    }
}
