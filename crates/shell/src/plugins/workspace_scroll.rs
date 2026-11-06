//! Wheel/trackpad normalization with no queued focus requests or frame timer.
use iced::mouse::ScrollDelta;
use std::time::{Duration, Instant};
#[derive(Default)]
pub(super) struct Scroll {
    output: String,
    remainder: f32,
    last_event: Option<Instant>,
    last_switch: Option<Instant>,
}
impl Scroll {
    pub fn step(
        &mut self,
        output: &str,
        delta: ScrollDelta,
        busy: bool,
        now: Instant,
    ) -> Option<i32> {
        let (x, y, pixels) = match delta {
            ScrollDelta::Lines { x, y } => (x, y, false),
            ScrollDelta::Pixels { x, y } => (x, y, true),
        };
        let value = if y.abs() >= x.abs() { y } else { x };
        if !value.is_finite() || value == 0. {
            return None;
        }
        if self.output != output
            || self
                .last_event
                .is_none_or(|last| now.saturating_duration_since(last) > Duration::from_millis(400))
        {
            self.remainder = 0.;
        }
        self.output = output.into();
        self.last_event = Some(now);
        if busy
            || self.last_switch.is_some_and(|last| {
                now.saturating_duration_since(last) < Duration::from_millis(120)
            })
        {
            self.remainder = 0.;
            return None;
        }
        if pixels {
            if self.remainder.signum() != value.signum() {
                self.remainder = 0.;
            }
            self.remainder += value;
            if self.remainder.abs() < 40. {
                return None;
            }
        }
        self.remainder = 0.;
        self.last_switch = Some(now);
        // Iced reports up/left as positive; next is down/right.
        Some(if value < 0. { 1 } else { -1 })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discrete_scroll_is_directional_bounded_and_drops_pending_bursts() {
        let mut s = Scroll::default();
        let t = Instant::now();
        let line = |x, y| ScrollDelta::Lines { x, y };
        assert_eq!(s.step("A", line(0., -4.), false, t), Some(1));
        assert_eq!(
            s.step("A", line(0., -1.), false, t + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            s.step("A", line(0., 1.), true, t + Duration::from_millis(200)),
            None
        );
        assert_eq!(
            s.step("A", line(0., 1.), false, t + Duration::from_millis(201)),
            Some(-1)
        );
        assert_eq!(
            s.step("B", line(-1., 0.), false, t + Duration::from_millis(400)),
            Some(1)
        );
        assert_eq!(
            s.step(
                "B",
                line(0., f32::NAN),
                false,
                t + Duration::from_millis(600)
            ),
            None
        );
    }
    #[test]
    fn trackpad_threshold_resets_on_direction_output_and_gesture_gap() {
        let mut s = Scroll::default();
        let t = Instant::now();
        let pixel = |y| ScrollDelta::Pixels { x: 0., y };
        assert_eq!(s.step("A", pixel(-25.), false, t), None);
        assert_eq!(
            s.step("B", pixel(-20.), false, t + Duration::from_millis(10)),
            None
        );
        assert_eq!(
            s.step("B", pixel(20.), false, t + Duration::from_millis(20)),
            None
        );
        assert_eq!(
            s.step("B", pixel(20.), false, t + Duration::from_millis(30)),
            Some(-1)
        );
        assert_eq!(
            s.step("A", pixel(-25.), false, t + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            s.step("A", pixel(-25.), false, t + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            s.step("A", pixel(-15.), false, t + Duration::from_millis(2010)),
            Some(1)
        );
    }
}
