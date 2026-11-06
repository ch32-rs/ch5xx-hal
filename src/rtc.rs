//! Real-time clock calendar and alarm.
//!
//! The counter is clocked by the 32 kHz oscillator and keeps running through a
//! software reset, so the calendar is not cleared by [`crate::init`]. It is a
//! day count plus a count of 32 kHz cycles within the day. The vendor calendar
//! starts at the year 2020. The day count is 14 bits, so the last value it can
//! hold is 2064-11-08.
//!
//! The day count and the within-day count are loaded through `TRIG`, which is
//! also the compare value of the alarm. [`Rtc::set_datetime`] turns the alarm
//! off, because the compare value does not survive the load.
//!
//! `MODE_CTRL` and `TRIG` are safe-access registers. The module compiles for the
//! CH585 and CH573 layouts, which share these fields. CH572 counts differently
//! and is not covered. Only CH585 has been run on a board.

use core::marker::PhantomData;

use embassy_hal_internal::PeripheralType;

use crate::{pac, peripherals, rcc, Peri};

/// First year of the vendor calendar.
const EPOCH_YEAR: u16 = 2020;
/// Last year that still has a day inside the 14-bit day count.
const LAST_YEAR: u16 = 2064;
/// Largest day count, `CNT_DAY` is 14 bits.
const MAX_DAY: u32 = 0x3FFF;
/// The counter wraps one count past this value.
const MAX_COUNT: u32 = 0xA8C0_0000;
/// Days in a non-leap year.
const DAYS_PER_YEAR: u16 = 365;

/// Why a calendar value was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// The date or time is not a real calendar value, or it does not fit in the
    /// counter. The year is 2020..=2064, and 2064 stops at 8 November.
    DateTime,
}

/// A calendar value. The year is 2020..=2064, and 2064 stops at 8 November.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DateTime {
    /// Year, 2020..=2064.
    pub year: u16,
    /// Month, 1..=12.
    pub month: u8,
    /// Day of the month, 1..=31.
    pub day: u8,
    /// Hour, 0..=23.
    pub hour: u8,
    /// Minute, 0..=59.
    pub minute: u8,
    /// Second, 0..=59.
    pub second: u8,
}

/// Period of the RTC interval timer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Period {
    /// 0.125 s.
    Ms125 = 0,
    /// 0.25 s.
    Ms250 = 1,
    /// 0.5 s.
    Ms500 = 2,
    /// 1 s.
    S1 = 3,
    /// 2 s.
    S2 = 4,
    /// 4 s.
    S4 = 5,
    /// 8 s.
    S8 = 6,
    /// 16 s.
    S16 = 7,
}

/// The real-time clock.
pub struct Rtc<'d, T: Instance> {
    _phantom: PhantomData<&'d mut T>,
}

impl<'d, T: Instance> Rtc<'d, T> {
    /// Take ownership of the counter.
    ///
    /// The counter is not reset: it holds whatever it held across the last
    /// reset. Call [`Self::set_datetime`] to give it a known calendar value.
    pub fn new(_peri: Peri<'d, T>) -> Self {
        Self { _phantom: PhantomData }
    }

    /// Load the counter with `datetime`.
    ///
    /// The load uses the trigger register, so an alarm that was waiting is
    /// turned off. Call [`Self::set_alarm_in`] again afterwards.
    pub fn set_datetime(&mut self, datetime: DateTime) -> Result<(), Error> {
        let (day, low) = encode(datetime)?;
        // The vendor waits until the 32 kHz clock pin reads high, so the load
        // does not straddle an oscillator edge. The internal RC toggles that
        // pin; spin until one high sample is stable.
        while !clock_pin_high() {}

        // Drop the alarm before `TRIG` becomes the load port. Leaving it enabled
        // would compare against the day count written below.
        rcc::with_safe_access(|| {
            T::regs().trig().write(|w| w.set_trig(day as u32));
            T::regs().mode_ctrl().modify(|w| {
                w.set_trig_en(false);
                w.set_load_hi(true);
            });
        });
        while T::regs().cnt_day().read().cnt_day() != day {}
        rcc::with_safe_access(|| {
            T::regs().trig().write(|w| w.set_trig(low));
            T::regs().mode_ctrl().modify(|w| w.set_load_lo(true));
        });
        Ok(())
    }

    /// Read the counter as a calendar value.
    pub fn datetime(&self) -> DateTime {
        // The three words are not one register, so read them twice and keep a
        // pair that did not move between the reads.
        let (day, sec2, cycles) = loop {
            let day = T::regs().cnt_day().read().cnt_day();
            let sec2 = T::regs().cnt_2s().read().cnt_2s();
            let cycles = T::regs().cnt_32k().read().cnt_32k();
            if day == T::regs().cnt_day().read().cnt_day()
                && sec2 == T::regs().cnt_2s().read().cnt_2s()
                && cycles == T::regs().cnt_32k().read().cnt_32k()
            {
                break (day, sec2, cycles);
            }
        };
        decode(day, sec2, cycles)
    }

    /// The raw 32 kHz cycle count, read twice so a tearing read is discarded.
    pub fn cycle_count(&self) -> u32 {
        loop {
            let first = raw_count();
            let second = raw_count();
            if first == second {
                return first;
            }
        }
    }

    /// Raise the trigger flag `cycles` 32 kHz cycles from now.
    ///
    /// The compare value wraps at [`MAX_COUNT`]. The flag stays set until
    /// [`Self::clear_trigger`].
    pub fn set_alarm_in(&mut self, cycles: u32) {
        let mut at = self.cycle_count().wrapping_add(cycles);
        if at > MAX_COUNT {
            at -= MAX_COUNT;
        }
        rcc::with_safe_access(|| {
            T::regs().trig().write(|w| w.set_trig(at));
            T::regs().mode_ctrl().modify(|w| w.set_trig_en(true));
        });
    }

    /// Whether the trigger compare has matched since it was last cleared.
    pub fn trigger_pending(&self) -> bool {
        T::regs().flag_ctrl().read().trig_flag()
    }

    /// Clear the trigger flag. The bit clears itself.
    pub fn clear_trigger(&mut self) {
        T::regs().flag_ctrl().write(|w| w.set_trig_clr(true));
    }

    /// Turn the trigger compare off.
    pub fn disable_alarm(&mut self) {
        rcc::with_safe_access(|| {
            T::regs().mode_ctrl().modify(|w| w.set_trig_en(false));
        });
    }

    /// Start the interval timer at `period`.
    ///
    /// The mode is cleared and then written, matching the vendor sequence,
    /// because the enable and the period share one safe-access byte.
    pub fn start_timer(&mut self, period: Period) {
        rcc::with_safe_access(|| {
            T::regs().mode_ctrl().modify(|w| {
                w.set_tmr_en(false);
                w.set_tmr_mode(0);
            });
        });
        rcc::with_safe_access(|| {
            T::regs().mode_ctrl().modify(|w| {
                w.set_tmr_mode(period as u8);
                w.set_tmr_en(true);
            });
        });
    }

    /// Stop the interval timer.
    pub fn stop_timer(&mut self) {
        rcc::with_safe_access(|| {
            T::regs().mode_ctrl().modify(|w| w.set_tmr_en(false));
        });
    }

    /// Whether an interval has elapsed since the flag was last cleared.
    pub fn timer_pending(&self) -> bool {
        T::regs().flag_ctrl().read().tmr_flag()
    }

    /// Clear the interval flag. The bit clears itself.
    pub fn clear_timer(&mut self) {
        T::regs().flag_ctrl().write(|w| w.set_tmr_clr(true));
    }
}

/// `CNT_DAY` and `CNT_2S` above the 32 kHz count, which is how the counter
/// is laid out: the day at bits 45:32, the two-second count at bits 31:16 and
/// the 32 kHz count at bits 15:0. The 32-bit value keeps the low 32 bits.
fn raw_count() -> u32 {
    let regs = pac::RTC;
    let sec2 = regs.cnt_2s().read().cnt_2s() as u32;
    let cycles = regs.cnt_32k().read().cnt_32k() as u32;
    (sec2 << 16) | cycles
}

/// Two consecutive reads of the 32 kHz clock pin agree, and it is high.
fn clock_pin_high() -> bool {
    let pin = pac::SYS.ck32k_config().read()._32k_clk_pin();
    pin && pin == pac::SYS.ck32k_config().read()._32k_clk_pin()
}

/// Turn a calendar value into the day count and the within-day word.
fn encode(datetime: DateTime) -> Result<(u16, u32), Error> {
    if !(EPOCH_YEAR..=LAST_YEAR).contains(&datetime.year)
        || !(1..=12).contains(&datetime.month)
        || datetime.day == 0
        || datetime.day > days_in_month(datetime.year, datetime.month)
        || datetime.hour > 23
        || datetime.minute > 59
        || datetime.second > 59
    {
        return Err(Error::DateTime);
    }

    let mut days: u32 = 0;
    let mut year = datetime.year;
    while year > EPOCH_YEAR {
        year -= 1;
        days += days_in_year(year) as u32;
    }
    let mut month = datetime.month;
    while month > 1 {
        month -= 1;
        days += days_in_month(datetime.year, month) as u32;
    }
    days += datetime.day as u32 - 1;
    if days > MAX_DAY {
        return Err(Error::DateTime);
    }

    // `CNT_2S` counts pairs of seconds. The odd second sets the top bit of the
    // 32 kHz count, which is where the vendor puts it.
    let sec2 = datetime.hour as u32 * 1800 + datetime.minute as u32 * 30 + datetime.second as u32 / 2;
    let odd = if datetime.second % 2 == 1 { 0x8000 } else { 0 };
    Ok((days as u16, (sec2 << 16) | odd))
}

/// Turn the three counter words into a calendar value.
fn decode(mut day: u16, sec2: u16, cycles: u16) -> DateTime {
    let mut year = EPOCH_YEAR;
    loop {
        let length = days_in_year(year);
        if day < length {
            break;
        }
        day -= length;
        year += 1;
    }

    let mut month = 1u8;
    while month < 12 {
        let length = days_in_month(year, month) as u16;
        if day < length {
            break;
        }
        day -= length;
        month += 1;
    }

    let seconds = sec2 as u32 * 2 + u32::from(cycles >= 0x8000);
    DateTime {
        year,
        month,
        day: day as u8 + 1,
        hour: (seconds / 3600) as u8,
        minute: ((seconds % 3600) / 60) as u8,
        second: (seconds % 60) as u8,
    }
}

fn days_in_year(year: u16) -> u16 {
    if is_leap(year) {
        DAYS_PER_YEAR + 1
    } else {
        DAYS_PER_YEAR
    }
}

fn is_leap(year: u16) -> bool {
    year % 400 == 0 || (year % 100 != 0 && year % 4 == 0)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// --- instance ------------------------------------------------------------------

mod sealed {
    use super::pac;

    pub trait Instance {
        fn regs() -> pac::rtc::Rtc;
    }
}

/// A real-time clock peripheral.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {}

impl sealed::Instance for peripherals::RTC {
    fn regs() -> pac::rtc::Rtc {
        pac::RTC
    }
}

impl Instance for peripherals::RTC {}
