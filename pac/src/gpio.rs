#![allow(clippy::missing_safety_doc)]
#![allow(clippy::identity_op)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::erasing_op)]

#[doc = "General porpose intput output"]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Gpio {
    ptr: *mut u8,
}
unsafe impl Send for Gpio {}
unsafe impl Sync for Gpio {}
impl Gpio {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "RW, GPIO PA interrupt enable"]
    #[inline(always)]
    pub const fn port_int_en(self, n: usize) -> crate::common::Reg<regs::IntEn, crate::common::RW> {
        assert!(n < 2usize);
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x90usize + n * 2usize) as _) }
    }
    #[doc = "RW, GPIO PA interrupt mode: 0=level action, 1=edge action"]
    #[inline(always)]
    pub const fn port_int_mode(
        self,
        n: usize,
    ) -> crate::common::Reg<regs::IntMode, crate::common::RW> {
        assert!(n < 2usize);
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x94usize + n * 2usize) as _) }
    }
    #[doc = "RW1, GPIO PA interrupt flag"]
    #[inline(always)]
    pub const fn port_int_if(self, n: usize) -> crate::common::Reg<regs::IntIf, crate::common::RW> {
        assert!(n < 2usize);
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x9cusize + n * 2usize) as _) }
    }
    #[doc = "GPIO port A and B"]
    #[inline(always)]
    pub const fn port(self, n: usize) -> Port {
        assert!(n < 2usize);
        unsafe { Port::from_ptr(self.ptr.add(0xa0usize + n * 32usize) as _) }
    }
}
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Port {
    ptr: *mut u8,
}
unsafe impl Send for Port {}
unsafe impl Sync for Port {}
impl Port {
    #[inline(always)]
    pub const unsafe fn from_ptr(ptr: *mut ()) -> Self {
        Self { ptr: ptr as _ }
    }
    #[inline(always)]
    pub const fn as_ptr(&self) -> *mut () {
        self.ptr as _
    }
    #[doc = "RW, GPIO PA I/O direction: 0=in, 1=out"]
    #[inline(always)]
    pub const fn dir(self) -> crate::common::Reg<regs::Dir, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0usize) as _) }
    }
    #[doc = "RO, GPIO PA input"]
    #[inline(always)]
    pub const fn pin(self) -> crate::common::Reg<regs::Pin, crate::common::R> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x04usize) as _) }
    }
    #[doc = "RW, GPIO PA output"]
    #[inline(always)]
    pub const fn out(self) -> crate::common::Reg<regs::Out, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x08usize) as _) }
    }
    #[doc = "WZ, GPIO PA clear output: 0=keep, 1=clear"]
    #[inline(always)]
    pub const fn clr(self) -> crate::common::Reg<regs::Clr, crate::common::W> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x0cusize) as _) }
    }
    #[doc = "RW, GPIO PA pullup resistance enable"]
    #[inline(always)]
    pub const fn pu(self) -> crate::common::Reg<regs::Pu, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x10usize) as _) }
    }
    #[doc = "RW, PA pulldown for input or PA driving capability for output"]
    #[inline(always)]
    pub const fn pd_drv(self) -> crate::common::Reg<regs::PdDrv, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x14usize) as _) }
    }
    #[doc = "RW, PA port output set register"]
    #[inline(always)]
    pub const fn set(self) -> crate::common::Reg<regs::Set, crate::common::RW> {
        unsafe { crate::common::Reg::from_ptr(self.ptr.add(0x18usize) as _) }
    }
}
pub mod regs {
    #[doc = "WZ, GPIO PA clear output: 0=keep, 1=clear"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Clr(pub u32);
    impl Clr {
        #[doc = "GPIO PA clear output byte 0"]
        #[inline(always)]
        pub const fn clr(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA clear output byte 0"]
        #[inline(always)]
        pub fn set_clr(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for Clr {
        #[inline(always)]
        fn default() -> Clr {
            Clr(0)
        }
    }
    #[doc = "RW, GPIO PA I/O direction: 0=in, 1=out"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Dir(pub u32);
    impl Dir {
        #[doc = "GPIO PA I/O direction byte 0"]
        #[inline(always)]
        pub const fn dir(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA I/O direction byte 0"]
        #[inline(always)]
        pub fn set_dir(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for Dir {
        #[inline(always)]
        fn default() -> Dir {
            Dir(0)
        }
    }
    #[doc = "RW, GPIO PA interrupt enable"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct IntEn(pub u16);
    impl IntEn {
        #[doc = "GPIO PA interrupt enable"]
        #[inline(always)]
        pub const fn ie(&self, n: usize) -> bool {
            assert!(n < 16usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA interrupt enable"]
        #[inline(always)]
        pub fn set_ie(&mut self, n: usize, val: bool) {
            assert!(n < 16usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u16) & 0x01) << offs);
        }
    }
    impl Default for IntEn {
        #[inline(always)]
        fn default() -> IntEn {
            IntEn(0)
        }
    }
    #[doc = "RW1, GPIO PA interrupt flag"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct IntIf(pub u16);
    impl IntIf {
        #[doc = "GPIO PA interrupt flag"]
        #[inline(always)]
        pub const fn if_(&self, n: usize) -> bool {
            assert!(n < 16usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA interrupt flag"]
        #[inline(always)]
        pub fn set_if_(&mut self, n: usize, val: bool) {
            assert!(n < 16usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u16) & 0x01) << offs);
        }
    }
    impl Default for IntIf {
        #[inline(always)]
        fn default() -> IntIf {
            IntIf(0)
        }
    }
    #[doc = "RW, GPIO PA interrupt mode: 0=level action, 1=edge action"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct IntMode(pub u16);
    impl IntMode {
        #[doc = "GPIO PA interrupt mode"]
        #[inline(always)]
        pub const fn edge(&self, n: usize) -> bool {
            assert!(n < 16usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA interrupt mode"]
        #[inline(always)]
        pub fn set_edge(&mut self, n: usize, val: bool) {
            assert!(n < 16usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u16) & 0x01) << offs);
        }
    }
    impl Default for IntMode {
        #[inline(always)]
        fn default() -> IntMode {
            IntMode(0)
        }
    }
    #[doc = "RW, GPIO PA output"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Out(pub u32);
    impl Out {
        #[doc = "GPIO PA output byte 0"]
        #[inline(always)]
        pub const fn out(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA output byte 0"]
        #[inline(always)]
        pub fn set_out(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for Out {
        #[inline(always)]
        fn default() -> Out {
            Out(0)
        }
    }
    #[doc = "RW, PA pulldown for input or PA driving capability for output"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct PdDrv(pub u32);
    impl PdDrv {
        #[doc = "PA pulldown for input or PA driving capability for output byte 0"]
        #[inline(always)]
        pub const fn pd_drv(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "PA pulldown for input or PA driving capability for output byte 0"]
        #[inline(always)]
        pub fn set_pd_drv(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for PdDrv {
        #[inline(always)]
        fn default() -> PdDrv {
            PdDrv(0)
        }
    }
    #[doc = "RW, GPIO PA input"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Pin(pub u32);
    impl Pin {
        #[doc = "GPIO PA input byte 0"]
        #[inline(always)]
        pub const fn pin(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA input byte 0"]
        #[inline(always)]
        pub fn set_pin(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for Pin {
        #[inline(always)]
        fn default() -> Pin {
            Pin(0)
        }
    }
    #[doc = "RW, GPIO PA pullup resistance enable"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Pu(pub u32);
    impl Pu {
        #[doc = "GPIO PA pullup resistance enable byte 0"]
        #[inline(always)]
        pub const fn pu(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "GPIO PA pullup resistance enable byte 0"]
        #[inline(always)]
        pub fn set_pu(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for Pu {
        #[inline(always)]
        fn default() -> Pu {
            Pu(0)
        }
    }
    #[doc = "RW, PA port output set register"]
    #[repr(transparent)]
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct Set(pub u32);
    impl Set {
        #[doc = "When the corresponding bit of the set register R3_2PA_SET is 0, the output of the PA pin remains unchanged; When it is 1, the PA pin outputs a high level"]
        #[inline(always)]
        pub const fn set(&self, n: usize) -> bool {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            let val = (self.0 >> offs) & 0x01;
            val != 0
        }
        #[doc = "When the corresponding bit of the set register R3_2PA_SET is 0, the output of the PA pin remains unchanged; When it is 1, the PA pin outputs a high level"]
        #[inline(always)]
        pub fn set_set(&mut self, n: usize, val: bool) {
            assert!(n < 24usize);
            let offs = 0usize + n * 1usize;
            self.0 = (self.0 & !(0x01 << offs)) | (((val as u32) & 0x01) << offs);
        }
    }
    impl Default for Set {
        #[inline(always)]
        fn default() -> Set {
            Set(0)
        }
    }
}
