//! Value types kept independent of MMIO for host tests.

pub const MAX_PERIOD: u32 = 0x03ff_ffff;
pub const MAX_CAPTURE_PERIOD: u32 = 0x01ff_ffff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    InvalidPeriod,
    InvalidDuty,
    ClockTooSlow,
    FifoOverrun,
    WaveformUnderrun,
}

/// Supported CNT_END value (2..=MAX_PERIOD). On CH585, 1 does not reload.
/// No implicit zero-as-maximum encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeriodTicks(u32);
impl PeriodTicks {
    pub const fn new(ticks: u32) -> Result<Self, Error> {
        if ticks < 2 || ticks > MAX_PERIOD {
            Err(Error::InvalidPeriod)
        } else {
            Ok(Self(ticks))
        }
    }
    pub const fn get(self) -> u32 {
        self.0
    }
    /// Capture reserves bit 25 for level/timeout metadata, not interval ticks.
    pub const fn new_capture(ticks: u32) -> Result<Self, Error> {
        if ticks > MAX_CAPTURE_PERIOD {
            return Err(Error::InvalidPeriod);
        }
        Self::new(ticks)
    }
    /// Rounded period for a requested rate; the counter has no prescaler.
    /// Query the achieved rate with `clock_hz / period.get()`.
    pub const fn from_hz(clock_hz: u32, frequency_hz: u32) -> Result<Self, Error> {
        if frequency_hz == 0 || clock_hz == 0 {
            return Err(Error::InvalidPeriod);
        }
        let ticks = (clock_hz as u64 + frequency_hz as u64 / 2) / frequency_hz as u64;
        if ticks > MAX_PERIOD as u64 {
            return Err(Error::InvalidPeriod);
        }
        Self::new(ticks as u32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CaptureEdge {
    Both = 1,
    FallingToFalling = 2,
    RisingToRising = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Repeat {
    Once = 0,
    Four = 1,
    Eight = 2,
    Sixteen = 3,
}
impl Repeat {
    pub const fn cycles(self) -> u8 {
        match self {
            Self::Once => 1,
            Self::Four => 4,
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPolarity {
    ActiveHigh,
    ActiveLow,
}

/// A capture word preserves bit 25, whose meaning depends on capture mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureWord(pub u32);
impl CaptureWord {
    pub const fn ticks(self) -> u32 {
        self.0 & 0x01ff_ffff
    }
    /// Valid only for Both edge capture: true means a high-level interval.
    pub const fn high_level(self) -> bool {
        self.0 & 0x0200_0000 != 0
    }
    /// Valid only for same-edge capture: true means a timeout fragment.
    pub const fn timeout_fragment(self) -> bool {
        self.0 & 0x0200_0000 != 0
    }
}

/// Pending event snapshot. Acknowledge selected bits, never modify W1C flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Events(pub(crate) u8);
impl Events {
    pub const CYCLE_END: Self = Self(1);
    pub const DATA: Self = Self(2);
    pub const FIFO_HALF: Self = Self(4);
    pub const DMA_END: Self = Self(8);
    pub const FIFO_ERROR: Self = Self(16);
    pub const ALL: Self = Self(31);
    pub const fn contains(self, event: Self) -> bool {
        self.0 & event.0 == event.0
    }
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn periods() {
        assert_eq!(PeriodTicks::new(0), Err(Error::InvalidPeriod));
        assert_eq!(PeriodTicks::new(1), Err(Error::InvalidPeriod));
        assert_eq!(PeriodTicks::new(MAX_PERIOD + 1), Err(Error::InvalidPeriod));
        for n in [2, 18, 288, MAX_PERIOD] {
            assert_eq!(PeriodTicks::new(n).unwrap().get(), n);
        }
        assert_eq!(PeriodTicks::new_capture(1), Err(Error::InvalidPeriod));
        assert_eq!(
            PeriodTicks::new_capture(MAX_CAPTURE_PERIOD).unwrap().get(),
            MAX_CAPTURE_PERIOD
        );
        assert_eq!(
            PeriodTicks::new_capture(MAX_CAPTURE_PERIOD + 1),
            Err(Error::InvalidPeriod)
        );
    }
    #[test]
    fn capture_metadata() {
        let c = CaptureWord(0x0200_0120);
        assert_eq!(c.ticks(), 288);
        assert!(c.high_level());
        assert!(c.timeout_fragment());
        assert!(!CaptureWord(17).timeout_fragment());
    }
    #[test]
    fn rates_and_repeats() {
        assert_eq!(PeriodTicks::from_hz(78_000_000, 1000).unwrap().get(), 78_000);
        for (c, f) in [(0, 1), (1, 0), (1, 3), (u32::MAX, 1)] {
            assert_eq!(PeriodTicks::from_hz(c, f), Err(Error::InvalidPeriod));
        }
        for (r, n) in [
            (Repeat::Once, 1),
            (Repeat::Four, 4),
            (Repeat::Eight, 8),
            (Repeat::Sixteen, 16),
        ] {
            assert_eq!(r.cycles(), n);
        }
        for bits in 0..32 {
            let events = Events(bits);
            for event in [
                Events::CYCLE_END,
                Events::DATA,
                Events::FIFO_HALF,
                Events::DMA_END,
                Events::FIFO_ERROR,
            ] {
                assert_eq!(events.contains(event), bits & event.bits() != 0);
            }
        }
    }
}
