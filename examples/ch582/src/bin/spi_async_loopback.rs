//! Exercise the interrupt-driven SPI0 path and its local DMA engine on CH582.
//!
//! Connect PA14 (MOSI) to PA15 (MISO). SCK is PA13; no chip select is used.
//! The bus requests 1 MHz from
//! the 6.4 MHz default clock. Build with `--features spi-async`.
//!
//! The first two checks use the interrupt path for full-duplex transfers in mode
//! 0 and mode 3. The half-duplex DMA receive and transmit checks each exceed
//! the 12-bit `TOTAL_CNT` limit, exercising chunked re-arming.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::mode::Async;
use hal::peripherals::SPI0;
use hal::spi::{Config, InterruptHandler, Spi};
use hal::time::Hertz;
use panic_halt as _;
use qingke_rt::entry;

hal::bind_interrupts!(struct Irqs {
    SPI0 => InterruptHandler<SPI0>;
});

/// Longer than one `TOTAL_CNT` chunk, so the driver re-arms mid-transfer.
const LEN: usize = 5000;

static TX: hal::dma::StaticByteBuffer<LEN> = hal::dma::StaticByteBuffer::new([0; LEN]);
static RX: hal::dma::StaticByteBuffer<LEN> = hal::dma::StaticByteBuffer::new([0; LEN]);

#[entry]
fn main() -> ! {
    let p = hal::init(Default::default());

    let tx = TX.take().unwrap();
    let rx = RX.take().unwrap();
    for (index, byte) in tx.iter_mut().enumerate() {
        *byte = (index % 251) as u8 + 1;
    }

    let spi = match Spi::new_async::<0>(p.SPI0, Irqs, p.PA13, p.PA14, p.PA15, Config::new(Hertz(1_000_000))) {
        Ok(spi) => spi,
        Err(error) => {
            defmt::error!("SPI init error {}", error as u8);
            loop {
                core::hint::spin_loop();
            }
        }
    };
    defmt::info!("SPI0 async engine ready");

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(run(spi, tx, rx).unwrap());
    })
}

#[embassy_executor::task]
async fn run(mut spi: Spi<'static, SPI0, Async>, tx: &'static mut [u8], rx: &'static mut [u8]) -> ! {
    defmt::info!("SPI async loopback, jumper PA14 to PA15");

    let payload = [0x00u8, 0xFF, 0xA5, 0x5A, 0x01, 0x80, 0x0F, 0xF0, 0x3C, 0xC3];
    let mut echo = [0u8; 10];
    let result = spi.transfer(&mut echo, &payload).await;
    match result {
        Ok(()) if echo == payload => defmt::info!("async mode 0: {} bytes matched", echo.len()),
        Ok(()) => defmt::error!("async mode 0: mismatched"),
        Err(_) => defmt::error!("async mode 0: transfer failed"),
    }

    let mut config = Config::new(Hertz(1_000_000));
    config.mode = embedded_hal::spi::MODE_3;
    spi.set_config(config).unwrap();
    let mut long = [0u8; 40];
    for (index, byte) in long.iter_mut().enumerate() {
        *byte = index as u8 * 7 + 3;
    }
    let mut received = [0u8; 40];
    match spi.transfer(&mut received, &long).await {
        Ok(()) if received == long => defmt::info!("async mode 3: {} bytes matched", long.len()),
        Ok(()) => defmt::error!("async mode 3: mismatched"),
        Err(_) => defmt::error!("async mode 3: transfer failed"),
    }

    // Restore mode 0 for the DMA checks.
    spi.set_config(Config::new(Hertz(1_000_000))).unwrap();

    // The receive engine samples the looped-back MOSI. Master receive mode does
    // not clock `0xFF` there, so the check is that the DMA filled every byte of
    // the chunk and that the wire held one value. The sentinel proves the buffer
    // was overwritten rather than left alone.
    for byte in rx.iter_mut() {
        *byte = 0x5A;
    }
    let rx_result: Result<(usize, u8, bool), ()> = async {
        let transfer = spi.read_dma(rx).map_err(|_| ())?;
        let buffer = transfer.wait(Irqs).await.map_err(|_| ())?;
        Ok((buffer.len(), buffer[0], buffer.iter().all(|byte| *byte == buffer[0])))
    }
    .await;
    match rx_result {
        Ok((length, value, true)) if value != 0x5A => {
            defmt::info!("dma rx {} bytes, uniform {=u8:x}", length, value)
        }
        Ok(_) => defmt::error!("dma rx: buffer not overwritten uniformly"),
        Err(()) => defmt::error!("dma rx failed"),
    }

    let tx_result: Result<usize, ()> = async {
        let transfer = spi.write_dma(tx).map_err(|_| ())?;
        let buffer = transfer.wait(Irqs).await.map_err(|_| ())?;
        Ok(buffer.len())
    }
    .await;
    match tx_result {
        Ok(length) => defmt::info!("dma tx {} bytes complete", length),
        Err(()) => defmt::error!("dma tx failed"),
    }

    defmt::info!("SPI async and DMA checks done");
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}
