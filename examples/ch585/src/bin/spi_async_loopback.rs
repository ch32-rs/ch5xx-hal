//! Exercise the interrupt-driven SPI path and the SPI0-local DMA engine.
//!
//! Wire PA14 (MOSI) to PA15 (MISO). SCK is PA13 and chip select is PA12; both
//! are driven by the example. The bus runs at 1 MHz from the 24 MHz PLL.
//!
//! The first two checks use the interrupt path for full-duplex transfers in mode
//! 0 and mode 3. The DMA checks then use the half-duplex engine: a receive is
//! checked against `0xFF`, the byte the controller puts on MOSI when it is only
//! clocking data in, and a transmit over the 12-bit `TOTAL_CNT` limit exercises
//! the chunked re-arm.

#![no_std]
#![no_main]

use ch5xx_hal as hal;
use defmt_rtt as _;
use hal::gpio::Level;
use hal::mode::Async;
use hal::peripherals::SPI0;
use hal::rcc::Config as ClockConfig;
use hal::spi::{chip_select, Config, InterruptHandler, Spi};
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
    let p = hal::init(hal::Config {
        rcc: ClockConfig::HSI_PLL_24MHZ,
    });

    let tx = TX.take().unwrap();
    let rx = RX.take().unwrap();
    for (index, byte) in tx.iter_mut().enumerate() {
        *byte = (index % 251) as u8 + 1;
    }

    let spi = Spi::new_async::<0>(p.SPI0, Irqs, p.PA13, p.PA14, p.PA15, Config::new(Hertz(1_000_000))).unwrap();
    let cs = chip_select(p.PA12, Level::High);

    static mut EXECUTOR: Option<embassy_executor::Executor> = None;
    #[allow(static_mut_refs)]
    let executor = unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(EXECUTOR);
        slot.insert(embassy_executor::Executor::new())
    };

    executor.run(move |spawner| {
        spawner.spawn(run(spi, cs, tx, rx).unwrap());
    })
}

#[embassy_executor::task]
async fn run(
    mut spi: Spi<'static, SPI0, Async>,
    mut cs: hal::spi::ChipSelect<'static>,
    tx: &'static mut [u8],
    rx: &'static mut [u8],
) -> ! {
    defmt::info!("SPI async loopback, wire PA14 to PA15");

    let payload = [0x00u8, 0xFF, 0xA5, 0x5A, 0x01, 0x80, 0x0F, 0xF0, 0x3C, 0xC3];
    let mut echo = [0u8; 10];
    let result = cs.select_async(spi.transfer(&mut echo, &payload)).await;
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
    match cs.select_async(spi.transfer(&mut received, &long)).await {
        Ok(()) if received == long => defmt::info!("async mode 3: {} bytes matched", long.len()),
        Ok(()) => defmt::error!("async mode 3: mismatched"),
        Err(_) => defmt::error!("async mode 3: transfer failed"),
    }

    // Restore mode 0 for the DMA checks.
    spi.set_config(Config::new(Hertz(1_000_000))).unwrap();

    let rx_result: Result<bool, ()> = cs
        .select_async(async {
            let transfer = spi.read_dma(rx).map_err(|_| ())?;
            let buffer = transfer.wait(Irqs).await.map_err(|_| ())?;
            Ok(buffer.iter().all(|byte| *byte == 0xFF))
        })
        .await;
    match rx_result {
        Ok(true) => defmt::info!("dma rx {} bytes, all ones", LEN),
        Ok(false) => defmt::error!("dma rx: unexpected data"),
        Err(()) => defmt::error!("dma rx failed"),
    }

    let tx_result: Result<usize, ()> = cs
        .select_async(async {
            let transfer = spi.write_dma(tx).map_err(|_| ())?;
            let buffer = transfer.wait(Irqs).await.map_err(|_| ())?;
            Ok(buffer.len())
        })
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
