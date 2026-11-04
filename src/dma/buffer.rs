//! Uniquely claimed, DMA-capable static storage.

/// Once-claimed static storage for DMA. The claim is never reset: explicitly
/// stopped transfers return the same unique slice, not a second claim token.
pub struct StaticBuffer<const N: usize> {
    storage: core::cell::UnsafeCell<[u32; N]>,
    taken: crate::atomic::AtomicBool,
}

// The only storage accessor requires a static owner and atomically succeeds
// once. It never exposes a shared reference to words accessed by CPU or DMA.
unsafe impl<const N: usize> Sync for StaticBuffer<N> {}

impl<const N: usize> StaticBuffer<N> {
    pub const fn new(words: [u32; N]) -> Self {
        Self {
            storage: core::cell::UnsafeCell::new(words),
            taken: crate::atomic::AtomicBool::new(false),
        }
    }

    pub fn take(&'static self) -> Option<&'static mut [u32; N]> {
        self.taken
            .compare_exchange(
                false,
                true,
                crate::atomic::Ordering::AcqRel,
                crate::atomic::Ordering::Acquire,
            )
            .ok()?;
        Some(unsafe { &mut *self.storage.get() })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    #[test]
    fn single_static_claim() {
        static B: StaticBuffer<4> = StaticBuffer::new([1, 2, 3, 4]);
        let words = B.take().unwrap();
        assert_eq!(*words, [1, 2, 3, 4]);
        assert_eq!(words.as_ptr() as usize & 3, 0);
        words[0] = 7;
        assert!(B.take().is_none());
    }
    #[test]
    fn concurrent_claim_is_unique() {
        static B: StaticBuffer<4> = StaticBuffer::new([0; 4]);
        let mut handles = std::vec::Vec::new();
        for _ in 0..16 {
            handles.push(std::thread::spawn(|| B.take()));
        }
        let mut successes = 0;
        for h in handles {
            if let Some(words) = h.join().unwrap() {
                words[0] = 1;
                successes += 1;
            }
        }
        assert_eq!(successes, 1);
    }
}
