//! Benchmark support for Ripplepath: a deterministic synthetic repository generator, a tiny Git
//! driver for repositories it creates, summary statistics and host probes.
//!
//! Everything measured is produced by `ripplepath-bench` (see docs/BENCHMARKS.md); nothing here
//! invents or extrapolates a number.

pub mod git;
pub mod host;
pub mod rng;
pub mod stats;
pub mod synth;
