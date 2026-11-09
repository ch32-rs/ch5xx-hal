use super::MAX_PACKET;

/// Endpoint packet buffer. DMA reads and writes it, so access is volatile.
#[repr(C, align(4))]
pub(super) struct Buffer {
    data: [u8; MAX_PACKET],
}

impl Buffer {
    pub(super) const fn new() -> Self {
        Self { data: [0; MAX_PACKET] }
    }

    pub(super) fn read(&self, out: &mut [u8]) {
        for (slot, byte) in out.iter_mut().enumerate() {
            *byte = unsafe { core::ptr::read_volatile(self.data.as_ptr().add(slot)) };
        }
    }

    pub(super) fn write(&mut self, input: &[u8]) {
        for (slot, byte) in input.iter().enumerate() {
            unsafe { core::ptr::write_volatile(self.data.as_mut_ptr().add(slot), *byte) };
        }
    }

    /// The value the DMA register takes: the low 16 bits, 4-byte aligned.
    pub(super) fn dma_address(&self) -> u16 {
        let address = self.data.as_ptr() as usize;
        debug_assert!(address % 4 == 0, "usb buffer is not 4-byte aligned");
        (address & 0xFFFF) as u16
    }
}

/// Endpoint 4 has no DMA register. Its single-direction buffer immediately
/// follows endpoint 0's; allocation forbids using both directions at once.
#[repr(C, align(4))]
pub(super) struct Ep0Buffers {
    pub(super) control: Buffer,
    pub(super) ep4: Buffer,
}

/// Select the shared packet buffer for an endpoint, independent of direction.
///
/// The caller must provide valid storage, an index below 8, and exclusive access
/// to the selected buffer for the returned reference's lifetime.
pub(super) unsafe fn buffer<'a>(ep0: *mut Ep0Buffers, endpoints: *mut [Buffer; 7], index: usize) -> &'a mut Buffer {
    match index {
        0 => &mut (*ep0).control,
        4 => &mut (*ep0).ep4,
        i => &mut *endpoints.cast::<Buffer>().add(i - 1),
    }
}
