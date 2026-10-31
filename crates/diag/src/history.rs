//! The recent past the live view draws: resource samples ([`History`]) and how long frames took
//! ([`Frames`]).

use std::collections::VecDeque;

use crate::Sample;

/// The last samples, oldest first, at most `capacity` of them.
#[derive(Debug, Clone)]
pub struct History {
    samples: VecDeque<Sample>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> History {
        History { samples: VecDeque::with_capacity(capacity), capacity: capacity.max(1) }
    }

    pub fn push(&mut self, s: Sample) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(s);
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// One value of every sample, oldest first.
    pub fn series(&self, value: impl Fn(&Sample) -> f32) -> Vec<f32> {
        self.samples.iter().map(value).collect()
    }

    /// The highest value of any sample.
    pub fn peak(&self, value: impl Fn(&Sample) -> f32) -> f32 {
        self.samples.iter().map(value).fold(0.0, f32::max)
    }
}

/// How long the last frames took to draw and when they were drawn, for frame time and frames per
/// second.
#[derive(Debug, Clone, Default)]
pub struct Frames {
    /// (time the frame started, in seconds; time it took, in seconds).
    frames: VecDeque<(f64, f32)>,
}

/// Frames kept: a few seconds at a high refresh rate.
const FRAMES_KEPT: usize = 600;

impl Frames {
    /// A frame drawn at `at` (seconds, any clock that only grows) that took `took` seconds.
    pub fn record(&mut self, at: f64, took: f32) {
        if self.frames.len() == FRAMES_KEPT {
            self.frames.pop_front();
        }
        self.frames.push_back((at, took.max(0.0)));
    }

    /// Frames drawn in the second before the last one. Docufy draws only when something changes,
    /// so a still window shows few.
    pub fn per_second(&self) -> usize {
        let Some(&(last, _)) = self.frames.back() else { return 0 };
        self.frames.iter().filter(|(at, _)| last - at < 1.0).count()
    }

    /// The median and 95th-percentile frame time of the kept frames, in milliseconds.
    pub fn times_ms(&self) -> Option<(f32, f32)> {
        if self.frames.is_empty() {
            return None;
        }
        let mut t: Vec<f32> = self.frames.iter().map(|(_, took)| took * 1000.0).collect();
        t.sort_by(f32::total_cmp);
        let at = |q: f32| t[((t.len() - 1) as f32 * q).round() as usize];
        Some((at(0.5), at(0.95)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_the_newest_samples() {
        let mut h = History::new(3);
        for i in 1..=5 {
            h.push(Sample { cpu: i as f32 * 10.0, ..Default::default() });
        }
        assert_eq!(h.len(), 3);
        assert_eq!(h.series(|s| s.cpu), vec![30.0, 40.0, 50.0]);
        assert_eq!(h.peak(|s| s.cpu), 50.0);
        assert_eq!(h.latest().map(|s| s.cpu), Some(50.0));
    }

    #[test]
    fn frames_give_rate_and_percentiles() {
        let mut f = Frames::default();
        assert_eq!(f.times_ms(), None);
        assert_eq!(f.per_second(), 0);
        // 60 frames in one second, one of them slow.
        for i in 0..60 {
            f.record(10.0 + i as f64 / 60.0, if i == 30 { 0.050 } else { 0.004 });
        }
        assert_eq!(f.per_second(), 60);
        let (median, p95) = f.times_ms().unwrap();
        assert!((median - 4.0).abs() < 0.01, "{median}");
        assert!((p95 - 4.0).abs() < 0.01, "one slow frame in 60 stays out of the 95th percentile: {p95}");
        for i in 0..10 {
            f.record(12.0 + i as f64, 0.030);
        }
        assert_eq!(f.per_second(), 1, "frames more than a second old don't count");
        assert!(f.times_ms().unwrap().1 >= 30.0);
    }
}
