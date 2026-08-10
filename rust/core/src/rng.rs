//! Deterministic PRNG — a faithful port of the mulberry32 used by the TS
//! reference sim, including JS's ToInt32 coercion semantics, so the two
//! implementations can be diffed run-for-run while both exist.

#[derive(Clone, Debug)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// Uniform in [0, 1). Matches the TS sim's `rand()` bit-for-bit.
    pub fn next_f64(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }

    /// Uniform integer in [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        (self.next_f64() * n as f64) as usize
    }

    /// Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = self.below(i + 1);
            xs.swap(i, j);
        }
    }
}
