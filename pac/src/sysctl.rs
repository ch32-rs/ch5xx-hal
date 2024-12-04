#![allow(clippy::missing_safety_doc)]
#![allow(clippy::identity_op)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::erasing_op)]

#[doc = "System control related registers"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Sysctl {
    ptr: *mut u8,
}
unsafe impl Send for Sysctl {}
unsafe impl Sync for Sysctl {}
impl Sysctl {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "WO, safe accessing sign register, must write SAFE_ACCESS_SIG1 then SAFE_ACCESS_SIG2 to enter safe accessing mode"]
    #[inline(always)]
    pub const fn safe_access_sig(
        self,
    ) -> crate::common::Reg<regs::SafeAccessSig, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x40usize) as _) }
    }
    #[doc = "RF, chip ID register, always is ID_CH59*"]
    #[inline(always)]
    pub const fn chip_id(self) -> crate::common::Reg<regs::ChipId, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x41usize) as _) }
    }
    #[doc = "RF, safe accessing ID register, always 0x0C"]
    #[inline(always)]
    pub const fn safe_access_id(self) -> crate::common::Reg<regs::SafeAccessId, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x42usize) as _) }
    }
    #[doc = "RW, watch-dog count, count by clock frequency Fsys/131072"]
    #[inline(always)]
    pub const fn wdog_count(self) -> crate::common::Reg<regs::WdogCount, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x43usize) as _) }
    }
    #[doc = "RWA, reset status, SAM or flash ROM configuration"]
    #[inline(always)]
    pub const fn reset_status_glob_rom_cfg(
        self,
    ) -> crate::common::Reg<regs::ResetStatusGlobRomCfg, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x44usize) as _) }
    }
    #[doc = "RO, global configuration information and status"]
    #[inline(always)]
    pub const fn glob_cfg_info(self) -> crate::common::Reg<regs::GlobCfgInfo, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x45usize) as _) }
    }
    #[doc = "RWA, reset and watch-dog control, SAM"]
    #[inline(always)]
    pub const fn rst_wdog_ctrl(self) -> crate::common::Reg<regs::RstWdogCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x46usize) as _) }
    }
    #[doc = "RW, value keeper during global reset"]
    #[inline(always)]
    pub const fn glob_reset_keep(
        self,
    ) -> crate::common::Reg<regs::GlobResetKeep, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x47usize) as _) }
    }
    #[doc = "RWA, PLL configuration control, SAM"]
    #[inline(always)]
    pub const fn pll_config(self) -> crate::common::Reg<regs::PllConfig, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x4busize) as _) }
    }
    #[doc = "RWA, external 32MHz oscillator tune control, SAM"]
    #[inline(always)]
    pub const fn xt32m_tune(self) -> crate::common::Reg<regs::Xt32mTune, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x4eusize) as _) }
    }
    #[doc = "RO, system clock count value for 32KHz multi-cycles"]
    #[inline(always)]
    pub const fn osc_cal_cnt(self) -> crate::common::Reg<regs::OscCalCnt, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x50usize) as _) }
    }
    #[doc = "RO, oscillator frequency calibration overflow times"]
    #[inline(always)]
    pub const fn osc_cal_ov_cnt(self) -> crate::common::Reg<regs::OscCalOvCnt, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x52usize) as _) }
    }
    #[doc = "RWA, oscillator frequency calibration control, SAM"]
    #[inline(always)]
    pub const fn osc_cal_ctrl(self) -> crate::common::Reg<regs::OscCalCtrl, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x53usize) as _) }
    }
}
pub mod regs {
    #[doc = "RF, chip ID register, always is ID_CH59*"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct ChipId(pub u8);
    impl ChipId {
        #[doc = "RF,chip ID register"]
        #[inline(always)]
        pub const fn chip_id(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "RF,chip ID register"]
        #[inline(always)]
        pub fn set_chip_id(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u8) & 0xff) << 0usize);
        }
    }
    impl Default for ChipId {
        #[inline(always)]
        fn default() -> ChipId {
            ChipId(0)
        }
    }
    #[doc = "RO, global configuration information and status"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct GlobCfgInfo(pub u8);
    impl GlobCfgInfo {
        #[doc = "RO, indicate protected status of Flash ROM code and data: 0=reading protect, 1=enable read by external programmer"]
        #[inline(always)]
        pub const fn cfg_rom_read(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "RO, indicate protected status of Flash ROM code and data: 0=reading protect, 1=enable read by external programmer"]
        #[inline(always)]
        pub fn set_cfg_rom_read(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RO, manual reset input enable status"]
        #[inline(always)]
        pub const fn cfg_reset_en(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RO, manual reset input enable status"]
        #[inline(always)]
        pub fn set_cfg_reset_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RO, boot-loader enable status"]
        #[inline(always)]
        pub const fn cfg_boot_en(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RO, boot-loader enable status"]
        #[inline(always)]
        pub fn set_cfg_boot_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RO, debug enable status"]
        #[inline(always)]
        pub const fn cfg_debug_en(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RO, debug enable status"]
        #[inline(always)]
        pub fn set_cfg_debug_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RO, indicate boot loader status: 0=application status (by software reset), 1=boot loader status"]
        #[inline(always)]
        pub const fn boot_loader(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RO, indicate boot loader status: 0=application status (by software reset), 1=boot loader status"]
        #[inline(always)]
        pub fn set_boot_loader(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
    }
    impl Default for GlobCfgInfo {
        #[inline(always)]
        fn default() -> GlobCfgInfo {
            GlobCfgInfo(0)
        }
    }
    #[doc = "RW, value keeper during global reset"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct GlobResetKeep(pub u8);
    impl GlobResetKeep {
        #[doc = "RW, value keeper during global reset"]
        #[inline(always)]
        pub const fn glob_reset_keep(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "RW, value keeper during global reset"]
        #[inline(always)]
        pub fn set_glob_reset_keep(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u8) & 0xff) << 0usize);
        }
    }
    impl Default for GlobResetKeep {
        #[inline(always)]
        fn default() -> GlobResetKeep {
            GlobResetKeep(0)
        }
    }
    #[doc = "RO, system clock count value for 32KHz multi-cycles"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct OscCalCnt(pub u16);
    impl OscCalCnt {
        #[doc = "RO, system clock count value for 32KHz multi-cycles"]
        #[inline(always)]
        pub const fn osc_cal_cnt(&self) -> u16 {
            let val = (self.0 >> 0usize) & 0x3fff;
            val as u16
        }
        #[doc = "RO, system clock count value for 32KHz multi-cycles"]
        #[inline(always)]
        pub fn set_osc_cal_cnt(&mut self, val: u16) {
            self.0 = (self.0 & !(0x3fff << 0usize)) | (((val as u16) & 0x3fff) << 0usize);
        }
        #[doc = "RW1, indicate OSC_CAL_OV_CNT not zero, set 1 to clear OSC_CAL_OV_CNT"]
        #[inline(always)]
        pub const fn osc_cal_ov_clr(&self) -> bool {
            let val = (self.0 >> 14usize) & 0x01;
            val != 0
        }
        #[doc = "RW1, indicate OSC_CAL_OV_CNT not zero, set 1 to clear OSC_CAL_OV_CNT"]
        #[inline(always)]
        pub fn set_osc_cal_ov_clr(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 14usize)) | (((val as u16) & 0x01) << 14usize);
        }
        #[doc = "RW1, interrupt flag for oscillator capture end, set 1 to clear"]
        #[inline(always)]
        pub const fn osc_cal_if(&self) -> bool {
            let val = (self.0 >> 15usize) & 0x01;
            val != 0
        }
        #[doc = "RW1, interrupt flag for oscillator capture end, set 1 to clear"]
        #[inline(always)]
        pub fn set_osc_cal_if(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 15usize)) | (((val as u16) & 0x01) << 15usize);
        }
    }
    impl Default for OscCalCnt {
        #[inline(always)]
        fn default() -> OscCalCnt {
            OscCalCnt(0)
        }
    }
    #[doc = "RWA, oscillator frequency calibration control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct OscCalCtrl(pub u8);
    impl OscCalCtrl {
        #[doc = "RWA, total cycles mode for oscillator capture"]
        #[inline(always)]
        pub const fn osc_cnt_total(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x07;
            val as u8
        }
        #[doc = "RWA, total cycles mode for oscillator capture"]
        #[inline(always)]
        pub fn set_osc_cnt_total(&mut self, val: u8) {
            self.0 = (self.0 & !(0x07 << 0usize)) | (((val as u8) & 0x07) << 0usize);
        }
        #[doc = "RO, calibration counter halt status: 0=counting, 1=halt for reading count value"]
        #[inline(always)]
        pub const fn osc_cnt_halt(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RO, calibration counter halt status: 0=counting, 1=halt for reading count value"]
        #[inline(always)]
        pub fn set_osc_cnt_halt(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RWA, interrupt enable for oscillator capture end"]
        #[inline(always)]
        pub const fn osc_cal_ie(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, interrupt enable for oscillator capture end"]
        #[inline(always)]
        pub fn set_osc_cal_ie(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RWA, calibration counter enable"]
        #[inline(always)]
        pub const fn osc_cnt_en(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, calibration counter enable"]
        #[inline(always)]
        pub fn set_osc_cnt_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
        #[doc = "RWA, select oscillator capture end mode: 0=normal, 1=append 2 cycles"]
        #[inline(always)]
        pub const fn osc_cnt_end(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, select oscillator capture end mode: 0=normal, 1=append 2 cycles"]
        #[inline(always)]
        pub fn set_osc_cnt_end(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u8) & 0x01) << 6usize);
        }
    }
    impl Default for OscCalCtrl {
        #[inline(always)]
        fn default() -> OscCalCtrl {
            OscCalCtrl(0)
        }
    }
    #[doc = "RO, oscillator frequency calibration overflow times"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct OscCalOvCnt(pub u8);
    impl OscCalOvCnt {
        #[doc = "RO, oscillator frequency calibration overflow times"]
        #[inline(always)]
        pub const fn osc_cal_ov_cnt(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "RO, oscillator frequency calibration overflow times"]
        #[inline(always)]
        pub fn set_osc_cal_ov_cnt(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u8) & 0xff) << 0usize);
        }
    }
    impl Default for OscCalOvCnt {
        #[inline(always)]
        fn default() -> OscCalOvCnt {
            OscCalOvCnt(0)
        }
    }
    #[doc = "RWA, PLL configuration control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct PllConfig(pub u8);
    impl PllConfig {
        #[doc = "RWA, PLL configure data"]
        #[inline(always)]
        pub const fn pll_cfg_dat(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x3f;
            val as u8
        }
        #[doc = "RWA, PLL configure data"]
        #[inline(always)]
        pub fn set_pll_cfg_dat(&mut self, val: u8) {
            self.0 = (self.0 & !(0x3f << 0usize)) | (((val as u8) & 0x3f) << 0usize);
        }
    }
    impl Default for PllConfig {
        #[inline(always)]
        fn default() -> PllConfig {
            PllConfig(0)
        }
    }
    #[doc = "RWA, reset status, SAM or flash ROM configuration"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct ResetStatusGlobRomCfg(pub u8);
    impl ResetStatusGlobRomCfg {
        #[doc = "RO, recent reset flag"]
        #[inline(always)]
        pub const fn reset_flag(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x07;
            val as u8
        }
        #[doc = "RO, recent reset flag"]
        #[inline(always)]
        pub fn set_reset_flag(&mut self, val: u8) {
            self.0 = (self.0 & !(0x07 << 0usize)) | (((val as u8) & 0x07) << 0usize);
        }
        #[doc = "RWA, code offset address selection in Flash ROM: 0=start address 0x000000, 1=start address 0x008000"]
        #[inline(always)]
        pub const fn rom_code_ofs(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, code offset address selection in Flash ROM: 0=start address 0x000000, 1=start address 0x008000"]
        #[inline(always)]
        pub fn set_rom_code_ofs(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
        #[doc = "RWA, enable flash ROM control interface enable"]
        #[inline(always)]
        pub const fn rom_ctrl_en(&self) -> bool {
            let val = (self.0 >> 5usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable flash ROM control interface enable"]
        #[inline(always)]
        pub fn set_rom_ctrl_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 5usize)) | (((val as u8) & 0x01) << 5usize);
        }
        #[doc = "RWA,enable flash ROM data and code area being erase/write"]
        #[inline(always)]
        pub const fn rom_data_we(&self) -> bool {
            let val = (self.0 >> 6usize) & 0x01;
            val != 0
        }
        #[doc = "RWA,enable flash ROM data and code area being erase/write"]
        #[inline(always)]
        pub fn set_rom_data_we(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 6usize)) | (((val as u8) & 0x01) << 6usize);
        }
        #[doc = "RWA, enable flash ROM code area being erase or write"]
        #[inline(always)]
        pub const fn rom_code_we(&self) -> bool {
            let val = (self.0 >> 7usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable flash ROM code area being erase or write"]
        #[inline(always)]
        pub fn set_rom_code_we(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 7usize)) | (((val as u8) & 0x01) << 7usize);
        }
    }
    impl Default for ResetStatusGlobRomCfg {
        #[inline(always)]
        fn default() -> ResetStatusGlobRomCfg {
            ResetStatusGlobRomCfg(0)
        }
    }
    #[doc = "RWA, reset and watch-dog control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct RstWdogCtrl(pub u8);
    impl RstWdogCtrl {
        #[doc = "WA or WZ, global software reset, high action, auto clear"]
        #[inline(always)]
        pub const fn software_reset(&self) -> bool {
            let val = (self.0 >> 0usize) & 0x01;
            val != 0
        }
        #[doc = "WA or WZ, global software reset, high action, auto clear"]
        #[inline(always)]
        pub fn set_software_reset(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 0usize)) | (((val as u8) & 0x01) << 0usize);
        }
        #[doc = "RWA, enable watch-dog reset if watch-dog timer overflow: 0=as timer only, 1=enable reset if timer overflow"]
        #[inline(always)]
        pub const fn wdog_rst_en(&self) -> bool {
            let val = (self.0 >> 1usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, enable watch-dog reset if watch-dog timer overflow: 0=as timer only, 1=enable reset if timer overflow"]
        #[inline(always)]
        pub fn set_wdog_rst_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 1usize)) | (((val as u8) & 0x01) << 1usize);
        }
        #[doc = "RWA, watch-dog timer overflow interrupt enable: 0=disable, 1=enable"]
        #[inline(always)]
        pub const fn wdog_int_en(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RWA, watch-dog timer overflow interrupt enable: 0=disable, 1=enable"]
        #[inline(always)]
        pub fn set_wdog_int_en(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RW1, watch-dog timer overflow interrupt flag, cleared by RW1 or reload watch-dog count or __SEV(Send-Event)"]
        #[inline(always)]
        pub const fn wdog_int_flag(&self) -> bool {
            let val = (self.0 >> 4usize) & 0x01;
            val != 0
        }
        #[doc = "RW1, watch-dog timer overflow interrupt flag, cleared by RW1 or reload watch-dog count or __SEV(Send-Event)"]
        #[inline(always)]
        pub fn set_wdog_int_flag(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 4usize)) | (((val as u8) & 0x01) << 4usize);
        }
    }
    impl Default for RstWdogCtrl {
        #[inline(always)]
        fn default() -> RstWdogCtrl {
            RstWdogCtrl(0)
        }
    }
    #[doc = "RF, safe accessing ID register, always 0x0C"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SafeAccessId(pub u8);
    impl SafeAccessId {
        #[doc = "RF,safe accessing ID register"]
        #[inline(always)]
        pub const fn safe_access_id(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "RF,safe accessing ID register"]
        #[inline(always)]
        pub fn set_safe_access_id(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u8) & 0xff) << 0usize);
        }
    }
    impl Default for SafeAccessId {
        #[inline(always)]
        fn default() -> SafeAccessId {
            SafeAccessId(0)
        }
    }
    #[doc = "WO, safe accessing sign register, must write SAFE_ACCESS_SIG1 then SAFE_ACCESS_SIG2 to enter safe accessing mode"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct SafeAccessSig(pub u8);
    impl SafeAccessSig {
        #[doc = "RO, current safe accessing mode: 11=safe unlocked (SAM), other=locked (00..01..10..11)"]
        #[inline(always)]
        pub const fn safe_acc_mode(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x03;
            val as u8
        }
        #[doc = "RO, current safe accessing mode: 11=safe unlocked (SAM), other=locked (00..01..10..11)"]
        #[inline(always)]
        pub fn set_safe_acc_mode(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 0usize)) | (((val as u8) & 0x03) << 0usize);
        }
        #[doc = "WO, safe accessing sign register, must write 0x57 then 0xA8 to enter safe accessing mode"]
        #[inline(always)]
        pub const fn safe_access_sig(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "WO, safe accessing sign register, must write 0x57 then 0xA8 to enter safe accessing mode"]
        #[inline(always)]
        pub fn set_safe_access_sig(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u8) & 0xff) << 0usize);
        }
        #[doc = "RO, Chip identification code"]
        #[inline(always)]
        pub const fn chip_id0(&self) -> bool {
            let val = (self.0 >> 2usize) & 0x01;
            val != 0
        }
        #[doc = "RO, Chip identification code"]
        #[inline(always)]
        pub fn set_chip_id0(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 2usize)) | (((val as u8) & 0x01) << 2usize);
        }
        #[doc = "RO, indicate safe accessing status now: 0=locked, read-only, 1=safe/unlocked (SAM), write enabled"]
        #[inline(always)]
        pub const fn safe_acc_act(&self) -> bool {
            let val = (self.0 >> 3usize) & 0x01;
            val != 0
        }
        #[doc = "RO, indicate safe accessing status now: 0=locked, read-only, 1=safe/unlocked (SAM), write enabled"]
        #[inline(always)]
        pub fn set_safe_acc_act(&mut self, val: bool) {
            self.0 = (self.0 & !(0x01 << 3usize)) | (((val as u8) & 0x01) << 3usize);
        }
        #[doc = "RO, safe accessing timer bit mask (16*clock number)"]
        #[inline(always)]
        pub const fn safe_acc_timer(&self) -> u8 {
            let val = (self.0 >> 4usize) & 0x07;
            val as u8
        }
        #[doc = "RO, safe accessing timer bit mask (16*clock number)"]
        #[inline(always)]
        pub fn set_safe_acc_timer(&mut self, val: u8) {
            self.0 = (self.0 & !(0x07 << 4usize)) | (((val as u8) & 0x07) << 4usize);
        }
    }
    impl Default for SafeAccessSig {
        #[inline(always)]
        fn default() -> SafeAccessSig {
            SafeAccessSig(0)
        }
    }
    #[doc = "RW, watch-dog count, count by clock frequency Fsys/131072"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct WdogCount(pub u8);
    impl WdogCount {
        #[doc = "RW,watch-dog count, count by clock frequency Fsys/131072"]
        #[inline(always)]
        pub const fn wdog_count(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0xff;
            val as u8
        }
        #[doc = "RW,watch-dog count, count by clock frequency Fsys/131072"]
        #[inline(always)]
        pub fn set_wdog_count(&mut self, val: u8) {
            self.0 = (self.0 & !(0xff << 0usize)) | (((val as u8) & 0xff) << 0usize);
        }
    }
    impl Default for WdogCount {
        #[inline(always)]
        fn default() -> WdogCount {
            WdogCount(0)
        }
    }
    #[doc = "RWA, external 32MHz oscillator tune control, SAM"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Xt32mTune(pub u8);
    impl Xt32mTune {
        #[doc = "RWA, external 32MHz oscillator bias current tune: 00=75% current, 01=standard current, 10=125% current, 11=150% current"]
        #[inline(always)]
        pub const fn xt32m_i_bias(&self) -> u8 {
            let val = (self.0 >> 0usize) & 0x03;
            val as u8
        }
        #[doc = "RWA, external 32MHz oscillator bias current tune: 00=75% current, 01=standard current, 10=125% current, 11=150% current"]
        #[inline(always)]
        pub fn set_xt32m_i_bias(&mut self, val: u8) {
            self.0 = (self.0 & !(0x03 << 0usize)) | (((val as u8) & 0x03) << 0usize);
        }
        #[doc = "RWA, external 32MHz oscillator load capacitor tune: Cap = XT32M_C_LOAD * 2 + 10pF"]
        #[inline(always)]
        pub const fn xt32m_c_load(&self) -> u8 {
            let val = (self.0 >> 4usize) & 0x0f;
            val as u8
        }
        #[doc = "RWA, external 32MHz oscillator load capacitor tune: Cap = XT32M_C_LOAD * 2 + 10pF"]
        #[inline(always)]
        pub fn set_xt32m_c_load(&mut self, val: u8) {
            self.0 = (self.0 & !(0x0f << 4usize)) | (((val as u8) & 0x0f) << 4usize);
        }
    }
    impl Default for Xt32mTune {
        #[inline(always)]
        fn default() -> Xt32mTune {
            Xt32mTune(0)
        }
    }
}
