//! Accounting for a wrapping `u32` sequence: gaps, duplicates and late arrivals.

use std::collections::BTreeMap;

/// A gap is final (counted as missing) once the highest sequence seen is this far past it; until
/// then a late arrival can still fill it.
pub const WINDOW: u64 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    First,
    InOrder,
    /// Arrived ahead of this many unseen predecessors.
    Gap(u64),
    /// Filled an open gap.
    Late,
    Duplicate,
    /// Older than the window, or older than the first value seen; cannot be classified.
    Stale,
}

impl Arrival {
    /// The value had not been seen before.
    pub const fn is_new(self) -> bool {
        matches!(
            self,
            Self::First | Self::InOrder | Self::Gap(_) | Self::Late
        )
    }
}

/// Values that never arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Missing {
    pub first: u32,
    pub count: u64,
    /// The `at` of the arrival that revealed the gap.
    pub seen_at: u64,
}

#[derive(Debug, Clone)]
struct Hole {
    end: u64,
    seen_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SeqTracker {
    max: Option<u64>,
    first: u64,
    /// Open gaps keyed by first missing value, in widened sequence space.
    holes: BTreeMap<u64, Hole>,
    finalized: Vec<Missing>,
    pub received: u64,
    pub missing: u64,
    pub duplicates: u64,
    pub late: u64,
    pub stale: u64,
}

impl SeqTracker {
    /// Records an arrival; `at` is any caller timestamp, kept to date the gaps it reveals.
    pub fn push(&mut self, seq: u32, at: u64) -> Arrival {
        let Some(max) = self.max else {
            let wide = (1 << 32) | u64::from(seq);
            self.max = Some(wide);
            self.first = wide;
            self.received += 1;
            return Arrival::First;
        };
        let wide = max.saturating_add_signed(i64::from(seq.wrapping_sub(max as u32) as i32));
        let arrival = if wide > max {
            self.max = Some(wide);
            if wide > max + 1 {
                self.holes.insert(
                    max + 1,
                    Hole {
                        end: wide,
                        seen_at: at,
                    },
                );
                Arrival::Gap(wide - max - 1)
            } else {
                Arrival::InOrder
            }
        } else if wide < self.first || wide.saturating_add(WINDOW) < max {
            Arrival::Stale
        } else if let Some((start, hole)) = self.hole_containing(wide) {
            self.holes.remove(&start);
            if start < wide {
                self.holes.insert(start, Hole { end: wide, ..hole });
            }
            if wide + 1 < hole.end {
                self.holes.insert(wide + 1, hole);
            }
            Arrival::Late
        } else {
            Arrival::Duplicate
        };
        match arrival {
            Arrival::Duplicate => self.duplicates += 1,
            Arrival::Stale => self.stale += 1,
            Arrival::Late => {
                self.late += 1;
                self.received += 1;
            }
            _ => self.received += 1,
        }
        self.finalize_through(self.max.unwrap_or(0).saturating_sub(WINDOW));
        arrival
    }

    fn hole_containing(&self, wide: u64) -> Option<(u64, Hole)> {
        self.holes
            .range(..=wide)
            .next_back()
            .filter(|(_, hole)| wide < hole.end)
            .map(|(&start, hole)| (start, hole.clone()))
    }

    fn finalize_through(&mut self, limit: u64) {
        while let Some((&start, hole)) = self.holes.first_key_value() {
            if start >= limit {
                break;
            }
            // Finalize only values below the reordering boundary. A large hole may
            // straddle it; its suffix can still be filled by late arrivals.
            let end = hole.end.min(limit);
            let count = end - start;
            self.finalized.push(Missing {
                first: start as u32,
                count,
                seen_at: hole.seen_at,
            });
            self.missing += count;
            let (_, hole) = self.holes.pop_first().unwrap();
            if end < hole.end {
                self.holes.insert(end, hole);
            }
        }
    }

    /// Closes every open gap; call when the sequence ends.
    pub fn finish(&mut self) {
        self.finalize_through(u64::MAX);
    }

    /// Gaps finalized since the last call.
    pub fn take_finalized(&mut self) -> Vec<Missing> {
        std::mem::take(&mut self.finalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(seqs: &[u32]) -> (SeqTracker, Vec<Arrival>) {
        let mut t = SeqTracker::default();
        let arrivals = seqs.iter().map(|&s| t.push(s, u64::from(s))).collect();
        t.finish();
        (t, arrivals)
    }

    #[test]
    fn in_order_and_gaps() {
        let (mut t, arrivals) = feed(&[5, 6, 9, 10]);
        assert_eq!(
            arrivals,
            [
                Arrival::First,
                Arrival::InOrder,
                Arrival::Gap(2),
                Arrival::InOrder
            ]
        );
        assert_eq!((t.received, t.missing), (4, 2));
        assert_eq!(
            t.take_finalized(),
            [Missing {
                first: 7,
                count: 2,
                seen_at: 9
            }]
        );
    }

    #[test]
    fn late_arrivals_fill_gaps() {
        let (t, arrivals) = feed(&[1, 4, 2, 5, 3]);
        assert_eq!(arrivals[2], Arrival::Late);
        assert_eq!(arrivals[4], Arrival::Late);
        assert_eq!((t.received, t.missing, t.late), (5, 0, 2));
    }

    #[test]
    fn split_gaps_keep_when_they_were_seen() {
        let (mut t, _) = feed(&[0, 10, 5]);
        assert_eq!(t.missing, 8);
        let seen: Vec<_> = t
            .take_finalized()
            .iter()
            .map(|m| (m.first, m.count, m.seen_at))
            .collect();
        assert_eq!(seen, [(1, 4, 10), (6, 4, 10)]);
    }

    #[test]
    fn duplicates_and_stale() {
        let (t, arrivals) = feed(&[10, 11, 11, 10, 9]);
        assert_eq!(
            arrivals[2..],
            [Arrival::Duplicate, Arrival::Duplicate, Arrival::Stale]
        );
        assert_eq!((t.received, t.duplicates, t.stale), (2, 2, 1));
    }

    #[test]
    fn wraps_around_u32() {
        let (t, arrivals) = feed(&[u32::MAX - 1, u32::MAX, 1, 0]);
        assert_eq!(arrivals[2], Arrival::Gap(1));
        assert_eq!(arrivals[3], Arrival::Late);
        assert_eq!((t.received, t.missing), (4, 0));
    }

    #[test]
    fn gaps_finalize_once_out_of_window() {
        let mut t = SeqTracker::default();
        t.push(0, 0);
        t.push(10, 0);
        assert_eq!(t.missing, 0);
        t.push(10 + WINDOW as u32, 0);
        assert_eq!(t.missing, 9);
        assert_eq!(t.push(5, 0), Arrival::Stale);
    }

    #[test]
    fn large_gap_keeps_one_pending_range_and_finalizes_in_two_parts() {
        let mut t = SeqTracker::default();
        t.push(0, 0);
        t.push(1_000_000, 10);
        assert_eq!(t.holes.len(), 1);
        t.finish();
        assert_eq!(t.missing, 999_999);
        let finalized = t.take_finalized();
        assert_eq!(finalized.len(), 2);
        assert_eq!(finalized.iter().map(|m| m.count).sum::<u64>(), 999_999);
        assert!(finalized.iter().all(|m| m.seen_at == 10));
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    #[test]
    fn regression_large_jump_expires_prefix_but_keeps_boundary_late() {
        let mut tracker = SeqTracker::default();
        tracker.push(0, 0);
        tracker.push(1_000_000, 10);
        let boundary = 1_000_000 - WINDOW as u32;
        assert_eq!(tracker.missing, u64::from(boundary - 1));
        assert_eq!(
            tracker.take_finalized(),
            vec![Missing {
                first: 1,
                count: u64::from(boundary - 1),
                seen_at: 10
            }]
        );
        assert_eq!(tracker.push(5, 11), Arrival::Stale);
        assert_eq!(tracker.push(boundary, 12), Arrival::Late);
        tracker.finish();
        assert_eq!(tracker.missing, 999_998);
    }

    #[test]
    fn regression_large_jump_across_wrap_preserves_window_and_gap_timestamp() {
        let mut tracker = SeqTracker::default();
        let start = u32::MAX - 500;
        tracker.push(start, 1);
        tracker.push(start.wrapping_add(5000), 7);
        assert_eq!(tracker.missing, 5000 - WINDOW - 1);
        assert_eq!(tracker.push(start.wrapping_add(1), 8), Arrival::Stale);
        assert_eq!(
            tracker.push(start.wrapping_add(5000 - WINDOW as u32), 9),
            Arrival::Late
        );
        tracker.finish();
        assert_eq!(tracker.missing, 4998);
        assert!(tracker.take_finalized().iter().all(|m| m.seen_at == 7));
    }
}
