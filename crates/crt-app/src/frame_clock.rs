// ABOUTME: Frame pacing for the focused, drawable terminal window.

use std::time::{Duration, Instant};

pub(crate) struct FrameClock {
    interval: Duration,
    active: bool,
    last_frame: Option<Instant>,
}

pub(crate) struct FrameTick {
    pub elapsed: Duration,
    pub resumed: bool,
}

impl FrameClock {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            active: false,
            last_frame: None,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Returns true when entering the active state, to request a repaint now.
    pub fn update_window(&mut self, focused: bool, hidden: bool, occluded: bool) -> bool {
        let was_active = self.active;
        self.active = focused && !hidden && !occluded;
        if !self.active {
            self.last_frame = None;
        }
        self.active && !was_active
    }

    pub fn is_due(&self, now: Instant) -> bool {
        self.active
            && self
                .last_frame
                .is_none_or(|last| now - last >= self.interval)
    }

    pub fn begin_frame(&mut self, now: Instant) -> Option<FrameTick> {
        if !self.is_due(now) {
            return None;
        }
        let previous = self.last_frame.replace(now);
        Some(FrameTick {
            elapsed: previous.map_or(self.interval, |last| now - last),
            resumed: previous.is_none(),
        })
    }

    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        if !self.active {
            return None;
        }
        // If the OS doesn't deliver redraws, never re-arm an expired deadline
        // and spin. Retry after a frame interval instead.
        Some(
            self.last_frame
                .map(|last| last + self.interval)
                .filter(|deadline| *deadline > now)
                .unwrap_or(now + self.interval),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_millis(8);

    #[test]
    fn inactive_window_neither_schedules_nor_accepts_frames() {
        let now = Instant::now();
        let mut clock = FrameClock::new(INTERVAL);
        assert!(clock.next_deadline(now).is_none());
        for focused in [false, true] {
            for hidden in [false, true] {
                for occluded in [false, true] {
                    if focused && !hidden && !occluded {
                        continue;
                    }
                    assert!(clock.update_window(true, false, false));
                    assert!(clock.begin_frame(now).is_some());
                    assert!(!clock.update_window(focused, hidden, occluded));
                    assert!(clock.next_deadline(now).is_none());
                    assert!(!clock.is_due(now));
                    assert!(clock.begin_frame(now).is_none());
                }
            }
        }
    }

    #[test]
    fn refocus_paints_immediately_without_counting_time_spent_in_background() {
        let now = Instant::now();
        let mut clock = FrameClock::new(INTERVAL);
        assert!(clock.update_window(true, false, false));
        assert!(clock.begin_frame(now).unwrap().resumed);
        assert!(!clock.update_window(true, false, false));
        assert!(clock.begin_frame(now).is_none());
        assert_eq!(clock.next_deadline(now), Some(now + INTERVAL));

        clock.update_window(false, false, false);
        let later = now + Duration::from_secs(300);
        assert!(clock.update_window(true, false, false));
        let tick = clock.begin_frame(later).unwrap();
        assert!(tick.resumed);
        assert_eq!(tick.elapsed, INTERVAL);
        assert!(!clock.begin_frame(later + INTERVAL).unwrap().resumed);
    }

    #[test]
    fn a_missed_redraw_does_not_leave_an_expired_timer() {
        let now = Instant::now();
        let mut clock = FrameClock::new(INTERVAL);
        clock.update_window(true, false, false);
        clock.begin_frame(now).unwrap();

        let missed = now + INTERVAL;
        assert!(clock.is_due(missed));
        assert_eq!(clock.next_deadline(missed), Some(missed + INTERVAL));
    }
}
