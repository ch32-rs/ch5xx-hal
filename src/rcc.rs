//! Clock configuration.
//!
//! The CH5xx has no RCC block. `SYS.CLK_CFG` (`0x4000_1008`) selects the system
//! clock source and divider, `SYS.HFCK_PWR_CTRL` (`0x4000_100A`) powers the
//! oscillators, and `SYSTEM_CONTROL.XT32M_TUNE` (`0x4000_104E`) trims the 32 MHz
//! crystal. All of them are RWA: writes need the safe-access (SAM) handshake
//! described below.
//!
//! # Tree
//!
//! Unlike the CH32 families there are no AHB/APB prescalers: `HCLK` *is* `Fsys`
//! and drives the core, SysTick and every peripheral.
//!
//! ```text
//!          HSI 16 MHz / HSE 32 MHz
//!                   |
//!      +------------+--------------+
//!      |            |              |
//!   undivided    / 2..=32        PLL: 624 / 2 = 312 MHz
//!   (16/32 MHz)  (1..=16 MHz)       / 4..=32
//!      |            |              |
//!      +------------+--------------+
//!                   Fsys
//! ```
//!
//! The datasheet's ranges (6.1, clock tree diagram): `1..=32 MHz` for the
//! divided/undivided modes and `9.75..=78 MHz` for the PLL mode. The PLL is
//! always 624 MHz (`16 MHz * 39`, or `32 MHz / 2 * 39`), so its source only
//! affects accuracy, not the result.
//!
//! `CLK_SYS_MOD` picks the branch, `CLK_PLL_DIV` divides it, `OSC32M_SEL`
//! selects HSE over HSI, and `XROM_SCLK_SEL` feeds the Flash controller. The
//! 32 kHz mode is deliberately not offered: it requires the whole program in
//! RAM.
//!
//! # Defaults
//!
//! [`Config::default`] is the datasheet's reset value, `CLK_CFG = 0x0003`
//! (HSI 16 MHz / 3 = 5.333 MHz), so adding this module does not change the
//! behaviour of existing programs. The boot ROM powers the PLL but does not
//! route the system clock through it.
//!
//! Every preset mirrors a `CLK_SOURCE_*` value in WCH's C SDK
//! (`CH58x_clk.h`), which is the authoritative encoding table. `HSE_*` and
//! `*_HSE_PLL_*` need the board's 32 MHz crystal; the reference board uses
//! 18 pF load capacitors, which is what the presets select.
//!
//! # Safe access
//!
//! The clock and oscillator registers are RWA. Entering SAM means writing
//! `0x57` then `0xA8` to `SAFE_ACCESS_SIG`; the window lasts ~112 `Tsys` and
//! closes itself (0x00 closes it early). [`set_config`] masks interrupts around
//! every window so a handler cannot consume it, and reads `CLK_CFG` back to
//! prove the write landed.
//!
//! # Frequencies
//!
//! [`clocks`] reports the **nominal** frequency of the configured tree.
//! [`crate::delay`] separately measures the real SysTick rate against the 32 kHz
//! RTC and keeps that number for its busy-waits: for HSE/PLL the nominal value
//! is more accurate than a measurement referenced to an RC oscillator, and for
//! HSI the two agree to within its tolerance.
//!
//! Do not read `CLK_CFG` with `probe-rs read`: that halts the core and the value
//! it reports is not what the running firmware sees. Read it from firmware,
//! e.g. `examples/ch585/src/bin/clk_probe.rs`.

use crate::atomic::{AtomicU32, Ordering};
use crate::pac;
use crate::time::Hertz;

/// Starting value for the cache, until [`set_config`] runs.
///
/// This is the CH585 reset default: HSI 16 MHz divided by 3.
pub const DEFAULT_FREQUENCY: Hertz = Hertz(5_333_333);

static SYSCLK_HZ: AtomicU32 = AtomicU32::new(DEFAULT_FREQUENCY.0);

const SAFE_ACCESS_SIG1: u8 = 0x57;
const SAFE_ACCESS_SIG2: u8 = 0xA8;
const SAFE_ACCESS_SIG0: u8 = 0x00;

/// Oscillator at the root of the clock tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Osc {
    /// Internal 16 MHz RC oscillator (HSI). Always available.
    Hsi16,
    /// External 32 MHz crystal (HSE) on X32MI/X32MO. Requires the crystal.
    Hse32,
}

/// How `Fsys` is derived from the oscillator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sys {
    /// Undivided oscillator: 16 MHz on HSI, 32 MHz on HSE (`CLK_SYS_MOD = 10`).
    Direct,
    /// Oscillator divided by `div`, `div` in `2..=32` (`CLK_SYS_MOD = 00`).
    ///
    /// The divider must keep `Fsys` at or above the 1 MHz minimum; for HSI
    /// that limits `div` to `2..=16`.
    Divided(u8),
    /// 312 MHz divided by `div`, `div` in `4..=32` (`CLK_SYS_MOD = 01`).
    ///
    /// That is 9.75 MHz at 32 and 78 MHz at 4. Sources above 78 MHz are outside
    /// the datasheet's range.
    Pll(u8),
}

/// HSE crystal load capacitance, as `XT32M_TUNE.C_LOAD`.
///
/// The datasheet (6.3) maps the four-bit field to `2 * value + 10 pF` for
/// values 0..=7 and `2 * value + 2 pF` for 8..=15. The CH585M-R1-1v1 board
/// fits 18 pF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HseCap {
    /// 2 pF.
    P2 = 8,
    /// 4 pF.
    P4 = 9,
    /// 6 pF.
    P6 = 10,
    /// 8 pF.
    P8 = 11,
    /// 10 pF.
    P10 = 0,
    /// 12 pF.
    P12 = 1,
    /// 14 pF.
    P14 = 2,
    /// 16 pF.
    P16 = 3,
    /// 18 pF.
    P18 = 4,
    /// 20 pF.
    P20 = 5,
    /// 22 pF.
    P22 = 6,
    /// 24 pF.
    P24 = 7,
}

/// Clock configuration.
///
/// Build one from a preset ([`Config::HSI_PLL_24MHZ`] and friends), from
/// [`Config::hsi_nearest`] / [`Config::hse_nearest`], or from
/// [`Config::new`]. The fields stay public so a preset can be adjusted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Oscillator the tree starts from.
    pub osc: Osc,
    /// Branch taken from `osc`.
    pub sys: Sys,
    /// Crystal load capacitance; only used when `osc` is [`Osc::Hse32`].
    pub hse_cap: HseCap,
}

impl Default for Config {
    /// The datasheet reset value: HSI 16 MHz divided by 3 (5.333 MHz).
    fn default() -> Self {
        Self {
            osc: Osc::Hsi16,
            sys: Sys::Divided(3),
            hse_cap: HseCap::P18,
        }
    }
}

/// Expands the preset table, so each entry stays one line and keeps its docs.
macro_rules! presets {
    ($($(#[$meta:meta])* $name:ident = $osc:ident, $sys:expr;)*) => {
        $(
            $(#[$meta])*
            pub const $name: Self = Self {
                osc: Osc::$osc,
                sys: $sys,
                hse_cap: HseCap::P18,
            };
        )*
    };
}

impl Config {
    /// A configuration for `osc` and `sys`, with the reference board's 18 pF
    /// HSE load capacitance.
    pub const fn new(osc: Osc, sys: Sys) -> Self {
        Self {
            osc,
            sys,
            hse_cap: HseCap::P18,
        }
    }

    /// Replace the HSE load capacitance.
    pub const fn with_hse_cap(self, hse_cap: HseCap) -> Self {
        Self { hse_cap, ..self }
    }

    /// Nominal `Fsys` produced by this configuration.
    ///
    /// This is compile-time arithmetic over the datasheet's tree; it does not
    /// read any register. The value is only meaningful for a configuration
    /// whose divisors are in range (see [`set_config`]).
    pub const fn sysclk(&self) -> Hertz {
        let base = match self.osc {
            Osc::Hsi16 => 16_000_000,
            Osc::Hse32 => 32_000_000,
        };
        match self.sys {
            Sys::Direct => Hertz(base),
            Sys::Divided(div) => Hertz(base / div as u32),
            Sys::Pll(div) => Hertz(312_000_000 / div as u32),
        }
    }

    /// Closest achievable clock that uses HSI only, so no crystal is needed.
    ///
    /// Ties go to the lower frequency. Use [`Config::sysclk`] to see what was
    /// chosen; a request outside 1..=78 MHz is still clamped to the tree.
    pub const fn hsi_nearest(hz: Hertz) -> Self {
        let mut best = 16_000_000;
        let mut sys = Sys::Direct;

        let mut div = 2u8;
        while div <= 16 {
            let freq = 16_000_000 / div as u32;
            if closer(hz.0, freq, best) {
                best = freq;
                sys = Sys::Divided(div);
            }
            div += 1;
        }

        let mut div = 4u8;
        while div <= 32 {
            let freq = 312_000_000 / div as u32;
            if closer(hz.0, freq, best) {
                best = freq;
                sys = Sys::Pll(div);
            }
            div += 1;
        }

        Self {
            osc: Osc::Hsi16,
            sys,
            hse_cap: HseCap::P18,
        }
    }

    /// Closest achievable clock driven by the 32 MHz crystal.
    ///
    /// Same selection rules as [`Config::hsi_nearest`], but divided and
    /// undivided branches use HSE, which reaches 32 MHz.
    pub const fn hse_nearest(hz: Hertz) -> Self {
        let mut best = 32_000_000;
        let mut sys = Sys::Direct;

        let mut div = 2u8;
        while div <= 32 {
            let freq = 32_000_000 / div as u32;
            if closer(hz.0, freq, best) {
                best = freq;
                sys = Sys::Divided(div);
            }
            div += 1;
        }

        let mut div = 4u8;
        while div <= 32 {
            let freq = 312_000_000 / div as u32;
            if closer(hz.0, freq, best) {
                best = freq;
                sys = Sys::Pll(div);
            }
            div += 1;
        }

        Self {
            osc: Osc::Hse32,
            sys,
            hse_cap: HseCap::P18,
        }
    }

    presets! {
        /// 16 MHz, HSI undivided (`CLK_SOURCE_HSI_16MHz`).
        HSI_16MHZ = Hsi16, Sys::Direct;

        /// 8 MHz, HSI / 2 (`CLK_SOURCE_HSI_8MHz`).
        HSI_8MHZ = Hsi16, Sys::Divided(2);

        /// 5.333 MHz, HSI / 3 (`CLK_SOURCE_HSI_5_3MHz`). The reset default.
        HSI_5_33MHZ = Hsi16, Sys::Divided(3);

        /// 4 MHz, HSI / 4 (`CLK_SOURCE_HSI_4MHz`).
        HSI_4MHZ = Hsi16, Sys::Divided(4);

        /// 2 MHz, HSI / 8 (`CLK_SOURCE_HSI_2MHz`).
        HSI_2MHZ = Hsi16, Sys::Divided(8);

        /// 1 MHz, HSI / 16 (`CLK_SOURCE_HSI_1MHz`).
        HSI_1MHZ = Hsi16, Sys::Divided(16);

        /// 32 MHz, HSE undivided (`CLK_SOURCE_HSE_32MHz`). Needs the crystal.
        HSE_32MHZ = Hse32, Sys::Direct;

        /// 16 MHz, HSE / 2 (`CLK_SOURCE_HSE_16MHz`). Needs the crystal.
        HSE_16MHZ = Hse32, Sys::Divided(2);

        /// 8 MHz, HSE / 4 (`CLK_SOURCE_HSE_8MHz`). Needs the crystal.
        HSE_8MHZ = Hse32, Sys::Divided(4);

        /// 6.4 MHz, HSE / 5 (`CLK_SOURCE_HSE_6_4MHz`). Needs the crystal.
        HSE_6_4MHZ = Hse32, Sys::Divided(5);

        /// 4 MHz, HSE / 8 (`CLK_SOURCE_HSE_4MHz`). Needs the crystal.
        HSE_4MHZ = Hse32, Sys::Divided(8);

        /// 2 MHz, HSE / 16 (`CLK_SOURCE_HSE_2MHz`). Needs the crystal.
        HSE_2MHZ = Hse32, Sys::Divided(16);

        /// 78 MHz, PLL / 4 (`CLK_SOURCE_HSI_PLL_78MHz`).
        HSI_PLL_78MHZ = Hsi16, Sys::Pll(4);

        /// 62.4 MHz, PLL / 5 (`CLK_SOURCE_HSI_PLL_62_4MHz`).
        HSI_PLL_62_4MHZ = Hsi16, Sys::Pll(5);

        /// 52 MHz, PLL / 6 (`CLK_SOURCE_HSI_PLL_52MHz`).
        HSI_PLL_52MHZ = Hsi16, Sys::Pll(6);

        /// 39 MHz, PLL / 8 (`CLK_SOURCE_HSI_PLL_39MHz`).
        HSI_PLL_39MHZ = Hsi16, Sys::Pll(8);

        /// 26 MHz, PLL / 12 (`CLK_SOURCE_HSI_PLL_26MHz`).
        HSI_PLL_26MHZ = Hsi16, Sys::Pll(12);

        /// 24 MHz, PLL / 13 (`CLK_SOURCE_HSI_PLL_24MHz`).
        ///
        /// 115200 baud becomes `DL = 26` = 115385 (+0.16%), inside the
        /// datasheet's 0.5% transmit budget, so this is the cheapest way to get
        /// a usable 115200 on a board without a crystal.
        HSI_PLL_24MHZ = Hsi16, Sys::Pll(13);

        /// 19.5 MHz, PLL / 16 (`CLK_SOURCE_HSI_PLL_19_5MHz`).
        HSI_PLL_19_5MHZ = Hsi16, Sys::Pll(16);

        /// 13 MHz, PLL / 24 (`CLK_SOURCE_HSI_PLL_13MHz`).
        HSI_PLL_13MHZ = Hsi16, Sys::Pll(24);

        /// 78 MHz, PLL / 4 from the crystal (`CLK_SOURCE_HSE_PLL_78MHz`).
        HSE_PLL_78MHZ = Hse32, Sys::Pll(4);

        /// 62.4 MHz, PLL / 5 from the crystal (`CLK_SOURCE_HSE_PLL_62_4MHz`).
        HSE_PLL_62_4MHZ = Hse32, Sys::Pll(5);

        /// 52 MHz, PLL / 6 from the crystal (`CLK_SOURCE_HSE_PLL_52MHz`).
        HSE_PLL_52MHZ = Hse32, Sys::Pll(6);

        /// 39 MHz, PLL / 8 from the crystal (`CLK_SOURCE_HSE_PLL_39MHz`).
        HSE_PLL_39MHZ = Hse32, Sys::Pll(8);

        /// 26 MHz, PLL / 12 from the crystal (`CLK_SOURCE_HSE_PLL_26MHz`).
        HSE_PLL_26MHZ = Hse32, Sys::Pll(12);

        /// 24 MHz, PLL / 13 from the crystal (`CLK_SOURCE_HSE_PLL_24MHz`).
        HSE_PLL_24MHZ = Hse32, Sys::Pll(13);

        /// 19.5 MHz, PLL / 16 from the crystal (`CLK_SOURCE_HSE_PLL_19_5MHz`).
        HSE_PLL_19_5MHZ = Hse32, Sys::Pll(16);

        /// 13 MHz, PLL / 24 from the crystal (`CLK_SOURCE_HSE_PLL_13MHz`).
        HSE_PLL_13MHZ = Hse32, Sys::Pll(24);
    }
}

/// Absolute difference, usable in `const fn`.
const fn abs_diff(a: u32, b: u32) -> u32 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// Whether `cand` is a better match for `requested` than `best`.
///
/// Ties prefer the lower frequency: an undershoot leaves more of the UART
/// receiver's tolerance margin than an overshoot in the same direction.
const fn closer(requested: u32, cand: u32, best: u32) -> bool {
    let cand_err = abs_diff(cand, requested);
    let best_err = abs_diff(best, requested);
    cand_err < best_err || (cand_err == best_err && cand < best)
}

/// Clock frequencies, as seen by drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clocks {
    /// System clock, which drives the core, SysTick and every peripheral.
    pub sysclk: Hertz,
}

/// The cached clock frequencies.
#[inline]
pub fn clocks() -> Clocks {
    Clocks {
        sysclk: Hertz(SYSCLK_HZ.load(Ordering::Relaxed)),
    }
}

/// Apply `config` to the hardware and update [`clocks`].
///
/// `ch5xx_hal::init` calls this before the delay calibration and the time
/// driver, which is the supported order. Only the clock tree and the
/// [`clocks`] cache are touched: nothing derived from the old frequency is
/// rebuilt. After a later call, re-run whatever depends on `Fsys`:
/// `crate::delay::recalibrate` (where the SysTick time driver is not selected)
/// for busy-waits, the embassy time driver (only during a fresh startup), UART
/// dividers, and any other peripheral configured from [`clocks`].
///
/// The HSE is expected to be running already: reset powers it and this
/// function does not wait for a cold crystal start. It only asserts the power
/// bit and selects the source.
///
/// # Panics
///
/// Panics if the configuration cannot be represented by the hardware:
/// `Divided` outside `2..=32`, `Pll` outside `4..=32`, or a divided `Fsys`
/// below the 1 MHz minimum. Also panics if the register write does not read
/// back as written.
///
/// # Safety
///
/// `Fsys` changes underneath everything running, leaving the derived state
/// described above stale. The oscillator source change itself is safe because
/// this function keeps the PLL running (`PLL_GATE_DISS`): XROM is clocked from
/// the PLL in direct and PLL modes. Do not reproduce the sequence elsewhere
/// without that bit.
pub unsafe fn set_config(config: Config) -> Clocks {
    validate(&config);

    let sysclk = config.sysclk();
    let before = pac::SYS.clk_cfg().read();

    let (sys_mod, div) = match config.sys {
        Sys::Direct => (0b10, 0),
        Sys::Divided(div) => (0b00, div),
        Sys::Pll(div) => (0b01, div),
    };
    // WCH's CLK_SOURCE_* table sets XROM to the PLL for both direct and PLL
    // modes, and to the 2x oscillator clock for divided modes. Mirror it.
    let xrom_sclk = matches!(config.sys, Sys::Direct | Sys::Pll(_));
    let osc32m_sel = config.osc == Osc::Hse32;

    // Changing the PLL source stops the PLL for 30..40 us unless told not to.
    // Both direct and PLL modes clock the Flash controller (XROM) from the
    // PLL, so every source change must keep it running, not only changes made
    // while `Fsys` itself comes from the PLL.
    let pll_gate_diss = before.osc32m_sel() != osc32m_sel;

    // Power the oscillators this configuration needs. Reset leaves HSI, HSE
    // and the PLL powered, so in the normal path this only re-asserts bits
    // that are already set.
    safe_access(|| {
        pac::SYS.hfck_pwr_ctrl().modify(|w| {
            match config.osc {
                Osc::Hsi16 => w.set_clk_rc16m_pon(true),
                Osc::Hse32 => w.set_clk_xt32m_pon(true),
            }
            if matches!(config.sys, Sys::Pll(_)) {
                w.set_clk_pll_pon(true);
            }
        });
    });

    if osc32m_sel {
        safe_access(|| {
            pac::SYSTEM_CONTROL
                .xt32m_tune()
                .modify(|w| w.set_xt32m_c_load(config.hse_cap as u8));
        });
    }

    safe_access(|| {
        pac::SYS.clk_cfg().write(|w| {
            w.set_clk_pll_div(div);
            w.set_clk_sys_mod(sys_mod);
            w.set_xrom_sclk_sel(xrom_sclk);
            w.set_osc32m_sel(osc32m_sel);
            w.set_pll_gate_diss(pll_gate_diss);
            w.set_pll_gate_time(false);
        });
    });

    // The oscillator multiplexer needs 1 us to settle after OSC32M_SEL changes
    // (datasheet 6.3). No timer is available at this point, so spin a fixed
    // count that is conservative even at the 1 MHz minimum.
    for _ in 0..64 {
        core::hint::spin_loop();
    }

    // A SAM window that expired early drops the write silently, so prove it.
    let after = pac::SYS.clk_cfg().read();
    assert_eq!(after.clk_sys_mod(), sys_mod, "rcc: CLK_SYS_MOD did not stick");
    assert_eq!(after.clk_pll_div(), div, "rcc: CLK_PLL_DIV did not stick");
    assert_eq!(after.osc32m_sel(), osc32m_sel, "rcc: OSC32M_SEL did not stick");
    assert_eq!(after.xrom_sclk_sel(), xrom_sclk, "rcc: XROM_SCLK_SEL did not stick");

    SYSCLK_HZ.store(sysclk.0, Ordering::Relaxed);
    Clocks { sysclk }
}

/// Reject configurations the clock tree cannot produce.
fn validate(config: &Config) {
    match config.sys {
        Sys::Direct => {}
        Sys::Divided(div) => {
            assert!((2..=32).contains(&div), "rcc: CLK_PLL_DIV must be 2..=32");
            let base = match config.osc {
                Osc::Hsi16 => 16_000_000,
                Osc::Hse32 => 32_000_000,
            };
            assert!(base / div as u32 >= 1_000_000, "rcc: Fsys below the 1 MHz minimum");
        }
        Sys::Pll(div) => assert!((4..=32).contains(&div), "rcc: PLL divider must be 4..=32"),
    }
}

/// Run `f` inside a safe-access window.
///
/// The window is ~112 `Tsys` long and must not lose those cycles to an
/// interrupt, or the write it protects is dropped; hence the critical section.
/// Closing the window early (0x00) keeps it from leaking into later, unrelated
/// register writes.
fn safe_access<R>(f: impl FnOnce() -> R) -> R {
    critical_section::with(|_| {
        let sig = pac::SYSTEM_CONTROL.safe_access_sig();
        sig.write(|w| w.set_safe_access_sig(SAFE_ACCESS_SIG1));
        sig.write(|w| w.set_safe_access_sig(SAFE_ACCESS_SIG2));
        let result = f();
        sig.write(|w| w.set_safe_access_sig(SAFE_ACCESS_SIG0));
        result
    })
}
