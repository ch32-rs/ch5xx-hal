use crate::pac::common::{Reg, RW};

/// Indexed device registers, using the PAC's addresses and endpoint arrays.
#[derive(Clone, Copy)]
pub(super) struct Registers(pub crate::pac::usbhs::Usbhs);

impl Registers {
    /// The transfer state machine uses the hardware's raw response codes and
    /// done flags. Restate each PAC register at its hardware width so the same
    /// code handles EP0 and the EP1-7 arrays without numeric addresses.
    fn reg<T: Copy>(ptr: *mut T) -> Reg<T, RW> {
        unsafe { Reg::from_ptr(ptr) }
    }

    pub fn enables(self, incoming: bool) -> Reg<u16, RW> {
        let dev = self.0;
        let ptr: *mut u16 = if incoming {
            dev.uep_tx_en().as_ptr().cast()
        } else {
            dev.uep_rx_en().as_ptr().cast()
        };
        Self::reg(ptr)
    }

    pub fn control(self, index: usize, incoming: bool) -> Reg<u8, RW> {
        let dev = self.0;
        let ptr: *mut u8 = match (index, incoming) {
            (0, true) => dev.uep0_tx_ctrl().as_ptr().cast(),
            (0, false) => dev.uep0_rx_ctrl().as_ptr().cast(),
            (n, true) => dev.uep_tx_ctrl(n - 1).as_ptr().cast(),
            (n, false) => dev.uep_rx_ctrl(n - 1).as_ptr().cast(),
        };
        Self::reg(ptr)
    }

    pub fn dma(self, index: usize, incoming: bool) -> Reg<u32, RW> {
        let dev = self.0;
        let ptr: *mut u32 = match (index, incoming) {
            (0, _) => dev.uep0_dma().as_ptr().cast(),
            (n, true) => dev.uep_tx_dma(n - 1).as_ptr().cast(),
            (n, false) => dev.uep_rx_dma(n - 1).as_ptr().cast(),
        };
        Self::reg(ptr)
    }

    pub fn max_length(self, index: usize) -> Reg<u32, RW> {
        let dev = self.0;
        let ptr: *mut u32 = if index == 0 {
            dev.uep0_max_len().as_ptr().cast()
        } else {
            dev.uep_max_len(index - 1).as_ptr().cast()
        };
        Self::reg(ptr)
    }

    pub fn tx_length(self, index: usize) -> Reg<u16, RW> {
        let dev = self.0;
        let ptr: *mut u16 = if index == 0 {
            dev.uep0_t_len().as_ptr().cast()
        } else {
            dev.uep_t_len(index - 1).as_ptr().cast()
        };
        Self::reg(ptr)
    }

    pub fn rx_length(self, index: usize) -> Reg<u16, RW> {
        let dev = self.0;
        let ptr: *mut u16 = if index == 0 {
            dev.uep0_rx_len().as_ptr().cast()
        } else {
            dev.uep_rx_len(index - 1).as_ptr().cast()
        };
        Self::reg(ptr)
    }

    pub fn iso(self, incoming: bool) -> Reg<u16, RW> {
        let dev = self.0;
        let ptr: *mut u16 = if incoming {
            dev.uep_t_iso().as_ptr().cast()
        } else {
            dev.uep_r_iso().as_ptr().cast()
        };
        Self::reg(ptr)
    }

    pub fn initialize(self) {
        let dev = self.0;
        self.enables(true).write_value(0);
        self.enables(false).write_value(0);
        Self::reg(dev.uep_t_tog_auto().as_ptr().cast::<u16>()).write_value(0);
        Self::reg(dev.uep_r_tog_auto().as_ptr().cast::<u16>()).write_value(0);
        Self::reg(dev.uep_t_burst().as_ptr().cast::<u8>()).write_value(0);
        Self::reg(dev.uep_t_burst_mode().as_ptr().cast::<u8>()).write_value(0);
        Self::reg(dev.uep_r_burst().as_ptr().cast::<u8>()).write_value(0);
        Self::reg(dev.uep_r_res_mode().as_ptr().cast::<u8>()).write_value(0);
        Self::reg(dev.uep_af_mode().as_ptr().cast::<u32>()).write_value(0);
        self.iso(true).write_value(0);
        self.iso(false).write_value(0);
        for index in 0..8 {
            self.control(index, true).write_value(0);
            self.control(index, false).write_value(0);
            self.tx_length(index).write_value(0);
        }
    }
}
