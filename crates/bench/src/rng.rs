/// SplitMix64: a tiny, well-distributed generator with a fixed algorithm, so a seed produces the
/// same repository on every platform and toolchain. A crate dependency could change its stream
/// between versions and silently change the benchmark dataset.
#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..bound`; `bound` must be non-zero. The modulo bias is below 2^-40 for the
    /// bounds used here, irrelevant for picking files.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    /// True with probability `percent`/100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_fixed() {
        // Reference values of SplitMix64 for seed 0; a change here changes every dataset.
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    }
}
