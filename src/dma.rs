//! Peripheral-local DMA, not an STM32-style shared channel controller.
//!
//! Each CH5xx peripheral that has DMA owns its own address registers and
//! transfer unit: the timer moves 32-bit FIFO words, SPI0 moves bytes, and the
//! two engines share nothing beyond this SRAM-window descriptor type. There is
//! no channel to allocate, no request multiplexer and no cross-peripheral
//! arbitration, so the HAL deliberately exposes no `Channel` trait. A driver
//! owns its engine and hands out a transfer object instead.
//!
//! Storage is consumed as an exclusive static slice: forgetting a transfer
//! leaks storage, never leaves DMA pointing into an expired stack. No slice is
//! exposed until DMA is stopped.

pub mod address;
pub use address::{Descriptor, Error};

use crate::timer::low_level::Timer;
use crate::timer::{Events, Instance};

mod buffer;
pub use buffer::{StaticBuffer, StaticByteBuffer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    MemoryToPeripheral,
    PeripheralToMemory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    Once,
    Circular,
}

/// Order CPU, SRAM, and peripheral I/O on QingKe V3C.
#[inline]
pub(crate) fn fence() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    unsafe {
        core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Owns the buffer and borrows the entire timer, including its local DMA engine.
/// Completion means DMA reached END, *not* that a PWM waveform left the pin.
#[must_use = "dropping stops DMA; use stop to recover the static storage"]
pub struct Transfer<'a, 'd, T: Instance> {
    timer: &'a mut Timer<'d, T>,
    buffer: Option<&'static mut [u32]>,
    descriptor: Descriptor,
    direction: Dir,
    mode: RunMode,
}

impl<'a, 'd, T: Instance> Transfer<'a, 'd, T> {
    /// # Safety
    /// Timer mode and signal route must match direction. TX words must all fit
    /// CNT_END; raw circular RX consumption requires a separate ownership proof.
    pub(crate) unsafe fn new(
        timer: &'a mut Timer<'d, T>,
        buffer: &'static mut [u32],
        direction: Dir,
        mode: RunMode,
    ) -> Result<Self, (Error, &'static mut [u32])> {
        let descriptor = match Descriptor::new(buffer.as_ptr() as usize, buffer.len()) {
            Ok(d) => d,
            Err(e) => return Err((e, buffer)),
        };
        timer.stop();
        let r = timer.regs();
        // With requests stopped, move END away from NOW before clearing a
        // potentially level-like DMA_END. Do not enlarge an active descriptor.
        let now = r.dma_now().read().0 & 0x1fffc;
        r.dma_end().write(|w| {
            w.0 = if now == descriptor.start {
                descriptor.end
            } else {
                descriptor.start
            }
        });
        r.int_flag().write(|w| w.0 = 31);
        #[cfg(feature = "embassy")]
        {
            T::state().events.store(0, crate::atomic::Ordering::Relaxed);
            T::state().rx_finite.store(
                direction == Dir::PeripheralToMemory && mode == RunMode::Once,
                crate::atomic::Ordering::Relaxed,
            );
        }
        r.dma_beg().write(|w| w.0 = descriptor.start);
        r.dma_end().write(|w| w.0 = descriptor.end);
        fence();
        r.ctrl_dma()
            .write(|w| w.0 = if mode == RunMode::Circular { 5 } else { 1 });
        // On CH585 IF_DMA_END is gated by IE_DMA_END, even with PFIC masked.
        // Arm the peripheral event for polling and circular completion too.
        r.inter_en().write(|w| w.0 = Events::DMA_END.bits());
        timer.start();
        if direction == Dir::MemoryToPeripheral {
            timer.enable_output();
        }
        Ok(Self {
            timer,
            buffer: Some(buffer),
            descriptor,
            direction,
            mode,
        })
    }

    /// Modulo position in words. It cannot count missed complete circular laps.
    pub fn position(&self) -> usize {
        let now = self.timer.regs().dma_now().read().0 & 0x1fffc;
        now.saturating_sub(self.descriptor.start) as usize / 4
    }
    /// Finite source exhausted, or at least one circular END observed. A sticky
    /// circular event is not a new-lap notification and does not count wraps.
    pub fn is_complete(&self) -> bool {
        self.timer.events().contains(Events::DMA_END)
            || (self.mode == RunMode::Once && (self.timer.regs().dma_now().read().0 & 0x1fffc) == self.descriptor.end)
    }
    /// Conservative RX FIFO fault / best-effort early TX starvation detection.
    /// RX may report overflow caused by delayed servicing *after* DMA END.
    /// TX FIFO-empty after source exhaustion is normal; a coalesced earlier
    /// underrun cannot be distinguished then. Success is not a waveform-fidelity
    /// proof at an arbitrary bus load or period.
    pub fn error(&self) -> Option<Error> {
        if self.timer.events().contains(Events::FIFO_ERROR)
            && (self.direction == Dir::PeripheralToMemory || !self.is_complete())
        {
            Some(Error::Fifo)
        } else {
            None
        }
    }
    pub fn run_mode(&self) -> RunMode {
        self.mode
    }

    /// Stop DMA and the timer before returning any CPU reference to storage.
    pub fn stop(mut self) -> &'static mut [u32] {
        self.quiesce();
        self.buffer.take().unwrap()
    }
    /// Wait for DMA exhaustion, then stop. Does not promise PWM output drain.
    pub fn blocking_wait(mut self) -> Result<&'static mut [u32], (Error, &'static mut [u32])> {
        loop {
            if let Some(e) = self.error() {
                self.quiesce();
                return Err((e, self.buffer.take().unwrap()));
            }
            if self.is_complete() {
                self.quiesce();
                return Ok(self.buffer.take().unwrap());
            }
            core::hint::spin_loop();
        }
    }
    #[cfg(feature = "embassy")]
    pub async fn wait(
        &mut self,
        irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, crate::timer::InterruptHandler<T>>,
    ) -> Result<(), Error> {
        self.timer.bind_interrupt(irq);
        loop {
            if let Some(e) = self.error() {
                return Err(e);
            }
            if self.is_complete() {
                return Ok(());
            }
            self.timer.wait_event(Events::DMA_END.union(Events::FIFO_ERROR)).await;
        }
    }
    /// Drain a finite PWM waveform without blocking the executor. Dropping this
    /// future stops the owned transfer; its static buffer is intentionally not
    /// returned on cancellation. Forgetting it can leak resources, not memory.
    /// End the source with zero width for an exact finite active waveform: a
    /// nonzero final width can repeat during the conservative drain interval.
    #[cfg(feature = "embassy")]
    pub async fn finish(
        mut self,
        irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, crate::timer::InterruptHandler<T>>,
    ) -> Result<&'static mut [u32], (Error, &'static mut [u32])> {
        if self.direction != Dir::MemoryToPeripheral || self.mode != RunMode::Once {
            self.quiesce();
            return Err((Error::InvalidMode, self.buffer.take().unwrap()));
        }
        if let Err(e) = self.wait(irq).await {
            self.quiesce();
            return Err((e, self.buffer.take().unwrap()));
        }
        while self.timer.regs().fifo_count().read().fifo_count() != 0 {
            self.timer.acknowledge(Events::CYCLE_END);
            self.timer.wait_event(Events::CYCLE_END).await;
        }
        for _ in 0..=self.timer.repeat.cycles() {
            self.timer.acknowledge(Events::CYCLE_END);
            self.timer.wait_event(Events::CYCLE_END).await;
        }
        self.quiesce();
        Ok(self.buffer.take().unwrap())
    }
    /// Wait through the last FIFO value and its PWM repetitions, then stop.
    /// This is deliberately separate from DMA completion.
    /// End the source with zero width to avoid extra active final-width cycles.
    pub fn blocking_finish(mut self) -> Result<&'static mut [u32], (Error, &'static mut [u32])> {
        if self.direction != Dir::MemoryToPeripheral || self.mode != RunMode::Once {
            self.quiesce();
            return Err((Error::InvalidMode, self.buffer.take().unwrap()));
        }
        while !self.is_complete() {
            if let Some(e) = self.error() {
                self.quiesce();
                return Err((e, self.buffer.take().unwrap()));
            }
            core::hint::spin_loop();
        }
        let r = self.timer.regs();
        while r.fifo_count().read().fifo_count() != 0 {
            core::hint::spin_loop();
        }
        // FIFO becomes empty when its final item is loaded, not when that
        // item's output ends. Observe more than a complete repeat group.
        for _ in 0..=self.timer.repeat.cycles() {
            r.int_flag().write(|w| w.0 = 1);
            while !r.int_flag().read().if_cyc_end() {
                core::hint::spin_loop();
            }
        }
        self.quiesce();
        Ok(self.buffer.take().unwrap())
    }
    fn quiesce(&mut self) {
        self.timer.stop();
        let r = self.timer.regs();
        let now = r.dma_now().read().0 & 0x1fffc;
        r.dma_end().write(|w| {
            w.0 = if now == self.descriptor.start {
                self.descriptor.end
            } else {
                self.descriptor.start
            }
        });
        r.int_flag().write(|w| w.0 = 31);
        #[cfg(feature = "embassy")]
        T::state().events.store(0, crate::atomic::Ordering::Relaxed);
    }
}
impl<T: Instance> Drop for Transfer<'_, '_, T> {
    fn drop(&mut self) {
        self.quiesce();
    }
}
