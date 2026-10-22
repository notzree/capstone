//! Replay timing. Records keep their recorded timestamps; pacing only decides when each is
//! released.

use std::time::{Duration, Instant};

use crate::Record;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pace {
    /// Release each record as soon as the consumer asks for it.
    Unpaced,
    /// Release records at the recorded spacing divided by this factor; 1.0 is real time.
    Speed(f64),
}

pub trait Clock {
    /// Time since an arbitrary fixed origin.
    fn now(&mut self) -> Duration;
    fn sleep_until(&mut self, deadline: Duration);
}

impl<C: Clock + ?Sized> Clock for &mut C {
    fn now(&mut self) -> Duration {
        (**self).now()
    }

    fn sleep_until(&mut self, deadline: Duration) {
        (**self).sleep_until(deadline);
    }
}

#[derive(Debug)]
pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for SystemClock {
    fn now(&mut self) -> Duration {
        self.0.elapsed()
    }

    fn sleep_until(&mut self, deadline: Duration) {
        if let Some(wait) = deadline.checked_sub(self.now()) {
            std::thread::sleep(wait);
        }
    }
}

pub struct Paced<I, C> {
    inner: I,
    pace: Pace,
    clock: C,
    anchor: Option<(u64, Duration)>,
}

impl<I, C> Paced<I, C> {
    pub fn new(inner: I, pace: Pace, clock: C) -> Self {
        Self {
            inner,
            pace,
            clock,
            anchor: None,
        }
    }

    pub fn into_inner(self) -> I {
        self.inner
    }
}

impl<I, C, E> Iterator for Paced<I, C>
where
    I: Iterator<Item = Result<Record, E>>,
    C: Clock,
{
    type Item = Result<Record, E>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.next()?;
        if let (Ok(record), Pace::Speed(speed)) = (&item, self.pace) {
            let clock = &mut self.clock;
            let &mut (first_ns, start) = self
                .anchor
                .get_or_insert_with(|| (record.mono_ns, clock.now()));
            let offset_ns = record.mono_ns.saturating_sub(first_ns) as f64 / speed;
            self.clock
                .sleep_until(start + Duration::from_nanos(offset_ns as u64));
        }
        Some(item)
    }
}
