//! USB full-speed device.
//!
//! CH585's USBFS is the WCH device controller, not the STM32 endpoint-table
//! peripheral. Each endpoint has its own DMA address, length and handshake
//! register, and the controller reads and writes the buffer in SRAM itself.
//! Buffers are 4-byte aligned and the DMA register takes the low 16 bits of the
//! address, which is how the vendor code programs it.
//!
//! On CH585 and CH592, the driver covers endpoints 0 to 7. Endpoint 4 has no DMA address of its
//! own; its OUT and IN buffers sit one and two packets after endpoint 0's. It
//! also ignores `AUTO_TOG` (the bit reads back zero), so the handler advances
//! its data toggles by hand. Endpoints 5 to 7 are enabled through `UEP567_MOD`
//! and have their own DMA registers. The host port is not implemented.
//!
//! `INT_BUSY` makes the serial engine answer NAK while a transfer completion is
//! pending. The handler NAKs the endpoint before clearing that flag, and a read
//! or write re-arms ACK when it starts. This does not establish exclusive CPU
//! ownership of the shared EP0 buffer; see the limitation below.
//!
//! A received packet only counts when `U_TOG_OK` says the data toggle matched
//! (datasheet §17.2: a packet whose toggle does not match is discarded). A
//! mismatched OUT packet is dropped without disturbing the queued read, so a
//! host retransmission cannot be mistaken for new data.
//!
//! Endpoint 0 toggles are manual: a SETUP leaves both at DATA1, each data packet
//! flips its direction, and the status stage stays DATA1. The other endpoints
//! use `AUTO_TOG` on CH585 and CH592, except endpoint 4, whose toggles the
//! handler advances. CH582 and CH583 use software toggles for endpoints 1–3.
//!
//! CH585 and CH592 use endpoints 0 to 7. CH582 and CH583 currently expose
//! only endpoints 0 to 3, with software toggles for non-control transfers.
//! USB2 is not exposed: the CH583 PAC lacks its clock-gate field. In addition,
//! masking CPU interrupts does not prevent a new SETUP from DMA-writing EP0
//! while software copies a previous packet. An old IN response can overwrite
//! that SETUP; safe packet handoff is unresolved. A SETUP can also be reported
//! alongside an earlier token: CH585 and CH592 currently do not process that
//! earlier token as required by their manuals. The USB driver is experimental.
//! CH585 control and CDC bulk transfers have been run on a board; CH582 and
//! CH583 have not been hardware-tested. USBHS is a different peripheral.

use core::cell::UnsafeCell;
use core::future::poll_fn;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver as driver;
use embassy_usb_driver::{
    Direction, EndpointAddress, EndpointAllocError, EndpointError, EndpointInfo, EndpointType, Event,
};

use crate::atomic::{compiler_fence, AtomicBool, AtomicU16, AtomicU32, Ordering};
use crate::interrupt::typelevel::Interrupt;
use crate::{interrupt, pac, peripherals, rcc, Peri, PeripheralType};

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let regs = T::regs();
        let flags = regs.int_fg().read();

        if flags.uif_bus_rst__uif_detect() {
            // A reset aborts every transfer and returns the address to zero.
            regs.dev_ad().write(|w| w.set_mask_usb_addr(0));
            T::state().ep0_reset_generation.fetch_add(1, Ordering::Relaxed);
            T::state().ep0_setup.store(false, Ordering::Relaxed);
            T::state().in_done[0].store(false, Ordering::Relaxed);
            T::state().out_done[0].store(false, Ordering::Relaxed);
            for index in 0..EP_COUNT {
                T::state().enabled[index].store(false, Ordering::Relaxed);
                T::state().ep_in_wakers[index].wake();
                T::state().ep_out_wakers[index].wake();
            }
            // Endpoint 0 answers the next SETUP, nothing else.
            set_response::<T>(0, Direction::Out, Response::Ack);
            set_response::<T>(0, Direction::In, Response::Nak);
            set_toggle::<T>(0, Direction::In, false);
            set_toggle::<T>(0, Direction::Out, false);
            T::state().irq_reset.store(true, Ordering::Relaxed);
            regs.int_fg().write(|w| {
                w.set_uif_bus_rst__uif_detect(true);
                w.set_uif_transfer(flags.uif_transfer());
            });
            T::state().bus_waker.wake();
        }

        if flags.uif_suspend() {
            // The same flag covers suspend and resume; the level says which.
            if regs.mis_st().read().ums_suspend() {
                T::state().irq_suspend.store(true, Ordering::Relaxed);
            } else {
                T::state().irq_resume.store(true, Ordering::Relaxed);
            }
            regs.int_fg().write(|w| w.set_uif_suspend(true));
            T::state().bus_waker.wake();
        }

        // A FIFO overflow drops whatever was queued. The endpoint state is
        // intact, so clearing the flag is enough to resume; without clearing it
        // the interrupt would stay asserted.
        if flags.uif_fifo_ov() {
            regs.int_fg().write(|w| w.set_uif_fifo_ov(true));
        }

        if flags.uif_transfer() && !flags.uif_bus_rst__uif_detect() {
            let status = regs.int_st().read();
            let endpoint = status.mask_uis_h_res__mask_uis_endp() as usize;
            let token = Token::from_bits(status.mask_uis_token());
            // SETUP is identified by its own status bit: the datasheet says a
            // SETUP does not update MASK_UIS_TOKEN, so the token field may still
            // hold the previous transaction. It also does not take part in the
            // toggle comparison.
            let setup = status.uis_setup_act();
            // A packet whose data toggle did not match does not belong to this
            // transfer and must be discarded (datasheet §17.2).
            let accepted = flags.u_tog_ok();
            // Datasheet section 17.2.1 requires acknowledging a pending token
            // before a simultaneously reported SETUP, then acknowledging SETUP.
            if endpoint < EP_COUNT {
                match token {
                    Token::In => {
                        // The toggle check applies to received data; an IN
                        // transaction that completes was ACKed by the host.
                        set_response::<T>(endpoint, Direction::In, Response::Nak);
                        if endpoint == 4 || (cfg!(usb_ch583) && endpoint != 0) {
                            flip_toggle::<T>(endpoint, Direction::In);
                        }
                        T::state().in_done[endpoint].store(true, Ordering::Relaxed);
                        T::state().ep_in_wakers[endpoint].wake();
                    }
                    Token::Out => {
                        if accepted {
                            T::state().out_len[endpoint].store(regs.rx_len().read().rx_len() as u16, Ordering::Relaxed);
                            set_response::<T>(endpoint, Direction::Out, Response::Nak);
                            if endpoint == 4 || (cfg!(usb_ch583) && endpoint != 0) {
                                flip_toggle::<T>(endpoint, Direction::Out);
                            }
                            T::state().out_done[endpoint].store(true, Ordering::Relaxed);
                            T::state().ep_out_wakers[endpoint].wake();
                        } else {
                            // A retransmitted packet: discard it and keep
                            // receiving.
                            set_response::<T>(endpoint, Direction::Out, Response::Ack);
                        }
                    }
                    Token::Idle | Token::Sof => {}
                }
            }
            if setup && token != Token::Idle {
                regs.int_fg().write(|w| w.set_uif_transfer(true));
            }
            if setup {
                // A SETUP can only target endpoint 0. The endpoint and token
                // fields of `INT_ST` are not updated by a SETUP, so they may
                // still hold the previous transaction (or the reset value
                // 0xF), and `UIS_SETUP_ACT` is the only reliable indicator.
                T::state().out_len[0].store(8, Ordering::Relaxed);
                T::state().in_done[0].store(false, Ordering::Relaxed);
                T::state().ep0_setup.store(true, Ordering::Relaxed);
                T::state().out_done[0].store(true, Ordering::Relaxed);
                set_response::<T>(0, Direction::In, Response::Nak);
                set_response::<T>(0, Direction::Out, Response::Nak);
                T::state().ep_out_wakers[0].wake();
                T::state().ep_in_wakers[0].wake();
            }
            // Clearing the flag lets the serial engine leave its busy NAK.
            regs.int_fg().write(|w| w.set_uif_transfer(true));
        }
    }
}

/// Token in `INT_ST.MASK_UIS_TOKEN`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    Out = 0,
    Sof = 1,
    In = 2,
    Idle = 3,
}

impl Token {
    fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            1 => Token::Sof,
            2 => Token::In,
            3 => Token::Idle,
            _ => Token::Out,
        }
    }
}

/// Handshake response in `UEPn_CTRL`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Response {
    Ack = 0b00,
    Nak = 0b10,
    Stall = 0b11,
}

#[cfg(not(usb_ch583))]
const EP_COUNT: usize = 8;
// Only endpoints 1–3 have documented automatic-toggle support on CH583.
#[cfg(usb_ch583)]
const EP_COUNT: usize = 4;
/// Largest packet this driver accepts. The length registers are 7 bits.
const MAX_PACKET: usize = 64;

const NEW_AW: AtomicWaker = AtomicWaker::new();

/// Bookkeeping and DMA storage belonging to one controller.
#[doc(hidden)]
pub struct State {
    allocated: [AtomicBool; EP_COUNT],
    enabled: [AtomicBool; EP_COUNT],
    in_done: [AtomicBool; EP_COUNT],
    out_done: [AtomicBool; EP_COUNT],
    out_len: [AtomicU16; EP_COUNT],
    bus_waker: AtomicWaker,
    ep0_setup: AtomicBool,
    ep0_reset_generation: AtomicU32,
    ep_in_wakers: [AtomicWaker; EP_COUNT],
    ep_out_wakers: [AtomicWaker; EP_COUNT],
    irq_reset: AtomicBool,
    irq_suspend: AtomicBool,
    irq_resume: AtomicBool,
    ep0_buffers: UnsafeCell<Ep0Buffers>,
    ep_buffer: UnsafeCell<[Buffer; 7]>,
}

// Each controller is exclusively owned; endpoint allocation prevents two
// endpoints from sharing a DMA buffer. EP0 access is protected by the driver.
unsafe impl Sync for State {}

impl State {
    const fn new() -> Self {
        Self {
            allocated: [const { AtomicBool::new(false) }; EP_COUNT],
            enabled: [const { AtomicBool::new(false) }; EP_COUNT],
            in_done: [const { AtomicBool::new(false) }; EP_COUNT],
            out_done: [const { AtomicBool::new(false) }; EP_COUNT],
            out_len: [const { AtomicU16::new(0) }; EP_COUNT],
            bus_waker: NEW_AW,
            ep0_setup: AtomicBool::new(false),
            ep0_reset_generation: AtomicU32::new(0),
            ep_in_wakers: [NEW_AW; EP_COUNT],
            ep_out_wakers: [NEW_AW; EP_COUNT],
            irq_reset: AtomicBool::new(false),
            irq_suspend: AtomicBool::new(false),
            irq_resume: AtomicBool::new(false),
            ep0_buffers: UnsafeCell::new(Ep0Buffers {
                control: Buffer::new(),
                ep4_out: Buffer::new(),
                ep4_in: Buffer::new(),
            }),
            ep_buffer: UnsafeCell::new([const { Buffer::new() }; 7]),
        }
    }
}

static USB_STATE: State = State::new();

/// Endpoint packet buffer. DMA reads and writes it, so access is volatile.
#[repr(C, align(4))]
struct Buffer {
    data: [u8; MAX_PACKET],
}

impl Buffer {
    const fn new() -> Self {
        Self { data: [0; MAX_PACKET] }
    }

    fn read(&self, out: &mut [u8]) {
        for (slot, byte) in out.iter_mut().enumerate() {
            *byte = unsafe { core::ptr::read_volatile(self.data.as_ptr().add(slot)) };
        }
    }

    fn write(&mut self, input: &[u8]) {
        for (slot, byte) in input.iter().enumerate() {
            unsafe { core::ptr::write_volatile(self.data.as_mut_ptr().add(slot), *byte) };
        }
    }

    /// The value the DMA register takes: the low 16 bits, 4-byte aligned.
    fn dma_address(&self) -> u16 {
        let address = self.data.as_ptr() as usize;
        debug_assert!(address % 4 == 0, "usb buffer is not 4-byte aligned");
        (address & 0xFFFF) as u16
    }
}

/// Endpoint 0's buffer, then endpoint 4's OUT and IN buffers. Endpoint 4 has
/// no DMA register and uses the two packets after endpoint 0's, so all three
/// must stay adjacent.
#[repr(C, align(4))]
struct Ep0Buffers {
    control: Buffer,
    ep4_out: Buffer,
    ep4_in: Buffer,
}

fn buffer<T: Instance>(index: usize, direction: Direction) -> &'static mut Buffer {
    let state = T::state();
    unsafe {
        match (index, direction) {
            (0, _) => &mut (*state.ep0_buffers.get()).control,
            (4, Direction::Out) => &mut (*state.ep0_buffers.get()).ep4_out,
            (4, Direction::In) => &mut (*state.ep0_buffers.get()).ep4_in,
            (i, _) => &mut *state.ep_buffer.get().cast::<Buffer>().add(i - 1),
        }
    }
}

/// USB full-speed device driver.
pub struct Driver<'d, T: Instance> {
    phantom: PhantomData<&'d mut T>,
    alloc_in: usize,
    alloc_out: usize,
}

impl<'d, T: Instance> Driver<'d, T> {
    /// Take the peripheral and leave it reset, with the pull-up off.
    ///
    /// [`Bus::enable`] connects the instance's USB pins and pull-up.
    pub fn new(
        _peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Self {
        for index in 0..EP_COUNT {
            T::state().allocated[index].store(false, Ordering::Relaxed);
            T::state().enabled[index].store(false, Ordering::Relaxed);
            T::state().in_done[index].store(false, Ordering::Relaxed);
            T::state().out_done[index].store(false, Ordering::Relaxed);
            T::state().out_len[index].store(0, Ordering::Relaxed);
        }
        T::state().ep0_setup.store(false, Ordering::Relaxed);
        T::state().irq_reset.store(false, Ordering::Relaxed);
        T::state().irq_suspend.store(false, Ordering::Relaxed);
        T::state().irq_resume.store(false, Ordering::Relaxed);

        // Leave the controller clock enabled after acquisition.
        rcc::with_safe_access(|| {
            pac::SYS.slp_clk_off1().modify(|w| w.set_slp_clk_usb(false));
        });

        let regs = T::regs();
        regs.ctrl().write(|w| {
            w.set_uc_reset_sie(true);
            w.set_uc_clr_all(true);
        });
        regs.ctrl().write(|w| w.set_uc_reset_sie(false));
        regs.dev_ad().write(|w| w.set_mask_usb_addr(0));
        regs.int_fg().write(|w| {
            w.set_uif_bus_rst__uif_detect(true);
            w.set_uif_transfer(true);
            w.set_uif_suspend(true);
            w.set_uif_fifo_ov(true);
        });

        Self {
            phantom: PhantomData,
            alloc_in: 1,
            alloc_out: 1,
        }
    }

    fn alloc(&mut self, direction: Direction, requested: Option<EndpointAddress>) -> Result<usize, EndpointAllocError> {
        match requested {
            Some(address) => {
                if address.direction() != direction {
                    return Err(EndpointAllocError);
                }
                let index = address.index();
                if index == 0 || index >= EP_COUNT || T::state().allocated[index].swap(true, Ordering::Relaxed) {
                    return Err(EndpointAllocError);
                }
                Ok(index)
            }
            None => {
                let cursor = match direction {
                    Direction::In => &mut self.alloc_in,
                    Direction::Out => &mut self.alloc_out,
                };
                let mut index = *cursor;
                while index < EP_COUNT && T::state().allocated[index].load(Ordering::Relaxed) {
                    index += 1;
                }
                if index >= EP_COUNT {
                    return Err(EndpointAllocError);
                }
                T::state().allocated[index].store(true, Ordering::Relaxed);
                *cursor = index + 1;
                Ok(index)
            }
        }
    }
}

impl<'d, T: Instance> driver::Driver<'d> for Driver<'d, T> {
    type EndpointOut = Endpoint<'d, T, Out>;
    type EndpointIn = Endpoint<'d, T, In>;
    type ControlPipe = ControlPipe<'d, T>;
    type Bus = Bus<'d, T>;

    fn alloc_endpoint_out(
        &mut self,
        ep_type: EndpointType,
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointOut, EndpointAllocError> {
        if ep_type == EndpointType::Control || max_packet_size as usize > MAX_PACKET {
            return Err(EndpointAllocError);
        }
        let index = self.alloc(Direction::Out, ep_addr)?;
        enable_out::<T>(index);
        Ok(Endpoint {
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(index, Direction::Out),
                ep_type,
                max_packet_size,
                interval_ms,
            },
            _phantom: PhantomData,
        })
    }

    fn alloc_endpoint_in(
        &mut self,
        ep_type: EndpointType,
        ep_addr: Option<EndpointAddress>,
        max_packet_size: u16,
        interval_ms: u8,
    ) -> Result<Self::EndpointIn, EndpointAllocError> {
        if ep_type == EndpointType::Control || max_packet_size as usize > MAX_PACKET {
            return Err(EndpointAllocError);
        }
        let index = self.alloc(Direction::In, ep_addr)?;
        enable_in::<T>(index);
        Ok(Endpoint {
            info: EndpointInfo {
                addr: EndpointAddress::from_parts(index, Direction::In),
                ep_type,
                max_packet_size,
                interval_ms,
            },
            _phantom: PhantomData,
        })
    }

    fn start(self, control_max_packet_size: u16) -> (Self::Bus, Self::ControlPipe) {
        assert!(
            control_max_packet_size as usize <= MAX_PACKET,
            "usb control packet larger than 64 bytes"
        );
        let regs = T::regs();
        let control = buffer::<T>(0, Direction::Out);
        regs.uep0_dma().write(|w| w.set_uep0_dma(control.dma_address()));
        regs.uep0_t_len().write(|w| w.set_uep0_t_len(0));
        // Answer SETUP, hold everything else.
        set_response::<T>(0, Direction::Out, Response::Ack);
        set_response::<T>(0, Direction::In, Response::Nak);

        (
            Bus {
                _phantom: PhantomData,
                inited: false,
            },
            ControlPipe {
                max_packet_size: control_max_packet_size as usize,
                reset_generation: T::state().ep0_reset_generation.load(Ordering::Relaxed),
                _phantom: PhantomData,
            },
        )
    }
}

/// Whole-bus control. Owned by the USB device task.
pub struct Bus<'d, T: Instance> {
    _phantom: PhantomData<&'d mut T>,
    inited: bool,
}

impl<'d, T: Instance> driver::Bus for Bus<'d, T> {
    async fn enable(&mut self) {
        let regs = T::regs();
        // Connect D+/D- and the device pull-up. `INT_BUSY` holds the bus in NAK
        // while a completion flag is set, and DMA moves the packets.
        T::set_pins(true);
        #[cfg(not(usb_ch583))]
        regs.udev_ctrl().write(|w| {
            w.set_ud_port_en(true);
            w.set_ud_pd_dis(true);
        });
        #[cfg(usb_ch583)]
        regs.udev_ctrl__uhost_ctrl().write(|w| {
            w.set_ud_port_en__uh_port_en(true);
            w.set_ud_pd_dis__uh_pd_dis(true);
        });
        regs.ctrl().write(|w| {
            w.set_uc_dma_en(true);
            w.set_uc_int_busy(true);
            w.set_uc_dev_pu_en(true);
        });
        regs.int_en().write(|w| {
            w.set_uie_bus_rst__uie_detect(true);
            w.set_uie_transfer(true);
            w.set_uie_suspend(true);
            w.set_uie_fifo_ov(true);
        });
        unsafe { T::Interrupt::enable() };
    }

    async fn disable(&mut self) {
        T::Interrupt::disable();
        T::state().ep0_reset_generation.fetch_add(1, Ordering::Relaxed);
        T::state().ep0_setup.store(false, Ordering::Relaxed);
        T::state().in_done[0].store(false, Ordering::Relaxed);
        T::state().out_done[0].store(false, Ordering::Relaxed);
        T::state().ep_in_wakers[0].wake();
        T::state().ep_out_wakers[0].wake();
        T::regs().ctrl().write(|w| w.set_uc_dev_pu_en(false));
        T::set_pins(false);
    }

    async fn poll(&mut self) -> Event {
        // This chip has no VBUS-detection input in the USBFS block. Treat the
        // bus-powered device as powered on the first poll: embassy-usb starts
        // in `Unpowered` and only calls `enable` after this event.
        if !self.inited {
            self.inited = true;
            return Event::PowerDetected;
        }

        poll_fn(|cx| {
            T::state().bus_waker.register(cx.waker());
            if T::state().irq_reset.swap(false, Ordering::Relaxed) {
                Poll::Ready(Event::Reset)
            } else if T::state().irq_suspend.swap(false, Ordering::Relaxed) {
                Poll::Ready(Event::Suspend)
            } else if T::state().irq_resume.swap(false, Ordering::Relaxed) {
                Poll::Ready(Event::Resume)
            } else {
                Poll::Pending
            }
        })
        .await
    }

    fn endpoint_set_enabled(&mut self, ep_addr: EndpointAddress, enabled: bool) {
        let index = ep_addr.index();
        if index == 0 || index >= EP_COUNT {
            return;
        }
        T::state().enabled[index].store(enabled, Ordering::Relaxed);
        if enabled {
            // A newly enabled endpoint starts at DATA0 with no stale completion,
            // and stays NAK until a read or write arms it. The DMA address and
            // the `UEPn_MOD` enable bits are rewritten here as well: a bus reset
            // or a `disable`/`enable` cycle may have dropped them, and embassy
            // re-enables the endpoints through this call after every reset.
            T::state().in_done[index].store(false, Ordering::Relaxed);
            T::state().out_done[index].store(false, Ordering::Relaxed);
            match ep_addr.direction() {
                Direction::In => enable_in::<T>(index),
                Direction::Out => enable_out::<T>(index),
            }
            set_toggle::<T>(index, Direction::In, false);
            set_toggle::<T>(index, Direction::Out, false);
            set_response::<T>(index, Direction::In, Response::Nak);
            set_response::<T>(index, Direction::Out, Response::Nak);
            T::state().ep_in_wakers[index].wake();
            T::state().ep_out_wakers[index].wake();
        } else {
            set_response::<T>(index, Direction::In, Response::Nak);
            set_response::<T>(index, Direction::Out, Response::Nak);
        }
    }

    fn endpoint_set_stalled(&mut self, ep_addr: EndpointAddress, stalled: bool) {
        let index = ep_addr.index();
        if stalled {
            set_response::<T>(index, ep_addr.direction(), Response::Stall);
        } else {
            // Clearing a stall also resets the data toggle. OUT is armed again;
            // IN waits for the next write.
            set_toggle::<T>(index, ep_addr.direction(), false);
            let response = match ep_addr.direction() {
                Direction::Out => Response::Ack,
                Direction::In => Response::Nak,
            };
            set_response::<T>(index, ep_addr.direction(), response);
        }
    }

    fn endpoint_is_stalled(&mut self, ep_addr: EndpointAddress) -> bool {
        endpoint_response::<T>(ep_addr.index(), ep_addr.direction()) == Response::Stall
    }

    async fn remote_wakeup(&mut self) -> Result<(), driver::Unsupported> {
        // There is no device remote-wakeup bit in this register block.
        Err(driver::Unsupported)
    }
}

/// Endpoint 0, both directions.
pub struct ControlPipe<'d, T: Instance> {
    max_packet_size: usize,
    reset_generation: u32,
    _phantom: PhantomData<&'d mut T>,
}

/// Wait for one endpoint 0 completion. A new SETUP abandons the stage.
async fn wait_ep0<T: Instance>(direction: Direction, reset_generation: u32) -> Result<(), EndpointError> {
    poll_fn(|cx| {
        match direction {
            Direction::In => T::state().ep_in_wakers[0].register(cx.waker()),
            Direction::Out => T::state().ep_out_wakers[0].register(cx.waker()),
        }
        critical_section::with(|_| {
            if T::state().ep0_setup.load(Ordering::Relaxed)
                || T::state().ep0_reset_generation.load(Ordering::Relaxed) != reset_generation
            {
                return Poll::Ready(Err(EndpointError::Disabled));
            }
            let done = match direction {
                Direction::In => T::state().in_done[0].swap(false, Ordering::Relaxed),
                Direction::Out => T::state().out_done[0].swap(false, Ordering::Relaxed),
            };
            if done {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
    })
    .await
}

impl<T: Instance> ControlPipe<'_, T> {
    async fn accept_status(&mut self) -> Result<(), EndpointError> {
        critical_section::with(|_| {
            if T::state().ep0_setup.load(Ordering::Relaxed)
                || T::state().ep0_reset_generation.load(Ordering::Relaxed) != self.reset_generation
            {
                return Err(EndpointError::Disabled);
            }
            T::state().in_done[0].store(false, Ordering::Relaxed);
            T::regs().uep0_t_len().write(|w| w.set_uep0_t_len(0));
            set_response::<T>(0, Direction::In, Response::Ack);
            Ok(())
        })?;
        wait_ep0::<T>(Direction::In, self.reset_generation).await
    }
}

impl<'d, T: Instance> driver::ControlPipe for ControlPipe<'d, T> {
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    async fn setup(&mut self) -> [u8; 8] {
        let (packet, generation) = poll_fn(|cx| {
            T::state().ep_out_wakers[0].register(cx.waker());
            critical_section::with(|_| {
                if T::state().ep0_setup.swap(false, Ordering::Relaxed) {
                    let mut packet = [0u8; 8];
                    buffer::<T>(0, Direction::Out).read(&mut packet);
                    T::state().out_done[0].store(false, Ordering::Relaxed);
                    // SETUP is DATA0; the data stage begins at DATA1.
                    set_toggle::<T>(0, Direction::In, true);
                    set_toggle::<T>(0, Direction::Out, true);
                    set_response::<T>(0, Direction::Out, Response::Nak);
                    set_response::<T>(0, Direction::In, Response::Nak);
                    Poll::Ready((packet, T::state().ep0_reset_generation.load(Ordering::Relaxed)))
                } else {
                    set_response::<T>(0, Direction::Out, Response::Ack);
                    Poll::Pending
                }
            })
        })
        .await;
        self.reset_generation = generation;
        packet
    }

    async fn data_out(&mut self, buf: &mut [u8], _first: bool, _last: bool) -> Result<usize, EndpointError> {
        poll_fn(|cx| {
            T::state().ep_out_wakers[0].register(cx.waker());
            // A packet that already completed is consumed as-is; arming ACK is
            // only for the next one. The critical section keeps the handler from
            // completing a packet, or a new SETUP, between the check and the arm.
            critical_section::with(|_| {
                if T::state().ep0_setup.load(Ordering::Relaxed)
                    || T::state().ep0_reset_generation.load(Ordering::Relaxed) != self.reset_generation
                {
                    Poll::Ready(Err(EndpointError::Disabled))
                } else if T::state().out_done[0].swap(false, Ordering::Relaxed) {
                    Poll::Ready(Ok(()))
                } else {
                    set_response::<T>(0, Direction::Out, Response::Ack);
                    Poll::Pending
                }
            })
        })
        .await?;

        critical_section::with(|_| {
            if T::state().ep0_setup.load(Ordering::Relaxed)
                || T::state().ep0_reset_generation.load(Ordering::Relaxed) != self.reset_generation
            {
                return Err(EndpointError::Disabled);
            }
            let len = (T::state().out_len[0].load(Ordering::Relaxed) as usize).min(buf.len());
            buffer::<T>(0, Direction::Out).read(&mut buf[..len]);
            flip_toggle::<T>(0, Direction::Out);
            Ok(len)
        })
    }

    async fn data_in(&mut self, data: &[u8], _first: bool, last: bool) -> Result<(), EndpointError> {
        if data.len() > self.max_packet_size {
            return Err(EndpointError::BufferOverflow);
        }
        critical_section::with(|_| {
            if T::state().ep0_setup.load(Ordering::Relaxed)
                || T::state().ep0_reset_generation.load(Ordering::Relaxed) != self.reset_generation
            {
                return Err(EndpointError::Disabled);
            }
            T::state().in_done[0].store(false, Ordering::Relaxed);
            buffer::<T>(0, Direction::In).write(data);
            compiler_fence(Ordering::SeqCst);
            T::regs().uep0_t_len().write(|w| w.set_uep0_t_len(data.len() as u8));
            set_response::<T>(0, Direction::In, Response::Ack);
            Ok(())
        })?;
        wait_ep0::<T>(Direction::In, self.reset_generation).await?;
        critical_section::with(|_| {
            if T::state().ep0_setup.load(Ordering::Relaxed)
                || T::state().ep0_reset_generation.load(Ordering::Relaxed) != self.reset_generation
            {
                return Err(EndpointError::Disabled);
            }
            flip_toggle::<T>(0, Direction::In);
            if last {
                // The status stage of a control read is an empty OUT, still DATA1.
                set_response::<T>(0, Direction::Out, Response::Ack);
            }
            Ok(())
        })?;
        if last {
            wait_ep0::<T>(Direction::Out, self.reset_generation).await?;
        }
        Ok(())
    }

    async fn accept(&mut self) {
        // The status stage of a control write is an empty IN.
        let _ = self.accept_status().await;
    }

    async fn reject(&mut self) {
        critical_section::with(|_| {
            if !T::state().ep0_setup.load(Ordering::Relaxed)
                && T::state().ep0_reset_generation.load(Ordering::Relaxed) == self.reset_generation
            {
                set_response::<T>(0, Direction::In, Response::Stall);
                set_response::<T>(0, Direction::Out, Response::Stall);
            }
        });
    }

    async fn accept_set_address(&mut self, addr: u8) {
        if self.accept_status().await.is_ok() {
            critical_section::with(|_| {
                if !T::state().ep0_setup.load(Ordering::Relaxed)
                    && T::state().ep0_reset_generation.load(Ordering::Relaxed) == self.reset_generation
                {
                    T::regs().dev_ad().write(|w| w.set_mask_usb_addr(addr));
                }
            });
        }
    }
}

/// Marker for the IN direction.
pub enum In {}
/// Marker for the OUT direction.
pub enum Out {}

/// One allocated endpoint.
pub struct Endpoint<'d, T: Instance, D> {
    info: EndpointInfo,
    _phantom: PhantomData<(&'d mut T, D)>,
}

impl<'d, T: Instance, D> driver::Endpoint for Endpoint<'d, T, D> {
    fn info(&self) -> &EndpointInfo {
        &self.info
    }

    async fn wait_enabled(&mut self) {
        poll_fn(|cx| {
            let index = self.info.addr.index();
            T::state().ep_in_wakers[index].register(cx.waker());
            T::state().ep_out_wakers[index].register(cx.waker());
            if T::state().enabled[index].load(Ordering::Relaxed) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

impl<'d, T: Instance> driver::EndpointOut for Endpoint<'d, T, Out> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, EndpointError> {
        let index = self.info.addr.index();
        if !T::state().enabled[index].load(Ordering::Relaxed) {
            return Err(EndpointError::Disabled);
        }
        poll_fn(|cx| {
            T::state().ep_out_wakers[index].register(cx.waker());
            critical_section::with(|_| {
                if !T::state().enabled[index].load(Ordering::Relaxed) {
                    Poll::Ready(Err(EndpointError::Disabled))
                } else if T::state().out_done[index].swap(false, Ordering::Relaxed) {
                    Poll::Ready(Ok(()))
                } else {
                    // Arm ACK only when there is no completed packet to consume.
                    set_response::<T>(index, Direction::Out, Response::Ack);
                    Poll::Pending
                }
            })
        })
        .await?;

        let len = T::state().out_len[index].load(Ordering::Relaxed) as usize;
        let len = len.min(buf.len()).min(self.info.max_packet_size as usize);
        buffer::<T>(index, Direction::Out).read(&mut buf[..len]);
        Ok(len)
    }
}

impl<'d, T: Instance> driver::EndpointIn for Endpoint<'d, T, In> {
    async fn write(&mut self, buf: &[u8]) -> Result<(), EndpointError> {
        let index = self.info.addr.index();
        if !T::state().enabled[index].load(Ordering::Relaxed) {
            return Err(EndpointError::Disabled);
        }
        if buf.len() > self.info.max_packet_size as usize {
            return Err(EndpointError::BufferOverflow);
        }
        T::state().in_done[index].store(false, Ordering::Relaxed);
        buffer::<T>(index, Direction::In).write(buf);
        compiler_fence(Ordering::SeqCst);
        set_tx_len::<T>(index, buf.len() as u8);
        set_response::<T>(index, Direction::In, Response::Ack);

        poll_fn(|cx| {
            T::state().ep_in_wakers[index].register(cx.waker());
            if !T::state().enabled[index].load(Ordering::Relaxed) {
                Poll::Ready(Err(EndpointError::Disabled))
            } else if T::state().in_done[index].swap(false, Ordering::Relaxed) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

// --- endpoint registers --------------------------------------------------------

// CH583 retains host/device aliases in the generated device registers.
#[cfg(not(usb_ch583))]
macro_rules! ep1_ctrl {
    ($regs:expr) => {
        $regs.uep1_ctrl()
    };
}
#[cfg(usb_ch583)]
macro_rules! ep1_ctrl {
    ($regs:expr) => {
        $regs.uep1_ctrl__uh_setup()
    };
}
#[cfg(not(usb_ch583))]
macro_rules! ep2_ctrl {
    ($regs:expr) => {
        $regs.uep2_ctrl()
    };
}
#[cfg(usb_ch583)]
macro_rules! ep2_ctrl {
    ($regs:expr) => {
        $regs.uep2_ctrl_uh_rx_ctrl()
    };
}
#[cfg(not(usb_ch583))]
macro_rules! ep3_ctrl {
    ($regs:expr) => {
        $regs.uep3_ctrl()
    };
}
#[cfg(usb_ch583)]
macro_rules! ep3_ctrl {
    ($regs:expr) => {
        $regs.uep3_ctrl__uh_tx_ctrl()
    };
}

#[cfg(not(usb_ch583))]
macro_rules! uep2_dma {
    ($regs:expr) => {
        $regs.uep2_dma()
    };
}
#[cfg(usb_ch583)]
macro_rules! uep2_dma {
    ($regs:expr) => {
        $regs.uep2_dma__uh_rx_dma()
    };
}

#[cfg(not(usb_ch583))]
macro_rules! uep3_dma {
    ($regs:expr) => {
        $regs.uep3_dma()
    };
}
#[cfg(usb_ch583)]
macro_rules! uep3_dma {
    ($regs:expr) => {
        $regs.uep3_dma__uh_tx_dma()
    };
}

#[cfg(not(usb_ch583))]
macro_rules! uep2_3_mod {
    ($regs:expr) => {
        $regs.uep2_3_mod()
    };
}
#[cfg(usb_ch583)]
macro_rules! uep2_3_mod {
    ($regs:expr) => {
        $regs.uep2_3_mod__uh_ep_mod()
    };
}

#[cfg(not(usb_ch583))]
macro_rules! uep2_t_len {
    ($regs:expr) => {
        $regs.uep2_t_len()
    };
}
#[cfg(usb_ch583)]
macro_rules! uep2_t_len {
    ($regs:expr) => {
        $regs.uep2_t_len_uh_ep_pid()
    };
}

#[cfg(not(usb_ch583))]
macro_rules! uep3_t_len {
    ($regs:expr) => {
        $regs.uep3_t_len()
    };
}
#[cfg(usb_ch583)]
macro_rules! uep3_t_len {
    ($regs:expr) => {
        $regs.uep3_t_len__uh_tx_len()
    };
}

#[cfg(not(usb_ch583))]
macro_rules! set_uep2_rx_en_field {
    ($w:expr, $v:expr) => {
        $w.set_uep2_rx_en($v)
    };
}
#[cfg(usb_ch583)]
macro_rules! set_uep2_rx_en_field {
    ($w:expr, $v:expr) => {
        $w.set_uep2_rx_en__uh_ep_rx_en($v)
    };
}

#[cfg(not(usb_ch583))]
macro_rules! set_uep3_tx_en_field {
    ($w:expr, $v:expr) => {
        $w.set_uep3_tx_en($v)
    };
}
#[cfg(usb_ch583)]
macro_rules! set_uep3_tx_en_field {
    ($w:expr, $v:expr) => {
        $w.set_uep3_tx_en__uh_ep_tx_en($v)
    };
}

#[cfg(not(usb_ch583))]
macro_rules! set_uep3_t_len_field {
    ($w:expr, $v:expr) => {
        $w.set_uep3_t_len($v)
    };
}
#[cfg(usb_ch583)]
macro_rules! set_uep3_t_len_field {
    ($w:expr, $v:expr) => {
        $w.set_uep3_t_len__uh_tx_len($v)
    };
}

// Each endpoint's control register is a distinct PAC type, even though the
// fields are identical. The macro repeats the body per endpoint instead of
// pretending they share a type.
macro_rules! modify_ctrl {
    ($regs:expr, $index:expr, |$w:ident| $body:expr) => {
        match $index {
            0 => $regs.uep0_ctrl().modify(|$w| $body),
            1 => ep1_ctrl!($regs).modify(|$w| $body),
            2 => ep2_ctrl!($regs).modify(|$w| $body),
            3 => ep3_ctrl!($regs).modify(|$w| $body),
            4 => $regs.uep4_ctrl().modify(|$w| $body),
            5 => $regs.uep5_ctrl().modify(|$w| $body),
            6 => $regs.uep6_ctrl().modify(|$w| $body),
            _ => $regs.uep7_ctrl().modify(|$w| $body),
        }
    };
}

macro_rules! read_ctrl {
    ($regs:expr, $index:expr, |$w:ident| $body:expr) => {
        match $index {
            0 => {
                let $w = $regs.uep0_ctrl().read();
                $body
            }
            1 => {
                let $w = ep1_ctrl!($regs).read();
                $body
            }
            2 => {
                let $w = ep2_ctrl!($regs).read();
                $body
            }
            3 => {
                let $w = ep3_ctrl!($regs).read();
                $body
            }
            4 => {
                let $w = $regs.uep4_ctrl().read();
                $body
            }
            5 => {
                let $w = $regs.uep5_ctrl().read();
                $body
            }
            6 => {
                let $w = $regs.uep6_ctrl().read();
                $body
            }
            _ => {
                let $w = $regs.uep7_ctrl().read();
                $body
            }
        }
    };
}

fn set_response<T: Instance>(index: usize, direction: Direction, response: Response) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| {
        match direction {
            Direction::In => w.set_mask_uep_t_res(response as u8),
            Direction::Out => w.set_mask_uep_r_res(response as u8),
        }
    });
}

fn endpoint_response<T: Instance>(index: usize, direction: Direction) -> Response {
    let regs = T::regs();
    let bits = read_ctrl!(regs, index, |w| {
        match direction {
            Direction::In => w.mask_uep_t_res(),
            Direction::Out => w.mask_uep_r_res(),
        }
    });
    match bits {
        0b00 => Response::Ack,
        0b11 => Response::Stall,
        _ => Response::Nak,
    }
}

#[cfg(not(usb_ch583))]
fn set_toggle<T: Instance>(index: usize, direction: Direction, data1: bool) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| {
        match direction {
            Direction::In => w.set_uep_t_tog(data1),
            Direction::Out => w.set_uep_r_tog(data1),
        }
    });
}

#[cfg(not(usb_ch583))]
fn flip_toggle<T: Instance>(index: usize, direction: Direction) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| {
        match direction {
            Direction::In => w.set_uep_t_tog(!w.uep_t_tog()),
            Direction::Out => w.set_uep_r_tog(!w.uep_r_tog()),
        }
    });
}

#[cfg(usb_ch583)]
fn set_toggle<T: Instance>(index: usize, direction: Direction, data1: bool) {
    let regs = T::regs();
    match index {
        0 => regs.uep0_ctrl().modify(|w| match direction {
            Direction::In => w.set_uep_t_tog(data1),
            Direction::Out => w.set_uep_r_tog(data1),
        }),
        1 => regs.uep1_ctrl__uh_setup().modify(|w| match direction {
            Direction::In => w.set_uep_t_tog__uh_sof_en(data1),
            Direction::Out => w.set_uep_r_tog__uh_pre_pid_en(data1),
        }),
        2 => regs.uep2_ctrl_uh_rx_ctrl().modify(|w| match direction {
            Direction::In => w.set_uep_t_tog(data1),
            Direction::Out => w.set_uep_r_tog__uh_r_tog(data1),
        }),
        3 => regs.uep3_ctrl__uh_tx_ctrl().modify(|w| match direction {
            Direction::In => w.set_uep_t_tog_rb_uh_t_tog(data1),
            Direction::Out => w.set_uep_r_tog(data1),
        }),
        _ => unreachable!(),
    }
}

#[cfg(usb_ch583)]
fn flip_toggle<T: Instance>(index: usize, direction: Direction) {
    let regs = T::regs();
    let data1 = match index {
        0 => {
            let w = regs.uep0_ctrl().read();
            match direction {
                Direction::In => w.uep_t_tog(),
                Direction::Out => w.uep_r_tog(),
            }
        }
        1 => {
            let w = regs.uep1_ctrl__uh_setup().read();
            match direction {
                Direction::In => w.uep_t_tog__uh_sof_en(),
                Direction::Out => w.uep_r_tog__uh_pre_pid_en(),
            }
        }
        2 => {
            let w = regs.uep2_ctrl_uh_rx_ctrl().read();
            match direction {
                Direction::In => w.uep_t_tog(),
                Direction::Out => w.uep_r_tog__uh_r_tog(),
            }
        }
        3 => {
            let w = regs.uep3_ctrl__uh_tx_ctrl().read();
            match direction {
                Direction::In => w.uep_t_tog_rb_uh_t_tog(),
                Direction::Out => w.uep_r_tog(),
            }
        }
        _ => unreachable!(),
    };
    set_toggle::<T>(index, direction, !data1);
}

fn set_tx_len<T: Instance>(index: usize, len: u8) {
    let regs = T::regs();
    match index {
        0 => regs.uep0_t_len().write(|w| w.set_uep0_t_len(len)),
        1 => regs.uep1_t_len().write(|w| w.set_uep1_t_len(len)),
        2 => uep2_t_len!(regs).write(|w| w.set_uep2_t_len(len)),
        3 => uep3_t_len!(regs).write(|w| set_uep3_t_len_field!(w, len)),
        4 => regs.uep4_t_len().write(|w| w.set_uep4_t_len(len)),
        5 => regs.uep5_t_len().write(|w| w.set_uep5_t_len(len)),
        6 => regs.uep6_t_len().write(|w| w.set_uep6_t_len(len)),
        _ => regs.uep7_t_len().write(|w| w.set_uep7_t_len(len)),
    }
}

fn enable_in<T: Instance>(index: usize) {
    let regs = T::regs();
    let dma = buffer::<T>(index, Direction::In).dma_address();
    match index {
        1 => {
            regs.uep1_dma().write(|w| w.set_uep1_dma(dma));
            regs.uep4_1_mod().modify(|w| w.set_uep1_tx_en(true));
        }
        2 => {
            uep2_dma!(regs).write(|w| w.set_uep2_dma(dma));
            uep2_3_mod!(regs).modify(|w| w.set_uep2_tx_en(true));
        }
        3 => {
            uep3_dma!(regs).write(|w| w.set_uep3_dma(dma));
            uep2_3_mod!(regs).modify(|w| set_uep3_tx_en_field!(w, true));
        }
        4 => {
            // Endpoint 4's buffer follows endpoint 0's, so there is no DMA
            // register to write here.
            regs.uep4_1_mod().modify(|w| w.set_uep4_tx_en(true));
        }
        5 => {
            regs.uep5_dma().write(|w| w.set_uep5_dma(dma));
            regs.uep567_mod().modify(|w| w.set_uep5_tx_en(true));
        }
        6 => {
            regs.uep6_dma().write(|w| w.set_uep6_dma(dma));
            regs.uep567_mod().modify(|w| w.set_uep6_tx_en(true));
        }
        _ => {
            regs.uep7_dma().write(|w| w.set_uep7_dma(dma));
            regs.uep567_mod().modify(|w| w.set_uep7_tx_en(true));
        }
    }
    // Endpoint 4 does not implement the automatic toggle: `UEP4_CTRL.AUTO_TOG`
    // writes are dropped and read back as zero. Its toggle starts at DATA0 and
    // the interrupt handler advances it after each transfer.
    if index == 4 || cfg!(usb_ch583) {
        #[cfg(usb_ch583)]
        auto_toggle::<T>(index);
        set_toggle::<T>(index, Direction::In, false);
    } else {
        auto_toggle::<T>(index);
    }
    set_response::<T>(index, Direction::In, Response::Nak);
}

fn enable_out<T: Instance>(index: usize) {
    let regs = T::regs();
    let dma = buffer::<T>(index, Direction::Out).dma_address();
    match index {
        1 => {
            regs.uep1_dma().write(|w| w.set_uep1_dma(dma));
            regs.uep4_1_mod().modify(|w| w.set_uep1_rx_en(true));
        }
        2 => {
            uep2_dma!(regs).write(|w| w.set_uep2_dma(dma));
            uep2_3_mod!(regs).modify(|w| set_uep2_rx_en_field!(w, true));
        }
        3 => {
            uep3_dma!(regs).write(|w| w.set_uep3_dma(dma));
            uep2_3_mod!(regs).modify(|w| w.set_uep3_rx_en(true));
        }
        4 => {
            regs.uep4_1_mod().modify(|w| w.set_uep4_rx_en(true));
        }
        5 => {
            regs.uep5_dma().write(|w| w.set_uep5_dma(dma));
            regs.uep567_mod().modify(|w| w.set_uep5_rx_en(true));
        }
        6 => {
            regs.uep6_dma().write(|w| w.set_uep6_dma(dma));
            regs.uep567_mod().modify(|w| w.set_uep6_rx_en(true));
        }
        _ => {
            regs.uep7_dma().write(|w| w.set_uep7_dma(dma));
            regs.uep567_mod().modify(|w| w.set_uep7_rx_en(true));
        }
    }
    // See the note in `enable_in`: endpoint 4 toggles by hand.
    if index == 4 || cfg!(usb_ch583) {
        #[cfg(usb_ch583)]
        auto_toggle::<T>(index);
        set_toggle::<T>(index, Direction::Out, false);
    } else {
        auto_toggle::<T>(index);
    }
    set_response::<T>(index, Direction::Out, Response::Nak);
}

#[cfg(not(usb_ch583))]
fn auto_toggle<T: Instance>(index: usize) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| w.set_uep_auto_tog(true));
}

#[cfg(usb_ch583)]
fn auto_toggle<T: Instance>(index: usize) {
    let regs = T::regs();
    match index {
        1 => regs.uep1_ctrl__uh_setup().modify(|w| w.set_uep_auto_tog(false)),
        2 => regs
            .uep2_ctrl_uh_rx_ctrl()
            .modify(|w| w.set_uep_auto_tog__uh_r_auto_tog(false)),
        3 => regs
            .uep3_ctrl__uh_tx_ctrl()
            .modify(|w| w.set_uep_auto_tog_rb_uh_t_auto_tog(false)),
        _ => unreachable!(),
    }
}

// --- instance ------------------------------------------------------------------

mod sealed {
    use super::pac;

    #[cfg(usbfs_ch585)]
    pub type Regs = pac::usbfs::Usbfs;
    #[cfg(any(usb_ch592, usb_ch583))]
    pub type Regs = pac::usb::Usb;

    pub trait Instance {
        type Interrupt: crate::interrupt::typelevel::Interrupt;

        fn regs() -> Regs;
        fn state() -> &'static super::State;
        fn set_pins(enabled: bool);
    }
}

/// A USB full-speed device peripheral.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {}

#[cfg(usbfs_ch585)]
impl sealed::Instance for peripherals::USBFS {
    type Interrupt = interrupt::typelevel::USB;

    fn regs() -> pac::usbfs::Usbfs {
        pac::USBFS
    }

    fn state() -> &'static State {
        &USB_STATE
    }

    fn set_pins(enabled: bool) {
        critical_section::with(|_| {
            pac::SYS.pin_config().modify(|w| {
                w.set_pin_usb_en(enabled);
                w.set_udp_pu_en(enabled);
            })
        });
    }
}

#[cfg(usbfs_ch585)]
impl Instance for peripherals::USBFS {}

// CH592 has one USB block and no high-speed controller. The register block is
// the device-mode part of the CH585 USBFS layout, recorded as kind `usb`.
#[cfg(usb_ch592)]
impl sealed::Instance for peripherals::USB {
    type Interrupt = interrupt::typelevel::USB;

    fn regs() -> pac::usb::Usb {
        pac::USB
    }

    fn state() -> &'static State {
        &USB_STATE
    }

    fn set_pins(enabled: bool) {
        critical_section::with(|_| {
            pac::SYS.pin_analog_ie().modify(|w| {
                w.set_pin_usb_ie(enabled);
                w.set_pin_usb_dp_pu(enabled);
            })
        });
    }
}

#[cfg(usb_ch592)]
impl Instance for peripherals::USB {}

#[cfg(usb_ch583)]
impl sealed::Instance for peripherals::USB {
    type Interrupt = interrupt::typelevel::USB;

    fn regs() -> pac::usb::Usb {
        pac::USB
    }

    fn state() -> &'static State {
        &USB_STATE
    }

    fn set_pins(enabled: bool) {
        critical_section::with(|_| {
            pac::SYS.pin_analog_ie().modify(|w| {
                w.set_pin_usb_ie(enabled);
                w.set_pin_usb_dp_pu(enabled);
            })
        });
    }
}

#[cfg(usb_ch583)]
impl Instance for peripherals::USB {}

// USB2 needs its own clock-gate field in the generated CH583 PAC before it
// can be exposed. See CH583 datasheet section 5.2, SLP_CLK_OFF1 bit 5.

#[cfg(all(test, usb_ch583))]
mod tests {
    use super::*;

    #[test]
    fn controllers_have_independent_state_and_dma() {
        let first = State::new();
        let second = State::new();
        assert!(!core::ptr::eq(&first, &second));
        assert_ne!(first.ep0_buffers.get(), second.ep0_buffers.get());
        assert_ne!(first.ep_buffer.get(), second.ep_buffer.get());
        assert!(!core::ptr::eq(&first.bus_waker, &second.bus_waker));
        assert!(!core::ptr::eq(&first.ep_in_wakers[0], &second.ep_in_wakers[0]));
        assert!(!core::ptr::eq(&first.ep_out_wakers[0], &second.ep_out_wakers[0]));
    }

    #[test]
    fn controller_events_do_not_cross() {
        let first = State::new();
        let second = State::new();
        first.irq_reset.store(true, Ordering::Relaxed);
        first.ep0_reset_generation.fetch_add(1, Ordering::Relaxed);
        first.in_done[1].store(true, Ordering::Relaxed);
        assert!(!second.irq_reset.load(Ordering::Relaxed));
        assert_eq!(second.ep0_reset_generation.load(Ordering::Relaxed), 0);
        assert!(!second.in_done[1].load(Ordering::Relaxed));
    }

    #[test]
    fn ep0_and_ep4_dma_layout_is_preserved() {
        assert_eq!(core::mem::align_of::<Ep0Buffers>(), 4);
        assert_eq!(core::mem::offset_of!(Ep0Buffers, ep4_out), MAX_PACKET);
        assert_eq!(core::mem::offset_of!(Ep0Buffers, ep4_in), 2 * MAX_PACKET);
    }
}
