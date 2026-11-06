//! USB full-speed device.
//!
//! CH585's USBFS is the WCH device controller, not the STM32 endpoint-table
//! peripheral. Each endpoint has its own DMA address, length and handshake
//! register, and the controller reads and writes the buffer in SRAM itself.
//! Buffers are 4-byte aligned and the DMA register takes the low 16 bits of the
//! address, which is how the vendor code programs it.
//!
//! This driver covers endpoint 0 and endpoints 1 to 4. Endpoint 4 has no DMA
//! address of its own; its buffer sits one packet after endpoint 0's. Endpoints
//! 5 to 7 and the host port are not implemented.
//!
//! `INT_BUSY` makes the serial engine answer NAK while a transfer completion is
//! pending. The handler clears that flag and NAKs the endpoint, so no new packet
//! can overwrite the buffer before the future has copied it. A read or write
//! re-arms ACK when it starts.
//!
//! Endpoint 0 toggles are manual: a SETUP leaves both at DATA1, each data packet
//! flips its direction, and the status stage stays DATA1. The other endpoints
//! use `AUTO_TOG`.
//!
//! The register block is the CH585 one (CH572, CH581, CH584, CH585, CH595).
//! This driver has not yet been run on a board. USBHS is a different peripheral.

use core::future::poll_fn;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_sync::waitqueue::AtomicWaker;
use embassy_usb_driver as driver;
use embassy_usb_driver::{
    Direction, EndpointAddress, EndpointAllocError, EndpointError, EndpointInfo, EndpointType, Event,
};

use crate::atomic::{compiler_fence, AtomicBool, AtomicU16, Ordering};
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
            for index in 0..EP_COUNT {
                STATE.enabled[index].store(false, Ordering::Relaxed);
                EP_IN_WAKERS[index].wake();
                EP_OUT_WAKERS[index].wake();
            }
            // Endpoint 0 answers the next SETUP, nothing else.
            set_response::<T>(0, Direction::Out, Response::Ack);
            set_response::<T>(0, Direction::In, Response::Nak);
            set_toggle::<T>(0, Direction::In, false);
            set_toggle::<T>(0, Direction::Out, false);
            IRQ_RESET.store(true, Ordering::Relaxed);
            regs.int_fg().write(|w| w.set_uif_bus_rst__uif_detect(true));
            BUS_WAKER.wake();
        }

        if flags.uif_suspend() {
            // The same flag covers suspend and resume; the level says which.
            if regs.mis_st().read().ums_suspend() {
                IRQ_SUSPEND.store(true, Ordering::Relaxed);
            } else {
                IRQ_RESUME.store(true, Ordering::Relaxed);
            }
            regs.int_fg().write(|w| w.set_uif_suspend(true));
            BUS_WAKER.wake();
        }

        if flags.uif_transfer() {
            let status = regs.int_st().read();
            let endpoint = status.mask_uis_h_res__mask_uis_endp() as usize;
            let token = Token::from_bits(status.mask_uis_token());
            if endpoint < EP_COUNT {
                match token {
                    Token::In => {
                        // NAK until the next packet is queued.
                        set_response::<T>(endpoint, Direction::In, Response::Nak);
                        STATE.in_done[endpoint].store(true, Ordering::Relaxed);
                        EP_IN_WAKERS[endpoint].wake();
                    }
                    Token::Out | Token::Setup => {
                        STATE.out_len[endpoint].store(regs.rx_len().read().rx_len() as u16, Ordering::Relaxed);
                        set_response::<T>(endpoint, Direction::Out, Response::Nak);
                        if endpoint == 0 && token == Token::Setup {
                            EP0_SETUP.store(true, Ordering::Relaxed);
                        }
                        STATE.out_done[endpoint].store(true, Ordering::Relaxed);
                        EP_OUT_WAKERS[endpoint].wake();
                    }
                    Token::Sof => {}
                }
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
    Setup = 3,
}

impl Token {
    fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            1 => Token::Sof,
            2 => Token::In,
            3 => Token::Setup,
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

const EP_COUNT: usize = 5;
/// Largest packet this driver accepts. The length registers are 7 bits.
const MAX_PACKET: usize = 64;

const NEW_AW: AtomicWaker = AtomicWaker::new();
static BUS_WAKER: AtomicWaker = NEW_AW;
static EP0_SETUP: AtomicBool = AtomicBool::new(false);
static EP_IN_WAKERS: [AtomicWaker; EP_COUNT] = [NEW_AW; EP_COUNT];
static EP_OUT_WAKERS: [AtomicWaker; EP_COUNT] = [NEW_AW; EP_COUNT];
static IRQ_RESET: AtomicBool = AtomicBool::new(false);
static IRQ_SUSPEND: AtomicBool = AtomicBool::new(false);
static IRQ_RESUME: AtomicBool = AtomicBool::new(false);

/// Endpoint bookkeeping shared with the interrupt handler.
struct State {
    allocated: [AtomicBool; EP_COUNT],
    enabled: [AtomicBool; EP_COUNT],
    in_done: [AtomicBool; EP_COUNT],
    out_done: [AtomicBool; EP_COUNT],
    out_len: [AtomicU16; EP_COUNT],
}

static STATE: State = State {
    allocated: [const { AtomicBool::new(false) }; EP_COUNT],
    enabled: [const { AtomicBool::new(false) }; EP_COUNT],
    in_done: [const { AtomicBool::new(false) }; EP_COUNT],
    out_done: [const { AtomicBool::new(false) }; EP_COUNT],
    out_len: [const { AtomicU16::new(0) }; EP_COUNT],
};

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

/// Endpoint 0's buffer, then endpoint 4's. Endpoint 4 has no DMA register and
/// uses the packet after endpoint 0's, so the two must stay adjacent.
#[repr(C, align(4))]
struct Ep0Buffers {
    control: Buffer,
    ep4: Buffer,
}

static mut EP0_BUFFERS: Ep0Buffers = Ep0Buffers {
    control: Buffer::new(),
    ep4: Buffer::new(),
};
static mut EP_BUFFER: [Buffer; 4] = [const { Buffer::new() }; 4];

fn buffer(index: usize) -> &'static mut Buffer {
    unsafe {
        match index {
            0 => &mut *core::ptr::addr_of_mut!(EP0_BUFFERS.control),
            4 => &mut *core::ptr::addr_of_mut!(EP0_BUFFERS.ep4),
            i => &mut *core::ptr::addr_of_mut!(EP_BUFFER[i]),
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
    /// [`Bus::enable`] connects the pull-up. The pins are PB11 (D+) and PB10
    /// (D-); `PIN_USB_EN` connects them, so they are not GPIO.
    pub fn new(
        _peri: Peri<'d, T>,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Self {
        for index in 0..EP_COUNT {
            STATE.allocated[index].store(false, Ordering::Relaxed);
            STATE.enabled[index].store(false, Ordering::Relaxed);
            STATE.in_done[index].store(false, Ordering::Relaxed);
            STATE.out_done[index].store(false, Ordering::Relaxed);
            STATE.out_len[index].store(0, Ordering::Relaxed);
        }
        EP0_SETUP.store(false, Ordering::Relaxed);
        IRQ_RESET.store(false, Ordering::Relaxed);
        IRQ_SUSPEND.store(false, Ordering::Relaxed);
        IRQ_RESUME.store(false, Ordering::Relaxed);

        // The USB clock is gated in the safe-access sleep-clock byte.
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
                if index == 0 || index >= EP_COUNT || STATE.allocated[index].swap(true, Ordering::Relaxed) {
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
                while index < EP_COUNT && STATE.allocated[index].load(Ordering::Relaxed) {
                    index += 1;
                }
                if index >= EP_COUNT {
                    return Err(EndpointAllocError);
                }
                STATE.allocated[index].store(true, Ordering::Relaxed);
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
        let control = buffer(0);
        regs.uep0_dma().write(|w| w.set_uep0_dma(control.dma_address()));
        regs.uep0_t_len().write(|w| w.set_uep0_t_len(0));
        // Answer SETUP, hold everything else.
        set_response::<T>(0, Direction::Out, Response::Ack);
        set_response::<T>(0, Direction::In, Response::Nak);

        (
            Bus { _phantom: PhantomData },
            ControlPipe {
                max_packet_size: control_max_packet_size as usize,
                _phantom: PhantomData,
            },
        )
    }
}

/// Whole-bus control. Owned by the USB device task.
pub struct Bus<'d, T: Instance> {
    _phantom: PhantomData<&'d mut T>,
}

impl<'d, T: Instance> driver::Bus for Bus<'d, T> {
    async fn enable(&mut self) {
        let regs = T::regs();
        // Connect D+/D- and the device pull-up. `INT_BUSY` holds the bus in NAK
        // while a completion flag is set, and DMA moves the packets.
        pac::SYS.pin_config().modify(|w| {
            w.set_pin_usb_en(true);
            w.set_udp_pu_en(true);
        });
        regs.udev_ctrl().write(|w| {
            w.set_ud_port_en(true);
            w.set_ud_pd_dis(true);
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
        });
        unsafe { T::Interrupt::enable() };
    }

    async fn disable(&mut self) {
        T::Interrupt::disable();
        T::regs().ctrl().write(|w| w.set_uc_dev_pu_en(false));
        pac::SYS.pin_config().modify(|w| {
            w.set_udp_pu_en(false);
            w.set_pin_usb_en(false);
        });
    }

    async fn poll(&mut self) -> Event {
        poll_fn(|cx| {
            BUS_WAKER.register(cx.waker());
            if IRQ_RESET.swap(false, Ordering::Relaxed) {
                Poll::Ready(Event::Reset)
            } else if IRQ_SUSPEND.swap(false, Ordering::Relaxed) {
                Poll::Ready(Event::Suspend)
            } else if IRQ_RESUME.swap(false, Ordering::Relaxed) {
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
        STATE.enabled[index].store(enabled, Ordering::Relaxed);
        if enabled {
            // A newly enabled endpoint starts at DATA0 with no stale completion,
            // and stays NAK until a read or write arms it.
            STATE.in_done[index].store(false, Ordering::Relaxed);
            STATE.out_done[index].store(false, Ordering::Relaxed);
            set_toggle::<T>(index, Direction::In, false);
            set_toggle::<T>(index, Direction::Out, false);
            set_response::<T>(index, Direction::In, Response::Nak);
            set_response::<T>(index, Direction::Out, Response::Nak);
            EP_IN_WAKERS[index].wake();
            EP_OUT_WAKERS[index].wake();
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
    _phantom: PhantomData<&'d mut T>,
}

/// Wait for one endpoint 0 completion. A new SETUP abandons the stage.
async fn wait_ep0<T: Instance>(direction: Direction) -> Result<(), EndpointError> {
    poll_fn(|cx| {
        match direction {
            Direction::In => EP_IN_WAKERS[0].register(cx.waker()),
            Direction::Out => EP_OUT_WAKERS[0].register(cx.waker()),
        }
        if EP0_SETUP.load(Ordering::Relaxed) {
            return Poll::Ready(Err(EndpointError::Disabled));
        }
        let done = match direction {
            Direction::In => STATE.in_done[0].swap(false, Ordering::Relaxed),
            Direction::Out => STATE.out_done[0].swap(false, Ordering::Relaxed),
        };
        if done {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    })
    .await
}

impl<'d, T: Instance> driver::ControlPipe for ControlPipe<'d, T> {
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    async fn setup(&mut self) -> [u8; 8] {
        set_response::<T>(0, Direction::Out, Response::Ack);
        poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());
            if EP0_SETUP.swap(false, Ordering::Relaxed) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;

        let mut packet = [0u8; 8];
        buffer(0).read(&mut packet);
        STATE.out_done[0].store(false, Ordering::Relaxed);
        // SETUP is DATA0, so the data stage starts at DATA1 in both directions.
        set_toggle::<T>(0, Direction::In, true);
        set_toggle::<T>(0, Direction::Out, true);
        set_response::<T>(0, Direction::Out, Response::Nak);
        set_response::<T>(0, Direction::In, Response::Nak);
        packet
    }

    async fn data_out(&mut self, buf: &mut [u8], _first: bool, _last: bool) -> Result<usize, EndpointError> {
        poll_fn(|cx| {
            EP_OUT_WAKERS[0].register(cx.waker());
            // A packet that already completed is consumed as-is; arming ACK is
            // only for the next one. The critical section keeps the handler from
            // completing a packet, or a new SETUP, between the check and the arm.
            critical_section::with(|_| {
                if EP0_SETUP.load(Ordering::Relaxed) {
                    Poll::Ready(Err(EndpointError::Disabled))
                } else if STATE.out_done[0].swap(false, Ordering::Relaxed) {
                    Poll::Ready(Ok(()))
                } else {
                    set_response::<T>(0, Direction::Out, Response::Ack);
                    Poll::Pending
                }
            })
        })
        .await?;

        let len = STATE.out_len[0].load(Ordering::Relaxed) as usize;
        let len = len.min(buf.len());
        buffer(0).read(&mut buf[..len]);
        flip_toggle::<T>(0, Direction::Out);
        Ok(len)
    }

    async fn data_in(&mut self, data: &[u8], _first: bool, last: bool) -> Result<(), EndpointError> {
        if data.len() > self.max_packet_size {
            return Err(EndpointError::BufferOverflow);
        }
        STATE.in_done[0].store(false, Ordering::Relaxed);
        buffer(0).write(data);
        compiler_fence(Ordering::SeqCst);
        T::regs().uep0_t_len().write(|w| w.set_uep0_t_len(data.len() as u8));
        set_response::<T>(0, Direction::In, Response::Ack);
        wait_ep0::<T>(Direction::In).await?;
        flip_toggle::<T>(0, Direction::In);

        if last {
            // The status stage of a control read is an empty OUT, still DATA1.
            set_response::<T>(0, Direction::Out, Response::Ack);
            wait_ep0::<T>(Direction::Out).await?;
        }
        Ok(())
    }

    async fn accept(&mut self) {
        // The status stage of a control write is an empty IN.
        STATE.in_done[0].store(false, Ordering::Relaxed);
        T::regs().uep0_t_len().write(|w| w.set_uep0_t_len(0));
        set_response::<T>(0, Direction::In, Response::Ack);
        let _ = wait_ep0::<T>(Direction::In).await;
    }

    async fn reject(&mut self) {
        set_response::<T>(0, Direction::In, Response::Stall);
        set_response::<T>(0, Direction::Out, Response::Stall);
    }

    async fn accept_set_address(&mut self, addr: u8) {
        self.accept().await;
        T::regs().dev_ad().write(|w| w.set_mask_usb_addr(addr));
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
            EP_IN_WAKERS[index].register(cx.waker());
            EP_OUT_WAKERS[index].register(cx.waker());
            if STATE.enabled[index].load(Ordering::Relaxed) {
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
        if !STATE.enabled[index].load(Ordering::Relaxed) {
            return Err(EndpointError::Disabled);
        }
        poll_fn(|cx| {
            EP_OUT_WAKERS[index].register(cx.waker());
            critical_section::with(|_| {
                if !STATE.enabled[index].load(Ordering::Relaxed) {
                    Poll::Ready(Err(EndpointError::Disabled))
                } else if STATE.out_done[index].swap(false, Ordering::Relaxed) {
                    Poll::Ready(Ok(()))
                } else {
                    // Arm ACK only when there is no completed packet to consume.
                    set_response::<T>(index, Direction::Out, Response::Ack);
                    Poll::Pending
                }
            })
        })
        .await?;

        let len = STATE.out_len[index].load(Ordering::Relaxed) as usize;
        let len = len.min(buf.len()).min(self.info.max_packet_size as usize);
        buffer(index).read(&mut buf[..len]);
        Ok(len)
    }
}

impl<'d, T: Instance> driver::EndpointIn for Endpoint<'d, T, In> {
    async fn write(&mut self, buf: &[u8]) -> Result<(), EndpointError> {
        let index = self.info.addr.index();
        if !STATE.enabled[index].load(Ordering::Relaxed) {
            return Err(EndpointError::Disabled);
        }
        if buf.len() > self.info.max_packet_size as usize {
            return Err(EndpointError::BufferOverflow);
        }
        STATE.in_done[index].store(false, Ordering::Relaxed);
        buffer(index).write(buf);
        compiler_fence(Ordering::SeqCst);
        set_tx_len::<T>(index, buf.len() as u8);
        set_response::<T>(index, Direction::In, Response::Ack);

        poll_fn(|cx| {
            EP_IN_WAKERS[index].register(cx.waker());
            if !STATE.enabled[index].load(Ordering::Relaxed) {
                Poll::Ready(Err(EndpointError::Disabled))
            } else if STATE.in_done[index].swap(false, Ordering::Relaxed) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

// --- endpoint registers --------------------------------------------------------

// Each endpoint's control register is a distinct PAC type, even though the
// fields are identical. The macro repeats the body per endpoint instead of
// pretending they share a type.
macro_rules! modify_ctrl {
    ($regs:expr, $index:expr, |$w:ident| $body:expr) => {
        match $index {
            0 => $regs.uep0_ctrl().modify(|$w| $body),
            1 => $regs.uep1_ctrl().modify(|$w| $body),
            2 => $regs.uep2_ctrl().modify(|$w| $body),
            3 => $regs.uep3_ctrl().modify(|$w| $body),
            _ => $regs.uep4_ctrl().modify(|$w| $body),
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
                let $w = $regs.uep1_ctrl().read();
                $body
            }
            2 => {
                let $w = $regs.uep2_ctrl().read();
                $body
            }
            3 => {
                let $w = $regs.uep3_ctrl().read();
                $body
            }
            _ => {
                let $w = $regs.uep4_ctrl().read();
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

fn set_toggle<T: Instance>(index: usize, direction: Direction, data1: bool) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| {
        match direction {
            Direction::In => w.set_uep_t_tog(data1),
            Direction::Out => w.set_uep_r_tog(data1),
        }
    });
}

fn flip_toggle<T: Instance>(index: usize, direction: Direction) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| {
        match direction {
            Direction::In => w.set_uep_t_tog(!w.uep_t_tog()),
            Direction::Out => w.set_uep_r_tog(!w.uep_r_tog()),
        }
    });
}

fn set_tx_len<T: Instance>(index: usize, len: u8) {
    let regs = T::regs();
    match index {
        0 => regs.uep0_t_len().write(|w| w.set_uep0_t_len(len)),
        1 => regs.uep1_t_len().write(|w| w.set_uep1_t_len(len)),
        2 => regs.uep2_t_len().write(|w| w.set_uep2_t_len(len)),
        3 => regs.uep3_t_len().write(|w| w.set_uep3_t_len(len)),
        _ => regs.uep4_t_len().write(|w| w.set_uep4_t_len(len)),
    }
}

fn enable_in<T: Instance>(index: usize) {
    let regs = T::regs();
    let dma = buffer(index).dma_address();
    match index {
        1 => {
            regs.uep1_dma().write(|w| w.set_uep1_dma(dma));
            regs.uep4_1_mod().modify(|w| w.set_uep1_tx_en(true));
        }
        2 => {
            regs.uep2_dma().write(|w| w.set_uep2_dma(dma));
            regs.uep2_3_mod().modify(|w| w.set_uep2_tx_en(true));
        }
        3 => {
            regs.uep3_dma().write(|w| w.set_uep3_dma(dma));
            regs.uep2_3_mod().modify(|w| w.set_uep3_tx_en(true));
        }
        _ => {
            // Endpoint 4's buffer follows endpoint 0's, so there is no DMA
            // register to write here.
            regs.uep4_1_mod().modify(|w| w.set_uep4_tx_en(true));
        }
    }
    auto_toggle::<T>(index);
    set_response::<T>(index, Direction::In, Response::Nak);
}

fn enable_out<T: Instance>(index: usize) {
    let regs = T::regs();
    let dma = buffer(index).dma_address();
    match index {
        1 => {
            regs.uep1_dma().write(|w| w.set_uep1_dma(dma));
            regs.uep4_1_mod().modify(|w| w.set_uep1_rx_en(true));
        }
        2 => {
            regs.uep2_dma().write(|w| w.set_uep2_dma(dma));
            regs.uep2_3_mod().modify(|w| w.set_uep2_rx_en(true));
        }
        3 => {
            regs.uep3_dma().write(|w| w.set_uep3_dma(dma));
            regs.uep2_3_mod().modify(|w| w.set_uep3_rx_en(true));
        }
        _ => {
            regs.uep4_1_mod().modify(|w| w.set_uep4_rx_en(true));
        }
    }
    auto_toggle::<T>(index);
    set_response::<T>(index, Direction::Out, Response::Nak);
}

fn auto_toggle<T: Instance>(index: usize) {
    let regs = T::regs();
    modify_ctrl!(regs, index, |w| w.set_uep_auto_tog(true));
}

// --- instance ------------------------------------------------------------------

mod sealed {
    use super::pac;

    pub trait Instance {
        type Interrupt: crate::interrupt::typelevel::Interrupt;

        fn regs() -> pac::usbfs::Usbfs;
    }
}

/// A USB full-speed device peripheral.
#[allow(private_bounds)]
pub trait Instance: sealed::Instance + PeripheralType {}

impl sealed::Instance for peripherals::USBFS {
    type Interrupt = interrupt::typelevel::USB;

    fn regs() -> pac::usbfs::Usbfs {
        pac::USBFS
    }
}

impl Instance for peripherals::USBFS {}
