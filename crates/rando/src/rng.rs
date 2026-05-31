//! Deterministic, version-stable PRNG for reproducible randomizer seeds.
//!
//! A randomizer's whole contract is that a given seed always produces the same
//! result. Pulling that from an external crate's generator risks the output
//! shifting when the dependency changes its algorithm, so the randomizer uses
//! its own [`SplitMix64`] — a tiny, well-known generator whose output is fixed
//! forever by the constants below. Same seed in, same byte-stream out, on any
//! machine and any build.

/// SplitMix64 (Steele, Lea & Flood). Stateless-looking, fast, and stable.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Seed the generator. Any `u64` is a valid seed.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64-bit output.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish integer in `0..n` (`n > 0`). Modulo bias is negligible for
    /// the small ranges a randomizer uses (item pools, list lengths).
    pub fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0, "below(0) is undefined");
        (self.next_u64() % n as u64) as usize
    }

    /// In-place Fisher-Yates shuffle, driven by this generator.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = SplitMix64::new(0xDEAD_BEEF);
        let mut b = SplitMix64::new(0xDEAD_BEEF);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_diverges() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(2);
        let differ = (0..64).any(|_| a.next_u64() != b.next_u64());
        assert!(differ);
    }

    #[test]
    fn known_vector_is_stable() {
        // Pin the first output for seed 0 so a future refactor can't silently
        // change the generated stream (which would break every published seed).
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut r = SplitMix64::new(42);
        let mut v: Vec<u32> = (0..100).collect();
        let orig = v.clone();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, orig, "shuffle must preserve the multiset");
        assert_ne!(v, orig, "100 elements should not stay in order");
    }
}
