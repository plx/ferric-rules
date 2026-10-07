//! Per-engine version of the CLIPS reference platform's additive random stream.

/// The pinned Linux CLIPS 6.30 reference uses glibc's 31-word `random` generator.
/// Keep state inside the engine rather than touching the embedding process RNG.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct RandomState {
    words: [u32; 31],
    front: usize,
}

impl Default for RandomState {
    fn default() -> Self {
        Self::seeded(1)
    }
}

impl RandomState {
    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    pub(crate) fn seeded(seed: u32) -> Self {
        let seed = seed.max(1);
        let mut words = [0; 31];
        words[0] = seed;
        let mut previous = i64::from(seed as i32);
        for word in &mut words[1..] {
            previous = 16_807 * (previous % 127_773) - 2_836 * (previous / 127_773);
            if previous < 0 {
                previous += 2_147_483_647;
            }
            *word = u32::try_from(previous).expect("seed recurrence fits u32");
        }
        let mut state = Self { words, front: 3 };
        for _ in 0..310 {
            state.next();
        }
        state
    }

    pub(crate) fn next(&mut self) -> i64 {
        let rear = (self.front + 28) % 31;
        let next = self.words[self.front].wrapping_add(self.words[rear]);
        self.words[self.front] = next;
        self.front = (self.front + 1) % 31;
        i64::from(next >> 1)
    }

    #[cfg(feature = "serde")]
    pub(crate) fn validate_snapshot(&self) -> Result<(), String> {
        if self.front >= self.words.len() || self.words.iter().all(|word| *word == 0) {
            return Err("invalid random generator state".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RandomState;

    #[test]
    fn pinned_reference_streams_include_unsigned_seed_conversion() {
        for (seed, expected) in [
            (1, [1_804_289_383, 846_930_886, 1_681_692_777]),
            (42, [71_876_166, 708_592_740, 1_483_128_881]),
        ] {
            let mut state = RandomState::seeded(seed);
            assert_eq!([state.next(), state.next(), state.next()], expected);
        }
        let mut state = RandomState::seeded(u32::MAX);
        assert_eq!([state.next(), state.next()], [254_925_627, 1_205_188_300]);
        let mut state = RandomState::seeded(1 << 31);
        assert_eq!([state.next(), state.next()], [1_336_741_213, 1_210_407_648]);
    }

    #[test]
    fn independent_engines_and_copied_state_do_not_share_a_stream() {
        let mut original = RandomState::seeded(42);
        let mut copy = original.clone();
        for _ in 0..100 {
            assert_eq!(original.next(), copy.next());
        }
        assert_eq!(RandomState::seeded(42).next(), 71_876_166);
        assert_eq!(RandomState::seeded(0).next(), RandomState::seeded(1).next());
    }
}
