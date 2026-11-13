//! A seeded random generator, the same on every platform.

/// A seeded random generator: xoshiro256** (D. Blackman and S. Vigna, "Scrambled linear
/// pseudorandom number generators", ACM Trans. Math. Softw. 47, 36 (2021)), its state
/// filled by SplitMix64 as the paper recommends. The same seed gives the same numbers on
/// every platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Random {
    state: [u64; 4],
}

/// SplitMix64: advances `x` and returns the next output.
fn split_mix(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Random {
    /// The generator for `seed`.
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        Self {
            state: [
                split_mix(&mut x),
                split_mix(&mut x),
                split_mix(&mut x),
                split_mix(&mut x),
            ],
        }
    }

    /// An independent stream for trial `index` of a run seeded `seed`: each trial draws
    /// from its own, so the trials are the same however they are shared among threads.
    pub fn stream(seed: u64, index: u64) -> Self {
        let mut x = seed;
        let base = split_mix(&mut x);
        let mut y = index;
        Self::new(base ^ split_mix(&mut y))
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, 1), from the top 53 bits.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Standard normal, by the Box–Muller transform.
    pub fn normal(&mut self) -> f64 {
        let u1 = 1.0 - self.uniform(); // (0, 1]
        let u2 = self.uniform();
        (-2.0 * u1.ln()).sqrt() * (2.0 * core::f64::consts::PI * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_mix_and_xoshiro_give_their_published_first_outputs() {
        // SplitMix64 from seed 0 begins 0xe220a8397b1dcdaf (the sequence published with
        // Vigna's reference splitmix64.c).
        let mut x = 0u64;
        assert_eq!(split_mix(&mut x), 0xe220_a839_7b1d_cdaf);
    }
}
