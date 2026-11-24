use web_time::{Duration, Instant};

// Bound catch-up work per frame without losing simulated time. Up to
// MAX_STEPS_PER_FRAME ticks run at the fixed `dt`; when more are due (a slow
// frame), the same whole-tick time is covered by that many coarser steps, so
// precision drops but the game keeps real-time speed. The fractional remainder
// stays in the accumulator for render interpolation. Returns (steps, step_dt).
pub(super) fn physics_steps(accumulator: &mut f32, elapsed: f32, dt: f32) -> (u32, f32) {
    const MAX_STEPS_PER_FRAME: u32 = 4;
    let total = f64::from(*accumulator) + f64::from(elapsed);
    let step = f64::from(dt);
    let due = (total / step).floor();
    *accumulator = (total - due * step) as f32;
    let due = due as u32;
    if due <= MAX_STEPS_PER_FRAME {
        (due, dt)
    } else {
        (
            MAX_STEPS_PER_FRAME,
            (f64::from(due) * step / f64::from(MAX_STEPS_PER_FRAME)) as f32,
        )
    }
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
    fn slow_frames_use_coarser_steps_but_keep_real_time() {
        for dt in [1. / 60., 1. / 120.] {
            let mut accumulator = 0.;
            let (steps, step_dt) = physics_steps(&mut accumulator, dt * 12., dt);
            assert_eq!(steps, 4);
            assert!((steps as f32 * step_dt - dt * 12.).abs() < 1e-5);
            assert!(accumulator < dt);
        }
    }

    #[test]
    fn normal_frames_keep_the_fixed_step() {
        for dt in [1. / 60., 1. / 120.] {
            let mut accumulator = 0.;
            assert_eq!(physics_steps(&mut accumulator, dt * 0.5, dt), (0, dt));
            assert_eq!(physics_steps(&mut accumulator, dt * 0.5, dt), (1, dt));
            assert_eq!(physics_steps(&mut accumulator, dt * 4., dt), (4, dt));
            assert_eq!(physics_steps(&mut accumulator, 0., dt), (0, dt));
        }
    }

    #[test]
    fn fractional_time_is_kept_when_capped() {
        let dt = 1. / 120.;
        let mut accumulator = 0.;
        let (steps, step_dt) = physics_steps(&mut accumulator, dt * 8.5, dt);
        assert_eq!(steps, 4);
        assert!((steps as f32 * step_dt - dt * 8.).abs() < 1e-5);
        assert!((accumulator - dt * 0.5).abs() < 1e-5);
    }
}
