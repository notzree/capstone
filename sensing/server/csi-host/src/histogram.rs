//! Fixed-memory histogram of `u64` values: exact below 1,024, within 1/128 above.

const LINEAR: u64 = 1024;
const LINEAR_BITS: u32 = 10;
const SUB_BITS: u32 = 7;
const BUCKETS: usize = LINEAR as usize + (64 - LINEAR_BITS as usize) * (1 << SUB_BITS);

#[derive(Debug, Clone)]
pub struct Histogram {
    counts: Box<[u64]>,
    total: u64,
    max: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            counts: vec![0; BUCKETS].into_boxed_slice(),
            total: 0,
            max: 0,
        }
    }
}

fn index(value: u64) -> usize {
    if value < LINEAR {
        return value as usize;
    }
    let exp = 63 - value.leading_zeros();
    let mantissa = (value >> (exp - SUB_BITS)) & ((1 << SUB_BITS) - 1);
    LINEAR as usize + ((exp - LINEAR_BITS) as usize) * (1 << SUB_BITS) + mantissa as usize
}

/// Smallest and largest value in bucket `i`.
fn bounds(i: usize) -> (u64, u64) {
    if i < LINEAR as usize {
        return (i as u64, i as u64);
    }
    let k = i - LINEAR as usize;
    let exp = LINEAR_BITS + (k >> SUB_BITS) as u32;
    let step = 1u64 << (exp - SUB_BITS);
    let low = (1u64 << exp) + (k as u64 & ((1 << SUB_BITS) - 1)) * step;
    (low, low + (step - 1))
}

impl Histogram {
    pub fn record(&mut self, value: u64) {
        self.counts[index(value)] += 1;
        self.total += 1;
        self.max = self.max.max(value);
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn max(&self) -> u64 {
        self.max
    }

    /// Nearest-rank quantile, reported as the middle of its bucket; `None` when empty.
    pub fn quantile(&self, q: f64) -> Option<u64> {
        if self.total == 0 {
            return None;
        }
        let rank = ((q.clamp(0.0, 1.0) * self.total as f64).ceil() as u64).clamp(1, self.total);
        let mut seen = 0;
        for (i, &count) in self.counts.iter().enumerate() {
            seen += count;
            if seen >= rank {
                let (low, high) = bounds(i);
                return Some((low + (high - low) / 2).min(self.max));
            }
        }
        Some(self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_below_linear_range() {
        let mut h = Histogram::default();
        for v in 1..=100 {
            h.record(v);
        }
        assert_eq!(h.quantile(0.5), Some(50));
        assert_eq!(h.quantile(0.99), Some(99));
        assert_eq!(h.quantile(1.0), Some(100));
    }

    #[test]
    fn relative_error_is_bounded() {
        for value in [1024, 1500, 10_000, 123_456, 9_999_999, u64::MAX / 3] {
            let mut h = Histogram::default();
            h.record(value);
            h.record(value);
            let got = h.quantile(0.5).unwrap();
            let error = got.abs_diff(value) as f64 / value as f64;
            assert!(error <= 1.0 / 128.0, "{value} -> {got}");
        }
    }

    #[test]
    fn buckets_tile_the_range() {
        for i in 1..BUCKETS {
            assert_eq!(bounds(i - 1).1 + 1, bounds(i).0, "bucket {i}");
            assert_eq!(index(bounds(i).0), i);
            assert_eq!(index(bounds(i).1), i);
        }
        assert_eq!(bounds(BUCKETS - 1).1, u64::MAX);
    }
}
