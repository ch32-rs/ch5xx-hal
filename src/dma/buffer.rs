//! Uniquely claimed, DMA-capable static storage.

/// Once-claimed static storage for DMA. The claim is never reset: explicitly
/// stopped transfers return the same unique slice, not a second claim token.
///
/// `T` is the transfer unit of the owning engine: `u32` for the timer FIFO and
/// `u8` for SPI. The storage is 4-byte aligned, which satisfies every engine.
#[repr(align(4))]
struct Aligned<T: Copy, const N: usize>([T; N]);

pub struct StaticBufferImpl<T: Copy + Send, const N: usize> {
    storage: core::cell::UnsafeCell<Aligned<T, N>>,
    taken: crate::atomic::AtomicBool,
}

/// Timer DMA storage, one 32-bit word per FIFO entry.
pub type StaticBuffer<const N: usize> = StaticBufferImpl<u32, N>;

/// Byte-oriented DMA storage for peripherals such as SPI0.
pub type StaticByteBuffer<const N: usize> = StaticBufferImpl<u8, N>;

/// 16-bit sample storage for the ADC's local DMA engine.
pub type StaticSampleBuffer<const N: usize> = StaticBufferImpl<u16, N>;

// The only storage accessor requires a static owner and atomically succeeds
// once. It never exposes a shared reference to words accessed by CPU or DMA.
unsafe impl<T: Copy + Send, const N: usize> Sync for StaticBufferImpl<T, N> {}

impl<T: Copy + Send, const N: usize> StaticBufferImpl<T, N> {
    pub const fn new(value: [T; N]) -> Self {
        Self {
            storage: core::cell::UnsafeCell::new(Aligned(value)),
            taken: crate::atomic::AtomicBool::new(false),
        }
    }

    pub fn take(&'static self) -> Option<&'static mut [T; N]> {
        self.taken
            .compare_exchange(
                false,
                true,
                crate::atomic::Ordering::AcqRel,
                crate::atomic::Ordering::Acquire,
            )
            .ok()?;
        Some(unsafe { &mut (*self.storage.get()).0 })
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
    #[test]
    fn byte_storage_is_claimed_once() {
        static B: StaticByteBuffer<8> = StaticByteBuffer::new([0; 8]);
        let bytes = B.take().unwrap();
        assert_eq!(bytes.as_ptr() as usize & 3, 0);
        bytes[0] = 1;
        assert!(B.take().is_none());
    }
}
