#![allow(clippy::missing_safety_doc)]
#![allow(clippy::identity_op)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::erasing_op)]

#[doc = "System Control Register"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Sys {
    ptr: *mut u8,
}
unsafe impl Send for Sys {}
unsafe impl Sync for Sys {}
impl Sys {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "WO, Watchdog key register, SAM"]
    #[inline(always)]
    pub const fn iwdg_kr(self) -> crate::common::Reg<regs::IwdgKr, crate::common::W> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0usize) as _) }
    }
    #[doc = "RW, Watchdog configuration register, SAM"]
    #[inline(always)]
    pub const fn iwdg_cfg(self) -> crate::common::Reg<regs::IwdgCfg, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x04usize) as _) }
    }
    #[doc = "RWA, system clock configuration, SAM"]
    #[inline(always)]
    pub const fn clk_sys_cfg(self) -> crate::common::Reg<regs::ClkSysCfg, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x08usize) as _) }
    }
    #[doc = "RWA, High frequency clock module power control register, SAM"]
    #[inline(always)]
    pub const fn hfck_pwr_ctrl(self) -> crate::common::Reg<regs::HfckPwrCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0ausize) as _) }
    }
    #[doc = "RWA, sleep clock off control byte 0, SAM"]
    #[inline(always)]
    pub const fn slp_clk_off0(self) -> crate::common::Reg<regs::SlpClkOff0, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0cusize) as _) }
    }
    #[doc = "RWA, sleep clock off control byte 1, SAM"]
    #[inline(always)]
    pub const fn slp_clk_off1(self) -> crate::common::Reg<regs::SlpClkOff1, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0dusize) as _) }
    }
    #[doc = "RWA, wake control, SAM"]
    #[inline(always)]
    pub const fn slp_wake_ctrl(self) -> crate::common::Reg<regs::SlpWakeCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0eusize) as _) }
    }
    #[doc = "RWA, peripherals power down control, SAM"]
    #[inline(always)]
    pub const fn slp_power_ctrl(self) -> crate::common::Reg<regs::SlpPowerCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0fusize) as _) }
    }
    #[doc = "RWA, Secure Access Mode Control Register, SAM"]
    #[inline(always)]
    pub const fn safe_mode_ctrl(self) -> crate::common::Reg<regs::SafeModeCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x10usize) as _) }
    }
    #[doc = "RWA, Secure access to clock control registers, SAM"]
    #[inline(always)]
    pub const fn safe_clk_ctrl(self) -> crate::common::Reg<regs::SafeClkCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x11usize) as _) }
    }
    #[doc = "RWA, Secure Access Debugging Control Register, SAM"]
    #[inline(always)]
    pub const fn safe_debug_ctrl(
        self,
    ) -> crate::common::Reg<regs::SafeDebugCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x12usize) as _) }
    }
    #[doc = "RWA, Long term reset control register for secure access, SAM"]
    #[inline(always)]
    pub const fn safe_lrst_ctrl(self) -> crate::common::Reg<regs::SafeLrstCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x13usize) as _) }
    }
    #[doc = "RW, function pin alternate configuration"]
    #[inline(always)]
    pub const fn pin_alternate(self) -> crate::common::Reg<regs::PinAlternate, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x18usize) as _) }
    }
    #[doc = "RW, Function pin configuration registere"]
    #[inline(always)]
    pub const fn pin_config(self) -> crate::common::Reg<regs::PinConfig, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x1ausize) as _) }
    }
    #[doc = "RW, Function pin digital input disable register"]
    #[inline(always)]
    pub const fn pin_in_dis(self) -> crate::common::Reg<regs::PinInDis, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x1cusize) as _) }
    }
    #[doc = "RWA, power plan before sleep instruction, SAM"]
    #[inline(always)]
    pub const fn power_plan(self) -> crate::common::Reg<regs::PowerPlan, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x20usize) as _) }
    }
    #[doc = "RWA, aux power adjust control, SAM"]
    #[inline(always)]
    pub const fn aux_power_adj(self) -> crate::common::Reg<regs::AuxPowerAdj, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x22usize) as _) }
    }
    #[doc = "RWA, battery voltage detector control, SAM"]
    #[inline(always)]
    pub const fn bat_det_ctrl(self) -> crate::common::Reg<regs::BatDetCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x24usize) as _) }
    }
    #[doc = "RWA, battery voltage detector configuration, SAM"]
    #[inline(always)]
    pub const fn bat_det_cfg(self) -> crate::common::Reg<regs::BatDetCfg, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x25usize) as _) }
    }
    #[doc = "RO, battery status"]
    #[inline(always)]
    pub const fn bat_status(self) -> crate::common::Reg<regs::BatStatus, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x26usize) as _) }
    }
    #[doc = "RWA, internal 32KHz oscillator tune control, SAM"]
    #[inline(always)]
    pub const fn int32k_tune(self) -> crate::common::Reg<regs::Int32kTune, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x2cusize) as _) }
    }
    #[doc = "RWA, External 32KHz oscillator tune control, SAM"]
    #[inline(always)]
    pub const fn xt32k_tune(self) -> crate::common::Reg<regs::Xt32kTune, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x2eusize) as _) }
    }
    #[doc = "RWA, 32KHz oscillator configure"]
    #[inline(always)]
    pub const fn ck32k_config(self) -> crate::common::Reg<regs::Ck32kConfig, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x2fusize) as _) }
    }
}
pub mod regs {
    #[doc = "RWA, aux power adjust control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct AuxPowerAdj(pub u16);
    impl AuxPowerAdj {
        #[doc = "RWA, Output voltage regulation value of auxiliary power supply for ultra-low power consumption LDO"]
        #[inline(always)]
        pub const fn ulpldo_adj(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x07;
            val as u8
        }
        #[doc = "RWA, Output voltage regulation value of auxiliary power supply for ultra-low power consumption LDO"]
        #[inline(always)]
        pub fn set_ulpldo_adj(&mut self, val: u8) {
            self.0 = (self.0 & !(0x07 << 0usize)) | (((val as u16) & 0x07) << 0usize);
        }
        #[doc = "RWA, Low power auxiliary DC-DC enable bit"]
        #[inline(always)]
        pub const fn dcdc_charge(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Low power auxiliary DC-DC enable bit"]
        #[inline(always)]
        pub fn set_dcdc_charge(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u16) & 0x01) << 7usize);
        }
        #[doc = "RWA, retain. The original value must be kept unchanged when writing"]
        #[inline(always)]
        pub const fn cfg_ivref(&self) -> u8 {
            let val = (self.0 >> 8usize) & 0x0f;
            val as u8
        }
        #[doc = "RWA, retain. The original value must be kept unchanged when writing"]
        #[inline(always)]
        pub fn set_cfg_ivref(&mut self, val: u8) {
            self.0 = (self.0 & !(0x0f << 8usize)) | (((val as u16) & 0x0f) << 8usize);
        }
        #[doc = "RWA, TouchKey bias current, used to stabilize I/O potential and prevent accidental wake-up"]
        #[inline(always)]
        pub const fn ib_tkey_sel(&self) -> u8 {
            let val = (self.0 >> 12usize) & 0x0f;
            val as u8
        }
        #[doc = "RWA, TouchKey bias current, used to stabilize I/O potential and prevent accidental wake-up"]
        #[inline(always)]
        pub fn set_ib_tkey_sel(&mut self, val: u8) {
            self.0 = (self.0 & !(0x0f << 12usize)) | (((val as u16) & 0x0f) << 12usize);
        }
    }
    impl Default for AuxPowerAdj {
        #[inline(always)]
        fn default() -> AuxPowerAdj {
            AuxPowerAdj(0)
        }
    }
    #[doc = "RWA, battery voltage detector configuration, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct BatDetCfg(pub u8);
    impl BatDetCfg {
        #[doc = "RWA, select threshold voltage of battery voltage low"]
        #[inline(always)]
        pub const fn bat_low_vth(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, select threshold voltage of battery voltage low"]
        #[inline(always)]
        pub fn set_bat_low_vth(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 0usize)) | (((val as u8) & 0x03) << 0usize);
        }
    }
    impl Default for BatDetCfg {
        #[inline(always)]
        fn default() -> BatDetCfg {
            BatDetCfg(0)
        }
    }
    #[doc = "RWA, battery voltage detector control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct BatDetCtrl(pub u8);
    impl BatDetCtrl {
        #[doc = "RWA, When BAT_MON_EN=0, it is enabled by high-precision battery voltage detection function"]
        #[inline(always)]
        pub const fn bat_det_en(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, When BAT_MON_EN=0, it is enabled by high-precision battery voltage detection function"]
        #[inline(always)]
        pub fn set_bat_det_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, When BAT_MON_EN=1, set low power consumption"]
        #[inline(always)]
        pub const fn bat_low_vthx(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, When BAT_MON_EN=1, set low power consumption"]
        #[inline(always)]
        pub fn set_bat_low_vthx(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, interrupt enable for battery lower voltage"]
        #[inline(always)]
        pub const fn bat_lower_ie(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, interrupt enable for battery lower voltage"]
        #[inline(always)]
        pub fn set_bat_lower_ie(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, interrupt enable for battery low voltage"]
        #[inline(always)]
        pub const fn bat_low_ie(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, interrupt enable for battery low voltage"]
        #[inline(always)]
        pub fn set_bat_low_ie(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
    }
    impl Default for BatDetCtrl {
        #[inline(always)]
        fn default() -> BatDetCtrl {
            BatDetCtrl(0)
        }
    }
    #[doc = "RO, battery status"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct BatStatus(pub u8);
    impl BatStatus {
        #[doc = "RO, battery lower voltage status, high action"]
        #[inline(always)]
        pub const fn bat_stat_lower(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RO, battery lower voltage status, high action"]
        #[inline(always)]
        pub fn set_bat_stat_lower(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RO, battery low voltage status, high action"]
        #[inline(always)]
        pub const fn bat_stat_low(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RO, battery low voltage status, high action"]
        #[inline(always)]
        pub fn set_bat_stat_low(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
    }
    impl Default for BatStatus {
        #[inline(always)]
        fn default() -> BatStatus {
            BatStatus(0)
        }
    }
    #[doc = "RWA, 32KHz oscillator configure"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Ck32kConfig(pub u8);
    impl Ck32kConfig {
        #[doc = "RWA, external 32KHz oscillator power on"]
        #[inline(always)]
        pub const fn clk_xt32k_pon(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, external 32KHz oscillator power on"]
        #[inline(always)]
        pub fn set_clk_xt32k_pon(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, internal 32KHz oscillator power on"]
        #[inline(always)]
        pub const fn clk_int32k_pon(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, internal 32KHz oscillator power on"]
        #[inline(always)]
        pub fn set_clk_int32k_pon(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, 32KHz oscillator source selection: 0=RC, 1=XT"]
        #[inline(always)]
        pub const fn clk_osc32k_xt(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, 32KHz oscillator source selection: 0=RC, 1=XT"]
        #[inline(always)]
        pub fn set_clk_osc32k_xt(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, internal 32KHz oscillator low noise mode enable"]
        #[inline(always)]
        pub const fn clk_osc32k_filt(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, internal 32KHz oscillator low noise mode enable"]
        #[inline(always)]
        pub fn set_clk_osc32k_filt(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RO, 32KHz oscillator clock pin status"]
        #[inline(always)]
        pub const fn _32k_clk_pin(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RO, 32KHz oscillator clock pin status"]
        #[inline(always)]
        pub fn set__32k_clk_pin(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u8) & 0x01) << 7usize);
        }
    }
    impl Default for Ck32kConfig {
        #[inline(always)]
        fn default() -> Ck32kConfig {
            Ck32kConfig(0)
        }
    }
    #[doc = "RWA, system clock configuration, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct ClkSysCfg(pub u16);
    impl ClkSysCfg {
        #[doc = "RWA, output clock divider from PLL or CK32M"]
        #[inline(always)]
        pub const fn clk_pll_div(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x1f;
            val as u8
        }
        #[doc = "RWA, output clock divider from PLL or CK32M"]
        #[inline(always)]
        pub fn set_clk_pll_div(&mut self, val: u8) {
            self.0 = (self.0 & !(0x1f << 0usize)) | (((val as u16) & 0x1f) << 0usize);
        }
        #[doc = "RWA, system clock source mode: 00=divided from 32MHz"]
        #[inline(always)]
        pub const fn clk_sys_mod(&self) -> u8 {
            let val = (self.0 >> 6usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, system clock source mode: 00=divided from 32MHz"]
        #[inline(always)]
        pub fn set_clk_sys_mod(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 6usize)) | (((val as u16) & 0x03) << 6usize);
        }
        #[doc = "RWA, XROM clock source"]
        #[inline(always)]
        pub const fn xrom_sclk_sel(&self) -> bool {
            let val = (self.0 >> 8usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, XROM clock source"]
        #[inline(always)]
        pub fn set_xrom_sclk_sel(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 8usize)) | (((val as u16) & 0x01) << 8usize);
        }
        #[doc = "RWA, Selection of PLL clock source"]
        #[inline(always)]
        pub const fn osc32m_sel(&self) -> bool {
            let val = (self.0 >> 9usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Selection of PLL clock source"]
        #[inline(always)]
        pub fn set_osc32m_sel(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 9usize)) | (((val as u16) & 0x01) << 9usize);
        }
        #[doc = "RWA, Do you turn off the PLL clock when switching the PLL clock source"]
        #[inline(always)]
        pub const fn pll_gate_diss(&self) -> bool {
            let val = (self.0 >> 12usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Do you turn off the PLL clock when switching the PLL clock source"]
        #[inline(always)]
        pub fn set_pll_gate_diss(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 12usize)) | (((val as u16) & 0x01) << 12usize);
        }
        #[doc = "RWA, When switching the PLL clock source, turn off the time selection of the PLL clock"]
        #[inline(always)]
        pub const fn pll_gate_time(&self) -> bool {
            let val = (self.0 >> 13usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, When switching the PLL clock source, turn off the time selection of the PLL clock"]
        #[inline(always)]
        pub fn set_pll_gate_time(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 13usize)) | (((val as u16) & 0x01) << 13usize);
        }
    }
    impl Default for ClkSysCfg {
        #[inline(always)]
        fn default() -> ClkSysCfg {
            ClkSysCfg(0)
        }
    }
    #[doc = "RWA, High frequency clock module power control register, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct HfckPwrCtrl(pub u8);
    impl HfckPwrCtrl {
        #[doc = "RWA,Enable items for the internal 16MHz oscillator HSI"]
        #[inline(always)]
        pub const fn clk_rc16m_pon(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA,Enable items for the internal 16MHz oscillator HSI"]
        #[inline(always)]
        pub fn set_clk_rc16m_pon(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, External 32MHz oscillator HSE power control bit"]
        #[inline(always)]
        pub const fn clk_xt32m_pon(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, External 32MHz oscillator HSE power control bit"]
        #[inline(always)]
        pub fn set_clk_xt32m_pon(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, Used to control the stopping of the clock system in pause mode"]
        #[inline(always)]
        pub const fn clk_xt32m_keep(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Used to control the stopping of the clock system in pause mode"]
        #[inline(always)]
        pub fn set_clk_xt32m_keep(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RWA, PLL power control bit"]
        #[inline(always)]
        pub const fn clk_pll_pon(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, PLL power control bit"]
        #[inline(always)]
        pub fn set_clk_pll_pon(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
    }
    impl Default for HfckPwrCtrl {
        #[inline(always)]
        fn default() -> HfckPwrCtrl {
            HfckPwrCtrl(0)
        }
    }
    #[doc = "RWA, internal 32KHz oscillator tune control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Int32kTune(pub u16);
    impl Int32kTune {
        #[doc = "RWA, Internal RC 32KHz clock frequency calibration value"]
        #[inline(always)]
        pub const fn int32k_tune(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0x1fff;
            val as u16
        }
        #[doc = "RWA, Internal RC 32KHz clock frequency calibration value"]
        #[inline(always)]
        pub fn set_int32k_tune(&mut self, val: u16) {
            self.0 = (self.0 & !(0x1fff << 0usize)) | (((val as u16) & 0x1fff) << 0usize);
        }
    }
    impl Default for Int32kTune {
        #[inline(always)]
        fn default() -> Int32kTune {
            Int32kTune(0)
        }
    }
    #[doc = "RW, Watchdog configuration register, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct IwdgCfg(pub u32);
    impl IwdgCfg {
        #[doc = "RW, RL \\[11:0\\]: Watchdog counter reload"]
        #[inline(always)]
        pub const fn rlr(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0x0fff;
            val as u16
        }
        #[doc = "RW, RL \\[11:0\\]: Watchdog counter reload"]
        #[inline(always)]
        pub fn set_rlr(&mut self, val: u16) {
            self.0 = (self.0 & !(0x0fff << 0usize)) | (((val as u32) & 0x0fff) << 0usize);
        }
        #[doc = "RO, Pre division factor (with write protection)"]
        #[inline(always)]
        pub const fn pr(&self) -> u8 {
            let val = (self.0 >> 12usize) & 0x07;
            val as u8
        }
        #[doc = "RO, Pre division factor (with write protection)"]
        #[inline(always)]
        pub fn set_pr(&mut self, val: u8) {
            self.0 = (self.0 & !(0x07 << 12usize)) | (((val as u32) & 0x07) << 12usize);
        }
        #[doc = "RO, Configure register update flag bit (with write protection)"]
        #[inline(always)]
        pub const fn pvu(&self) -> bool {
            let val = (self.0 >> 15usize) & 0x01;
            val != 0
        }
        #[doc = "RO, Configure register update flag bit (with write protection)"]
        #[inline(always)]
        pub fn set_pvu(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 15usize)) | (((val as u32) & 0x01) << 15usize);
        }
        #[doc = "RO, Watchdog countdown counter"]
        #[inline(always)]
        pub const fn count(&self) -> u16 {
            let val = (self.0 >> 16usize) & 0x0fff;
            val as u16
        }
        #[doc = "RO, Watchdog countdown counter"]
        #[inline(always)]
        pub fn set_count(&mut self, val: u16) {
            self.0 = (self.0 & !(0x0fff << 16usize)) | (((val as u32) & 0x0fff) << 16usize);
        }
        #[doc = "RW, Watchdog stop enabled (with write protection)"]
        #[inline(always)]
        pub const fn stop_en(&self) -> bool {
            let val = (self.0 >> 29usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Watchdog stop enabled (with write protection)"]
        #[inline(always)]
        pub fn set_stop_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 29usize)) | (((val as u32) & 0x01) << 29usize);
        }
        #[doc = "RW, Write protection"]
        #[inline(always)]
        pub const fn wr_protect(&self) -> bool {
            let val = (self.0 >> 30usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Write protection"]
        #[inline(always)]
        pub fn set_wr_protect(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 30usize)) | (((val as u32) & 0x01) << 30usize);
        }
        #[doc = "RW, Watch dog start switch: 1: ON; 0: Close"]
        #[inline(always)]
        pub const fn iwdg_en(&self) -> bool {
            let val = (self.0 >> 31usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Watch dog start switch: 1: ON; 0: Close"]
        #[inline(always)]
        pub fn set_iwdg_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 31usize)) | (((val as u32) & 0x01) << 31usize);
        }
    }
    impl Default for IwdgCfg {
        #[inline(always)]
        fn default() -> IwdgCfg {
            IwdgCfg(0)
        }
    }
    #[doc = "WO, Watchdog key register, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct IwdgKr(pub u32);
    impl IwdgKr {
        #[doc = "RO,KEY \\[15:0\\]: Key value"]
        #[inline(always)]
        pub const fn iwdg_kr(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0xffff;
            val as u16
        }
        #[doc = "RO,KEY \\[15:0\\]: Key value"]
        #[inline(always)]
        pub fn set_iwdg_kr(&mut self, val: u16) {
            self.0 = (self.0 & !(0xffff << 0usize)) | (((val as u32) & 0xffff) << 0usize);
        }
    }
    impl Default for IwdgKr {
        #[inline(always)]
        fn default() -> IwdgKr {
            IwdgKr(0)
        }
    }
    #[doc = "RW, function pin alternate configuration"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct PinAlternate(pub u16);
    impl PinAlternate {
        #[doc = "RW, TMR0 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_tmr0(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RW, TMR0 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_tmr0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u16) & 0x01) << 0usize);
        }
        #[doc = "RW, TMR1 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_tmr1(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RW, TMR1 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_tmr1(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u16) & 0x01) << 1usize);
        }
        #[doc = "RW, TMR2 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_tmr2(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RW, TMR2 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_tmr2(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u16) & 0x01) << 2usize);
        }
        #[doc = "RW, TMR3 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_tmr3(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RW, TMR3 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_tmr3(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u16) & 0x01) << 3usize);
        }
        #[doc = "RW, RXD0/TXD0 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_uart0(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RW, RXD0/TXD0 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_uart0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u16) & 0x01) << 4usize);
        }
        #[doc = "RW, RXD1/TXD1 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_uart1(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RW, RXD1/TXD1 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_uart1(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u16) & 0x01) << 5usize);
        }
        #[doc = "RW, RXD2/TXD2 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_uart2(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RW, RXD2/TXD2 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_uart2(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u16) & 0x01) << 6usize);
        }
        #[doc = "RW, RXD3/TXD3 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_uart3(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RW, RXD3/TXD3 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_uart3(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u16) & 0x01) << 7usize);
        }
        #[doc = "RW, SCS/SCK0/MOSI/MISO alternate pin enable"]
        #[inline(always)]
        pub const fn pin_spi0(&self) -> bool {
            let val = (self.0 >> 8usize) & 0x01;
            val != 0
        }
        #[doc = "RW, SCS/SCK0/MOSI/MISO alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_spi0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 8usize)) | (((val as u16) & 0x01) << 8usize);
        }
        #[doc = "RW, PWM4/PWM5/PWM7/PWM8/PWM9 alternate pin enable"]
        #[inline(always)]
        pub const fn pin_pwmx(&self) -> bool {
            let val = (self.0 >> 10usize) & 0x01;
            val != 0
        }
        #[doc = "RW, PWM4/PWM5/PWM7/PWM8/PWM9 alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_pwmx(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 10usize)) | (((val as u16) & 0x01) << 10usize);
        }
        #[doc = "RW, SCL/SDA alternate pin enable"]
        #[inline(always)]
        pub const fn pin_i2c(&self) -> bool {
            let val = (self.0 >> 11usize) & 0x01;
            val != 0
        }
        #[doc = "RW, SCL/SDA alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_i2c(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 11usize)) | (((val as u16) & 0x01) << 11usize);
        }
        #[doc = "RW, DSR/DTR alternate pin enable"]
        #[inline(always)]
        pub const fn pin_modem(&self) -> bool {
            let val = (self.0 >> 12usize) & 0x01;
            val != 0
        }
        #[doc = "RW, DSR/DTR alternate pin enable"]
        #[inline(always)]
        pub fn set_pin_modem(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 12usize)) | (((val as u16) & 0x01) << 12usize);
        }
        #[doc = "RW, INT24/INT25 function pin mapping selection bit"]
        #[inline(always)]
        pub const fn pin_intx(&self) -> bool {
            let val = (self.0 >> 13usize) & 0x01;
            val != 0
        }
        #[doc = "RW, INT24/INT25 function pin mapping selection bit"]
        #[inline(always)]
        pub fn set_pin_intx(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 13usize)) | (((val as u16) & 0x01) << 13usize);
        }
        #[doc = "RW, UART0 input/output inversion enable"]
        #[inline(always)]
        pub const fn pin_u0_inv(&self) -> bool {
            let val = (self.0 >> 14usize) & 0x01;
            val != 0
        }
        #[doc = "RW, UART0 input/output inversion enable"]
        #[inline(always)]
        pub fn set_pin_u0_inv(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 14usize)) | (((val as u16) & 0x01) << 14usize);
        }
        #[doc = "RW, RF antenna switch control output enable"]
        #[inline(always)]
        pub const fn rf_ant_sw_en(&self) -> bool {
            let val = (self.0 >> 15usize) & 0x01;
            val != 0
        }
        #[doc = "RW, RF antenna switch control output enable"]
        #[inline(always)]
        pub fn set_rf_ant_sw_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 15usize)) | (((val as u16) & 0x01) << 15usize);
        }
    }
    impl Default for PinAlternate {
        #[inline(always)]
        fn default() -> PinAlternate {
            PinAlternate(0)
        }
    }
    #[doc = "RW, Function pin configuration registere"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct PinConfig(pub u16);
    impl PinConfig {
        #[doc = "RW, Selection of interrupt pins for PB8 and PB16"]
        #[inline(always)]
        pub const fn pb16_8_sel(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Selection of interrupt pins for PB8 and PB16"]
        #[inline(always)]
        pub fn set_pb16_8_sel(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u16) & 0x01) << 4usize);
        }
        #[doc = "RW, Enable high-speed USB pins"]
        #[inline(always)]
        pub const fn pin_usb2_en(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Enable high-speed USB pins"]
        #[inline(always)]
        pub fn set_pin_usb2_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u16) & 0x01) << 5usize);
        }
        #[doc = "RW, Full speed USB UD+pin internal pull-up resistor enable"]
        #[inline(always)]
        pub const fn udp_pu_en(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Full speed USB UD+pin internal pull-up resistor enable"]
        #[inline(always)]
        pub fn set_udp_pu_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u16) & 0x01) << 6usize);
        }
        #[doc = "RW, Full speed USB pin enable"]
        #[inline(always)]
        pub const fn pin_usb_en(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RW, Full speed USB pin enable"]
        #[inline(always)]
        pub fn set_pin_usb_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u16) & 0x01) << 7usize);
        }
        #[doc = "RW, PB16~PB23 channel pin digital input disabled"]
        #[inline(always)]
        pub const fn pbhx_in_dis(&self) -> u8 {
            let val = (self.0 >> 8usize) & 0xff;
            val as u8
        }
        #[doc = "RW, PB16~PB23 channel pin digital input disabled"]
        #[inline(always)]
        pub fn set_pbhx_in_dis(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 8usize)) | (((val as u16) & 0xff) << 8usize);
        }
    }
    impl Default for PinConfig {
        #[inline(always)]
        fn default() -> PinConfig {
            PinConfig(0)
        }
    }
    #[doc = "RW, Function pin digital input disable register"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct PinInDis(pub u32);
    impl PinInDis {
        #[doc = "RW, PA0~PA15 channel pin digital input disabled"]
        #[inline(always)]
        pub const fn pax_in_dis(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0xffff;
            val as u16
        }
        #[doc = "RW, PA0~PA15 channel pin digital input disabled"]
        #[inline(always)]
        pub fn set_pax_in_dis(&mut self, val: u16) {
            self.0 = (self.0 & !(0xffff << 0usize)) | (((val as u32) & 0xffff) << 0usize);
        }
        #[doc = "RW, PB0~PB15 channel pin digital input disabled"]
        #[inline(always)]
        pub const fn pblx_in_dis(&self) -> u16 {
            let val = (self.0 >> 16usize) & 0xffff;
            val as u16
        }
        #[doc = "RW, PB0~PB15 channel pin digital input disabled"]
        #[inline(always)]
        pub fn set_pblx_in_dis(&mut self, val: u16) {
            self.0 = (self.0 & !(0xffff << 16usize)) | (((val as u32) & 0xffff) << 16usize);
        }
    }
    impl Default for PinInDis {
        #[inline(always)]
        fn default() -> PinInDis {
            PinInDis(0)
        }
    }
    #[doc = "RWA, power plan before sleep instruction, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct PowerPlan(pub u16);
    impl PowerPlan {
        #[doc = "RWA, power for FlashROM"]
        #[inline(always)]
        pub const fn pwr_xrom(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, power for FlashROM"]
        #[inline(always)]
        pub fn set_pwr_xrom(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u16) & 0x01) << 0usize);
        }
        #[doc = "RWA, SRAM power supply for RAM2K"]
        #[inline(always)]
        pub const fn pwr_ram32k(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, SRAM power supply for RAM2K"]
        #[inline(always)]
        pub fn set_pwr_ram32k(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u16) & 0x01) << 1usize);
        }
        #[doc = "RWA, power retention for core and base peripherals"]
        #[inline(always)]
        pub const fn pwr_core(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, power retention for core and base peripherals"]
        #[inline(always)]
        pub fn set_pwr_core(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u16) & 0x01) << 2usize);
        }
        #[doc = "RWA, power retention for USB and BLE"]
        #[inline(always)]
        pub const fn pwr_extend(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, power retention for USB and BLE"]
        #[inline(always)]
        pub fn set_pwr_extend(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u16) & 0x01) << 3usize);
        }
        #[doc = "RWA, SRAM power supply for RAM24K"]
        #[inline(always)]
        pub const fn pwr_ram96k(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, SRAM power supply for RAM24K"]
        #[inline(always)]
        pub fn set_pwr_ram96k(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u16) & 0x01) << 4usize);
        }
        #[doc = "RWA, Main power selection"]
        #[inline(always)]
        pub const fn main_act(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Main power selection"]
        #[inline(always)]
        pub fn set_main_act(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u16) & 0x01) << 6usize);
        }
        #[doc = "RWA, power for system"]
        #[inline(always)]
        pub const fn pwr_sys_en(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, power for system"]
        #[inline(always)]
        pub fn set_pwr_sys_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u16) & 0x01) << 7usize);
        }
        #[doc = "RWA, Internal LDO control"]
        #[inline(always)]
        pub const fn pwr_ldo_en(&self) -> bool {
            let val = (self.0 >> 8usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Internal LDO control"]
        #[inline(always)]
        pub fn set_pwr_ldo_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 8usize)) | (((val as u16) & 0x01) << 8usize);
        }
        #[doc = "RWA, DC/DC converter enable"]
        #[inline(always)]
        pub const fn pwr_dcdc_en(&self) -> bool {
            let val = (self.0 >> 9usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, DC/DC converter enable"]
        #[inline(always)]
        pub fn set_pwr_dcdc_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 9usize)) | (((val as u16) & 0x01) << 9usize);
        }
        #[doc = "RWA, DC/DC converter pre-enable"]
        #[inline(always)]
        pub const fn pwr_dcdc_pre(&self) -> bool {
            let val = (self.0 >> 10usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, DC/DC converter pre-enable"]
        #[inline(always)]
        pub fn set_pwr_dcdc_pre(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 10usize)) | (((val as u16) & 0x01) << 10usize);
        }
        #[doc = "RWA, Must write 0010."]
        #[inline(always)]
        pub const fn pwr_must_0010(&self) -> u8 {
            let val = (self.0 >> 11usize) & 0x0f;
            val as u8
        }
        #[doc = "RWA, Must write 0010."]
        #[inline(always)]
        pub fn set_pwr_must_0010(&mut self, val: u8) {
            self.0 = (self.0 & !(0x0f << 11usize)) | (((val as u16) & 0x0f) << 11usize);
        }
        #[doc = "RWA, Enable sleep power planning and control"]
        #[inline(always)]
        pub const fn pwr_plan_en(&self) -> bool {
            let val = (self.0 >> 15usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Enable sleep power planning and control"]
        #[inline(always)]
        pub fn set_pwr_plan_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 15usize)) | (((val as u16) & 0x01) << 15usize);
        }
    }
    impl Default for PowerPlan {
        #[inline(always)]
        fn default() -> PowerPlan {
            PowerPlan(0)
        }
    }
    #[doc = "RWA, Secure access to clock control registers, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SafeClkCtrl(pub u8);
    impl SafeClkCtrl {
        #[doc = "RWA, NFC clock off enable."]
        #[inline(always)]
        pub const fn clk_off_nfc(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, NFC clock off enable."]
        #[inline(always)]
        pub fn set_clk_off_nfc(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, ADC clock off enable"]
        #[inline(always)]
        pub const fn clk_off_adc(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, ADC clock off enable"]
        #[inline(always)]
        pub fn set_clk_off_adc(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, LED clock off enable"]
        #[inline(always)]
        pub const fn clk_off_led(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, LED clock off enable"]
        #[inline(always)]
        pub fn set_clk_off_led(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
    }
    impl Default for SafeClkCtrl {
        #[inline(always)]
        fn default() -> SafeClkCtrl {
            SafeClkCtrl(0)
        }
    }
    #[doc = "RWA, Secure Access Debugging Control Register, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SafeDebugCtrl(pub u8);
    impl SafeDebugCtrl {
        #[doc = "RWA, DEBUG disable enable."]
        #[inline(always)]
        pub const fn debug_dis(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, DEBUG disable enable."]
        #[inline(always)]
        pub fn set_debug_dis(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
    }
    impl Default for SafeDebugCtrl {
        #[inline(always)]
        fn default() -> SafeDebugCtrl {
            SafeDebugCtrl(0)
        }
    }
    #[doc = "RWA, Long term reset control register for secure access, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SafeLrstCtrl(pub u8);
    impl SafeLrstCtrl {
        #[doc = "RWA, Long reset enable."]
        #[inline(always)]
        pub const fn long_rst_en(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Long reset enable."]
        #[inline(always)]
        pub fn set_long_rst_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, Long reset time selection."]
        #[inline(always)]
        pub const fn long_tim_sel(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Long reset time selection."]
        #[inline(always)]
        pub fn set_long_tim_sel(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, Independent watchdog reset enable."]
        #[inline(always)]
        pub const fn iwdg_rst_en(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Independent watchdog reset enable."]
        #[inline(always)]
        pub fn set_iwdg_rst_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
    }
    impl Default for SafeLrstCtrl {
        #[inline(always)]
        fn default() -> SafeLrstCtrl {
            SafeLrstCtrl(0)
        }
    }
    #[doc = "RWA, Secure Access Mode Control Register, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SafeModeCtrl(pub u8);
    impl SafeModeCtrl {
        #[doc = "RWA, Enable automatic shutdown of security register."]
        #[inline(always)]
        pub const fn safe_auto_en(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Enable automatic shutdown of security register."]
        #[inline(always)]
        pub fn set_safe_auto_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, XROM Clock Selection"]
        #[inline(always)]
        pub const fn xrom_312m_sel(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, XROM Clock Selection"]
        #[inline(always)]
        pub fn set_xrom_312m_sel(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
    }
    impl Default for SafeModeCtrl {
        #[inline(always)]
        fn default() -> SafeModeCtrl {
            SafeModeCtrl(0)
        }
    }
    #[doc = "RWA, sleep clock off control byte 0, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SlpClkOff0(pub u8);
    impl SlpClkOff0 {
        #[doc = "RWA, Timer 0 clock source"]
        #[inline(always)]
        pub const fn slp_clk_tmr0(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Timer 0 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_tmr0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, Timer 1 clock source"]
        #[inline(always)]
        pub const fn slp_clk_tmr1(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Timer 1 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_tmr1(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, Timer 2 clock source"]
        #[inline(always)]
        pub const fn slp_clk_tmr2(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Timer 2 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_tmr2(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, Timer 3 clock source"]
        #[inline(always)]
        pub const fn slp_clk_tmr3(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Timer 3 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_tmr3(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RWA, UART0 clock source"]
        #[inline(always)]
        pub const fn slp_clk_uart0(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, UART0 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_uart0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RWA, UART1 clock source"]
        #[inline(always)]
        pub const fn slp_clk_uart1(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, UART1 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_uart1(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
        #[doc = "RWA, UART2 clock source"]
        #[inline(always)]
        pub const fn slp_clk_uart2(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, UART2 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_uart2(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u8) & 0x01) << 6usize);
        }
        #[doc = "RWA, UART3 clock source"]
        #[inline(always)]
        pub const fn slp_clk_uart3(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, UART3 clock source"]
        #[inline(always)]
        pub fn set_slp_clk_uart3(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u8) & 0x01) << 7usize);
        }
    }
    impl Default for SlpClkOff0 {
        #[inline(always)]
        fn default() -> SlpClkOff0 {
            SlpClkOff0(0)
        }
    }
    #[doc = "RWA, sleep clock off control byte 1, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SlpClkOff1(pub u8);
    impl SlpClkOff1 {
        #[doc = "RWA, close SPI0 clock"]
        #[inline(always)]
        pub const fn slp_clk_spi0(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close SPI0 clock"]
        #[inline(always)]
        pub fn set_slp_clk_spi0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, close SPI1 clock"]
        #[inline(always)]
        pub const fn slp_clk_spi1(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close SPI1 clock"]
        #[inline(always)]
        pub fn set_slp_clk_spi1(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, close PWMx clock"]
        #[inline(always)]
        pub const fn slp_clk_pwmx(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close PWMx clock"]
        #[inline(always)]
        pub fn set_slp_clk_pwmx(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, close I2C clock"]
        #[inline(always)]
        pub const fn slp_clk_i2c(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close I2C clock"]
        #[inline(always)]
        pub fn set_slp_clk_i2c(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RWA, close USB clock"]
        #[inline(always)]
        pub const fn slp_clk_usb(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close USB clock"]
        #[inline(always)]
        pub fn set_slp_clk_usb(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RWA, close USB2 clock"]
        #[inline(always)]
        pub const fn slp_clk_usb2(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close USB2 clock"]
        #[inline(always)]
        pub fn set_slp_clk_usb2(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
        #[doc = "RWA, close LCD clock"]
        #[inline(always)]
        pub const fn slp_clk_lcd(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close LCD clock"]
        #[inline(always)]
        pub fn set_slp_clk_lcd(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u8) & 0x01) << 6usize);
        }
        #[doc = "RWA, close BLE clock"]
        #[inline(always)]
        pub const fn slp_clk_ble(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close BLE clock"]
        #[inline(always)]
        pub fn set_slp_clk_ble(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u8) & 0x01) << 7usize);
        }
    }
    impl Default for SlpClkOff1 {
        #[inline(always)]
        fn default() -> SlpClkOff1 {
            SlpClkOff1(0)
        }
    }
    #[doc = "RWA, peripherals power down control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SlpPowerCtrl(pub u8);
    impl SlpPowerCtrl {
        #[doc = "RWA, wakeup delay time selection"]
        #[inline(always)]
        pub const fn wake_dly_mod(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, wakeup delay time selection"]
        #[inline(always)]
        pub fn set_wake_dly_mod(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 0usize)) | (((val as u8) & 0x03) << 0usize);
        }
        #[doc = "RWA, Utmi_clk clock source"]
        #[inline(always)]
        pub const fn clk_off_utmi(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, Utmi_clk clock source"]
        #[inline(always)]
        pub fn set_clk_off_utmi(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, close main SRAM clock"]
        #[inline(always)]
        pub const fn slp_clk_ramx(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close main SRAM clock"]
        #[inline(always)]
        pub fn set_slp_clk_ramx(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RWA, close retention 2KB SRAM clock"]
        #[inline(always)]
        pub const fn slp_clk_ram2k(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, close retention 2KB SRAM clock"]
        #[inline(always)]
        pub fn set_slp_clk_ram2k(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
        #[doc = "RWA, SRAM retention voltage selection"]
        #[inline(always)]
        pub const fn ram_ret_lv(&self) -> u8 {
            let val = (self.0 >> 6usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, SRAM retention voltage selection"]
        #[inline(always)]
        pub fn set_ram_ret_lv(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 6usize)) | (((val as u8) & 0x03) << 6usize);
        }
    }
    impl Default for SlpPowerCtrl {
        #[inline(always)]
        fn default() -> SlpPowerCtrl {
            SlpPowerCtrl(0)
        }
    }
    #[doc = "RWA, wake control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SlpWakeCtrl(pub u8);
    impl SlpWakeCtrl {
        #[doc = "RWA, enable USB waking"]
        #[inline(always)]
        pub const fn slp_usb_wake(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable USB waking"]
        #[inline(always)]
        pub fn set_slp_usb_wake(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, enable USB2 waking"]
        #[inline(always)]
        pub const fn slp_usb2_wake(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable USB2 waking"]
        #[inline(always)]
        pub fn set_slp_usb2_wake(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, GPIO edge wake-up mode selection"]
        #[inline(always)]
        pub const fn gpio_edge_wake(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, GPIO edge wake-up mode selection"]
        #[inline(always)]
        pub fn set_gpio_edge_wake(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RWA, enable RTC waking"]
        #[inline(always)]
        pub const fn slp_rtc_wake(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable RTC waking"]
        #[inline(always)]
        pub fn set_slp_rtc_wake(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RWA, enable GPIO waking"]
        #[inline(always)]
        pub const fn slp_gpio_wake(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable GPIO waking"]
        #[inline(always)]
        pub fn set_slp_gpio_wake(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RWA, enable BAT waking"]
        #[inline(always)]
        pub const fn slp_bat_wake(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable BAT waking"]
        #[inline(always)]
        pub fn set_slp_bat_wake(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
        #[doc = "RWA, event wakeup mode"]
        #[inline(always)]
        pub const fn wake_ev_mode(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, event wakeup mode"]
        #[inline(always)]
        pub fn set_wake_ev_mode(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u8) & 0x01) << 6usize);
        }
        #[doc = "RWA, enable GPIO wake-up mode"]
        #[inline(always)]
        pub const fn gpio_wake_mode(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable GPIO wake-up mode"]
        #[inline(always)]
        pub fn set_gpio_wake_mode(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u8) & 0x01) << 7usize);
        }
    }
    impl Default for SlpWakeCtrl {
        #[inline(always)]
        fn default() -> SlpWakeCtrl {
            SlpWakeCtrl(0)
        }
    }
    #[doc = "RWA, External 32KHz oscillator tune control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Xt32kTune(pub u8);
    impl Xt32kTune {
        #[doc = "RWA, Selection of bias current for external 32KHz oscillator"]
        #[inline(always)]
        pub const fn xt32k_i_tune(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, Selection of bias current for external 32KHz oscillator"]
        #[inline(always)]
        pub fn set_xt32k_i_tune(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 0usize)) | (((val as u8) & 0x03) << 0usize);
        }
        #[doc = "RWA, RC32K adjustment bit, internal 32KHz oscillator bias current selection"]
        #[inline(always)]
        pub const fn rc32k_i_tune(&self) -> u8 {
            let val = (self.0 >> 2usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, RC32K adjustment bit, internal 32KHz oscillator bias current selection"]
        #[inline(always)]
        pub fn set_rc32k_i_tune(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 2usize)) | (((val as u8) & 0x03) << 2usize);
        }
        #[doc = "RWA, Choose a built-in load capacitor that matches the external 32KHz crystal"]
        #[inline(always)]
        pub const fn xt32k_c_load(&self) -> u8 {
            let val = (self.0 >> 4usize) & 0x0f;
            val as u8
        }
        #[doc = "RWA, Choose a built-in load capacitor that matches the external 32KHz crystal"]
        #[inline(always)]
        pub fn set_xt32k_c_load(&mut self, val: u8) {
            self.0 = (self.0 & !(0x0f << 4usize)) | (((val as u8) & 0x0f) << 4usize);
        }
    }
    impl Default for Xt32kTune {
        #[inline(always)]
        fn default() -> Xt32kTune {
            Xt32kTune(0)
        }
    }
}
