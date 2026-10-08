use serde::{Deserialize, Serialize};

/// Order statistics of a sample. Percentiles use the nearest-rank method: every reported value is
/// one that was actually measured, never an interpolation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub n: usize,
    pub min: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
    pub mean: f64,
}

pub fn summarize(samples: &[f64]) -> Option<Summary> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let rank = |p: f64| -> f64 {
        let index = ((p * n as f64).ceil() as usize).clamp(1, n) - 1;
        sorted[index]
    };
    Some(Summary {
        n,
        min: sorted[0],
        median: rank(0.5),
        p95: rank(0.95),
        max: sorted[n - 1],
        mean: sorted.iter().sum::<f64>() / n as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank_percentiles() {
        let samples: Vec<f64> = (1..=20).map(f64::from).rev().collect();
        let s = summarize(&samples).unwrap();
        assert_eq!((s.min, s.median, s.p95, s.max), (1.0, 10.0, 19.0, 20.0));
        assert_eq!(summarize(&[3.0]).unwrap().p95, 3.0);
        assert!(summarize(&[]).is_none());
    }
}
