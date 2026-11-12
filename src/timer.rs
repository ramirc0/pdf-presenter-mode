//! The talk timer: elapsed time, and time left against a planned length or end time.

use std::time::{Duration, Instant};

use jiff::SignedDuration;
use jiff::civil::Time;

/// When the talk must end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// The talk lasts this long, counted from the first slide change.
    Length(Duration),
    /// The talk ends at this local time of day.
    EndsAt(Time),
}

/// The countdown a talk runs, against the one its notes file sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Countdown {
    /// Whatever the notes file sets, on every reload.
    FromNotes,
    /// This goal, whatever the notes file sets.
    Fixed(Goal),
    /// No countdown, whatever the notes file sets.
    Off,
}

impl Countdown {
    /// Puts this countdown into `schedule`, which came from the notes file.
    pub fn apply(self, schedule: &mut Schedule) {
        match self {
            Countdown::FromNotes => {}
            Countdown::Fixed(goal) => schedule.goal = Some(goal),
            Countdown::Off => schedule.goal = None,
        }
    }
}

/// The countdown target and when to warn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// The countdown target, if any.
    pub goal: Option<Goal>,
    /// How many minutes before the end the time left turns amber.
    pub last_minutes: u32,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            goal: None,
            last_minutes: 5,
        }
    }
}

/// How close the talk is to its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urgency {
    /// More than `last_minutes` left.
    Calm,
    /// Within `last_minutes` of the end.
    Closing,
    /// Past the end.
    Over,
}

impl Schedule {
    /// The time left, from `elapsed` talk time or the local time of day `now`.
    /// Negative once the talk runs over. `None` without a goal.
    pub fn remaining(&self, elapsed: Duration, now: Time) -> Option<SignedDuration> {
        let left = match self.goal? {
            Goal::Length(length) => {
                SignedDuration::try_from(length).ok()? - SignedDuration::try_from(elapsed).ok()?
            }
            Goal::EndsAt(end) => {
                let left = end.duration_since(now);
                // An end time just past midnight is tonight, not this morning.
                if left < SignedDuration::from_hours(-12) {
                    left + SignedDuration::from_hours(24)
                } else {
                    left
                }
            }
        };
        Some(left)
    }

    /// How urgent `left` is.
    pub fn urgency(&self, left: SignedDuration) -> Urgency {
        if left.is_negative() {
            Urgency::Over
        } else if left <= SignedDuration::from_mins(i64::from(self.last_minutes)) {
            Urgency::Closing
        } else {
            Urgency::Calm
        }
    }
}

/// A stopwatch for the talk.
#[derive(Debug, Default)]
pub struct Timer {
    /// When the current run started, while running.
    since: Option<Instant>,
    /// Time from earlier runs, before the last pause.
    banked: Duration,
}

impl Timer {
    /// Starts or resumes the timer at `now`.
    pub fn start(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }

    /// Pauses a running timer, or resumes a paused one, at `now`.
    /// A timer that never started stays at zero.
    pub fn toggle_pause(&mut self, now: Instant) {
        match self.since.take() {
            Some(since) => self.banked += now - since,
            None if self.banked > Duration::ZERO => self.since = Some(now),
            None => {}
        }
    }

    /// Stops the timer and sets it back to zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether the timer has not run since it was made or reset.
    pub fn unused(&self) -> bool {
        self.since.is_none() && self.banked == Duration::ZERO
    }

    /// Whether the timer is paused after running.
    pub fn paused(&self) -> bool {
        self.since.is_none() && self.banked > Duration::ZERO
    }

    /// Talk time so far, at `now`.
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.banked + self.since.map_or(Duration::ZERO, |since| now - since)
    }
}

/// `time` as `m:ss`, or `h:mm:ss` from an hour, with a minus sign when negative.
pub fn clock_face(time: SignedDuration) -> String {
    let sign = if time.is_negative() { "-" } else { "" };
    let seconds = time.as_secs().unsigned_abs();
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{sign}{h}:{m:02}:{s:02}")
    } else {
        format!("{sign}{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn the_timer_counts_only_while_running() {
        let t0 = Instant::now();
        let mut timer = Timer::default();
        assert_eq!(timer.elapsed(t0 + MINUTE), Duration::ZERO);
        timer.toggle_pause(t0);
        assert!(timer.unused());
        timer.start(t0);
        timer.toggle_pause(t0 + MINUTE);
        assert!(timer.paused());
        assert_eq!(timer.elapsed(t0 + 5 * MINUTE), MINUTE);
        timer.toggle_pause(t0 + 5 * MINUTE);
        assert_eq!(timer.elapsed(t0 + 6 * MINUTE), 2 * MINUTE);
        timer.start(t0 + 6 * MINUTE);
        assert_eq!(timer.elapsed(t0 + 7 * MINUTE), 3 * MINUTE);
        timer.reset();
        assert!(timer.unused());
    }

    #[test]
    fn a_length_counts_down_from_elapsed_time() {
        let schedule = Schedule {
            goal: Some(Goal::Length(20 * MINUTE)),
            last_minutes: 5,
        };
        let noon = Time::constant(12, 0, 0, 0);
        let left = schedule.remaining(16 * MINUTE, noon).unwrap();
        assert_eq!(left, SignedDuration::from_mins(4));
        assert_eq!(schedule.urgency(left), Urgency::Closing);
        let left = schedule.remaining(21 * MINUTE, noon).unwrap();
        assert_eq!(schedule.urgency(left), Urgency::Over);
        let left = schedule.remaining(MINUTE, noon).unwrap();
        assert_eq!(schedule.urgency(left), Urgency::Calm);
    }

    #[test]
    fn an_end_time_counts_down_on_the_wall_clock() {
        let schedule = Schedule {
            goal: Some(Goal::EndsAt(Time::constant(10, 30, 0, 0))),
            ..Schedule::default()
        };
        let left = schedule.remaining(Duration::ZERO, Time::constant(10, 0, 0, 0));
        assert_eq!(left, Some(SignedDuration::from_mins(30)));
        let left = schedule.remaining(Duration::ZERO, Time::constant(10, 31, 0, 0));
        assert_eq!(left, Some(SignedDuration::from_mins(-1)));
    }

    #[test]
    fn an_end_time_after_midnight_is_tonight() {
        let schedule = Schedule {
            goal: Some(Goal::EndsAt(Time::constant(0, 15, 0, 0))),
            ..Schedule::default()
        };
        let left = schedule.remaining(Duration::ZERO, Time::constant(23, 45, 0, 0));
        assert_eq!(left, Some(SignedDuration::from_mins(30)));
    }

    #[test]
    fn no_goal_means_no_countdown() {
        let left = Schedule::default().remaining(MINUTE, Time::constant(9, 0, 0, 0));
        assert_eq!(left, None);
    }

    #[test]
    fn clock_faces() {
        assert_eq!(clock_face(SignedDuration::from_secs(65)), "1:05");
        assert_eq!(clock_face(SignedDuration::from_secs(3725)), "1:02:05");
        assert_eq!(clock_face(SignedDuration::from_secs(-30)), "-0:30");
    }
}
