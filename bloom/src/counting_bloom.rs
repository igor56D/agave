use {
    super::bloom::BloomHashIndex,
    rand::Rng,
    std::sync::atomic::{AtomicU8, Ordering},
};

/// Concurrent counting bloom with u8 counters to support removals.
/// Memory: num_bits bytes (1 byte per bit position) + small key vector.
pub struct ConcurrentCountingBloom<T: BloomHashIndex> {
    num_bits: u64,
    keys: Vec<u64>,
    counts: Vec<AtomicU8>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: BloomHashIndex> ConcurrentCountingBloom<T> {
    pub fn new(num_bits: usize, keys: Vec<u64>) -> Self {
        let num_bits = num_bits.max(1) as u64;
        let counts = (0..num_bits).map(|_| AtomicU8::new(0)).collect();
        Self {
            num_bits,
            keys,
            counts,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn random(num_items: usize, false_positive_rate: f64, max_bits: usize) -> Self {
        let m = Self::num_bits(false_positive_rate, num_items as f64);
        let num_bits = 1usize.max(m.min(max_bits));
        let num_keys = Self::num_keys(num_bits as f64, num_items as f64) as usize;
        let keys: Vec<u64> = (0..num_keys).map(|_| rand::thread_rng().gen()).collect();
        Self::new(num_bits, keys)
    }

    #[inline]
    fn num_bits(false_rate: f64, n: f64) -> usize {
        // same formula as standard bloom
        (((n * false_rate.ln()) / (1f64 / 2f64.powf(2f64.ln())).ln()).ceil() as isize).max(1)
            as usize
    }

    #[inline]
    fn num_keys(m: f64, n: f64) -> f64 {
        if n == 0.0 {
            0.0
        } else {
            1f64.max(((m / n) * 2f64.ln()).round())
        }
    }

    #[inline]
    fn pos(&self, key: &T, hash_index: u64) -> usize {
        key.hash_at_index(hash_index)
            .checked_rem(self.num_bits)
            .unwrap_or(0) as usize
    }

    pub fn contains(&self, key: &T) -> bool {
        self.keys
            .iter()
            .all(|k| self.counts[self.pos(key, *k)].load(Ordering::Relaxed) > 0)
    }

    pub fn add(&self, key: &T) {
        for k in &self.keys {
            let idx = self.pos(key, *k);
            let c = self.counts[idx].load(Ordering::Relaxed);
            if c < u8::MAX {
                self.counts[idx].store(c + 1, Ordering::Relaxed);
            }
        }
    }

    pub fn remove(&self, key: &T) {
        for k in &self.keys {
            let idx = self.pos(key, *k);
            let c = self.counts[idx].load(Ordering::Relaxed);
            if c > 0 {
                self.counts[idx].store(c - 1, Ordering::Relaxed);
            }
        }
    }

    pub fn clear(&self) {
        self.counts
            .iter()
            .for_each(|c| c.store(0, Ordering::Relaxed));
    }
}
