use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Distribution {
    pub unit: String,
    pub count: usize,
    pub median: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

pub fn nearest_rank(sorted: &[f64], percentile: f64) -> f64 {
    assert!(!sorted.is_empty());
    let rank = (percentile * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

pub fn summarize(unit: &str, values: &[f64]) -> Option<Distribution> {
    if values.is_empty() { return None; }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(Distribution {
        unit: unit.into(), count: sorted.len(),
        median: nearest_rank(&sorted, 0.5), p95: nearest_rank(&sorted, 0.95),
        p99: nearest_rank(&sorted, 0.99), max: *sorted.last().unwrap(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStages {
    pub simulation_ms: f64,
    pub transport_ms: f64,
    pub pack_build_ms: f64,
    pub host_overhead_ms: f64,
    pub adapter_submission_ms: f64,
}

impl FrameStages {
    pub fn headroom(self, period_ms: f64) -> (f64, f64, f64) {
        let byo = period_ms - self.simulation_ms - self.transport_ms - self.host_overhead_ms;
        let bundled = byo - self.pack_build_ms;
        let end_to_end = bundled - self.adapter_submission_ms;
        (byo, bundled, end_to_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_and_negative_headroom_are_preserved() {
        let d = summarize("ms", &[5.0, 1.0, 4.0, 2.0, 3.0]).unwrap();
        assert_eq!((d.median, d.p95, d.p99, d.max), (3.0, 5.0, 5.0, 5.0));
        let stages = FrameStages { simulation_ms: 10.0, transport_ms: 1.0, pack_build_ms: 2.0, host_overhead_ms: 1.0, adapter_submission_ms: 4.0 };
        assert_eq!(stages.headroom(16.0), (4.0, 2.0, -2.0));
    }
}
