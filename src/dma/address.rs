//! Checked peripheral-local DMA descriptors, independent of MMIO.
//!
//! Every CH5xx DMA engine stores a 17-bit SRAM offset rather than a bus
//! address, so a descriptor is a `[start, end)` byte range below the 128 KiB
//! SRAM window. The window is the same for the timer, SPI, ADC and LED engines;
//! only the transfer unit and the peripheral-side behaviour differ.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    Empty,
    Unaligned,
    Address,
    Length,
    InvalidMode,
    InvalidWidth,
    Fifo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub(crate) start: u32,
    pub(crate) end: u32,
}
impl Descriptor {
    /// CH585 SRAM window; address bits 14/15/16 were independently probed.
    /// Exclusive END itself must be representable, so the top word is reserved.
    ///
    /// `words` is in 32-bit timer words, the unit of the timer DMA engine.
    pub fn new(address: usize, words: usize) -> Result<Self, Error> {
        let bytes = words.checked_mul(4).ok_or(Error::Length)?;
        Self::bytes(address, bytes)
    }

    /// A byte-addressed descriptor for engines that move bytes, such as SPI.
    ///
    /// The start is kept 32-bit aligned even though the registers decode byte
    /// offsets: the vendor documents byte alignment, and the stricter rule also
    /// keeps every chunk of a split transfer aligned. `end` is exclusive.
    pub fn bytes(address: usize, bytes: usize) -> Result<Self, Error> {
        if bytes == 0 {
            return Err(Error::Empty);
        }
        if address & 3 != 0 {
            return Err(Error::Unaligned);
        }
        let end = address.checked_add(bytes).ok_or(Error::Length)?;
        if address < 0x2000_0000 || end >= 0x2002_0000 {
            return Err(Error::Address);
        }
        Ok(Self {
            start: (address - 0x2000_0000) as u32,
            end: (end - 0x2000_0000) as u32,
        })
    }

    /// Transfer size in bytes.
    pub const fn len(self) -> usize {
        (self.end - self.start) as usize
    }

    /// Transfer size in 32-bit timer words.
    pub const fn words(self) -> usize {
        self.len() / 4
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptors() {
        assert_eq!(Descriptor::new(0x2000_0010, 8).unwrap().words(), 8);
        assert_eq!(Descriptor::new(0x2000_0000, 0), Err(Error::Empty));
        assert_eq!(Descriptor::new(0x2000_0001, 1), Err(Error::Unaligned));
        assert_eq!(Descriptor::new(0x1fff_fffc, 1), Err(Error::Address));
        assert_eq!(Descriptor::new(0x2001_fffc, 1), Err(Error::Address));
        assert_eq!(Descriptor::new(0x2000_0000, usize::MAX), Err(Error::Length));
        assert_eq!(Descriptor::new(usize::MAX - 3, 2), Err(Error::Length));
    }
    #[test]
    fn byte_descriptors() {
        let d = Descriptor::bytes(0x2000_0010, 13).unwrap();
        assert_eq!(d.start, 0x10);
        assert_eq!(d.end, 0x1d);
        assert_eq!(d.len(), 13);
        assert_eq!(Descriptor::bytes(0x2000_0000, 0), Err(Error::Empty));
        assert_eq!(Descriptor::bytes(0x2000_0001, 1), Err(Error::Unaligned));
        assert_eq!(Descriptor::bytes(0x2001_ffff, 1), Err(Error::Address));
        assert_eq!(Descriptor::bytes(usize::MAX - 1, 4), Err(Error::Length));
    }
    #[test]
    fn complete_sram_boundary_sweep() {
        for offset in (0..0x20000).step_by(4) {
            let address = 0x2000_0000 + offset;
            assert_eq!(Descriptor::new(address, 1).is_ok(), offset + 4 < 0x20000);
            if offset + 8 < 0x20000 {
                let d = Descriptor::new(address, 2).unwrap();
                assert_eq!(d.start as usize, offset);
                assert_eq!(d.end as usize, offset + 8);
            }
        }
    }
}
