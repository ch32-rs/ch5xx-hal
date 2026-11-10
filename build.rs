//! Generates the peripheral singletons, interrupt module and pin table for
//! `ch5xx-hal` from `ch5xx-metapac`'s metadata.
//!
//! This mirrors `ch32-hal/build.rs`, with the CH5xx differences folded in:
//!
//! - There is no RCC and no AFIO. The chip's clock/power/pin-mux registers live
//!   in the `SYS` (and `System_control`) blocks at `0x4000_1000`, so the
//!   `RccPeripheral` / `RemapPeripheral` impls ch32-hal emits have no
//!   counterpart here.
//! - GPIO is **one** peripheral (`GPIO`) split into per-port accessor registers
//!   (`PA_DIR`, `PB_OUT`, …), not one peripheral per port. Pins are still
//!   individual singletons (`PA0`, … `PB23`) so the driver API is unchanged.
//! - There are no per-pin EXTI channels; `GPIO_A` / `GPIO_B` are whole-port
//!   interrupts.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::{env, fs};

use ch5xx_metapac::metadata::METADATA;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Exactly one chip feature, e.g. `ch585`.
    let chip_name = env::vars()
        .map(|(a, _)| a)
        .filter(|x| x.starts_with("CARGO_FEATURE_CH5"))
        .collect::<Vec<_>>();
    let chip_name = match chip_name.len() {
        0 => panic!("no ch5xx chip Cargo feature enabled (e.g. `ch585`)"),
        1 => chip_name[0]
            .strip_prefix("CARGO_FEATURE_")
            .unwrap()
            .to_ascii_lowercase(),
        _ => panic!("multiple ch5xx chip Cargo features enabled; pick one"),
    };
    println!("cargo:rustc-cfg={chip_name}");

    // CH570 is the CH572 die without the radio. The radio is a register-less
    // placeholder, so every modelled block is shared; emit the CH572 cfg as
    // well so the drivers keyed on that family apply.
    let family = match chip_name.as_str() {
        "ch570" => "ch572",
        other => other,
    };
    if family != chip_name {
        println!("cargo:rustc-cfg={family}");
    }
    let chip_name = family.to_string();

    // Per-chip GPIO port widths. Metadata only lists pins that carry a
    // peripheral signal, so pads with no alternate function would not be
    // discovered; the widths differ per die and stay here.
    let ports: &[(char, u8)] = match chip_name.as_str() {
        "ch581" | "ch582" | "ch583" | "ch584" | "ch585" | "ch592" | "ch595" | "ch573" => &[('A', 16), ('B', 24)],
        // CH572/CH570 bond PA0-PA11 only.
        "ch572" => &[('A', 12)],
        other => panic!("no GPIO port map for chip `{other}`"),
    };
    // The PAC's `METADATA` is empty here: a build-dependency is compiled
    // without a chip feature, so the chip's register block is read from the
    // sibling data tree instead.
    let gpio_items = gpio_items(&chip_name);
    let port_b = gpio_items.iter().any(|item| item == "PB_DIR");
    let gpio_set = gpio_items.iter().any(|item| item == "PA_SET");
    let pin_intx = sys_has_pin_intx(&chip_name);
    // Port B, the write-1-to-set register and `PIN_INTX` are not on every die.
    // Drivers consult the cfg instead of naming a register the PAC lacks.
    if port_b {
        println!("cargo:rustc-cfg=gpio_port_b");
    }
    if gpio_set {
        println!("cargo:rustc-cfg=gpio_set");
    }
    if pin_intx {
        println!("cargo:rustc-cfg=gpio_pin_intx");
    }
    // The low-power driver writes the sleep and power-plan bytes of the SYS
    // block. CH592 has no HFCK_PWR_CTRL, so the set is not shared by every
    // selectable die; the driver only compiles where all of it exists.
    // CH572's POWER_PLAN is a different layout (no DC/DC or MUST_0010 bits),
    // so the shared sleep path does not model it yet.
    if chip_name != "ch572"
        && sys_has_registers(
            &chip_name,
            &[
                "SLP_WAKE_CTRL",
                "SLP_POWER_CTRL",
                "POWER_PLAN",
                "HFCK_PWR_CTRL",
                "BAT_DET_CTRL",
            ],
        )
    {
        println!("cargo:rustc-cfg=power_sleep");
    }
    // CH582/CH583 split the main SRAM retention differently: `PWR_RAM2K` is
    // the 2 KB block, `PWR_RAM30K` the 30 KB main block, and there is no
    // `PWR_LDO_EN`. The driver needs its own retention names and plan
    // composition.
    if register_version_opt(&chip_name, "sys").as_deref() == Some("ch583") {
        println!("cargo:rustc-cfg=power_plan_ch58x");
    }
    // CH592 and CH582/CH583 have the same shape of clock tree: a 32 MHz
    // crystal as the only high-speed source and a 480 MHz PLL. The register
    // fields and the safe-access window still differ, so only the tree model,
    // presets and validation are shared.
    if chip_name == "ch592" || register_version_opt(&chip_name, "sys").as_deref() == Some("ch583") {
        println!("cargo:rustc-cfg=xtal480");
    }
    // CH592/CH595 use a newer POWER_PLAN layout (`PWR_RAM24K` and
    // `PWR_RAM8K` instead of `PWR_RAM32K`/`PWR_RAM96K`, no `PWR_MUST_0010`).
    // The driver composes that plan from raw bits; CH592 also lacks
    // `HFCK_PWR_CTRL`, while CH595 keeps it.
    if sys_has_registers(
        &chip_name,
        &[
            "SLP_WAKE_CTRL",
            "SLP_POWER_CTRL",
            "POWER_PLAN",
            "BAT_DET_CTRL",
            "PWR_RAM24K",
        ],
    ) {
        println!("cargo:rustc-cfg=power_sleep_ch59x");
    }
    // The SPI register block is not one layout. The `ch585`, `ch583` and
    // `ch592` versions are the FIFO controller this driver programs; the
    // others are not, and naming their registers would not compile. CH592
    // narrows the DMA address registers to 16 bits and drops SPI1
    // (`SPI_NODMA`); the driver casts the addresses to the field width and
    // gates the second instance.
    match register_version(&chip_name, "spi").as_str() {
        // CH573 and CH572/CH570 use the same FIFO controller; CH572 selects
        // its alternate SCK through `PIN_ALTERNATE_H` instead of `PIN_SPI0`.
        "ch585" | "ch583" | "ch592" | "ch573" | "ch572" => println!("cargo:rustc-cfg=spi_fifo"),
        _ => {}
    }
    // CH583 has single conversion and DMA but no scan sequencer. CH573 and
    // CH595 are the same shape without the scan block; CH592 has a shorter
    // scan sequencer than CH585.
    match register_version_opt(&chip_name, "adc").as_deref() {
        Some("ch585") => {
            println!("cargo:rustc-cfg=adc_ch585");
            println!("cargo:rustc-cfg=adc_scan");
        }
        Some("ch583") => println!("cargo:rustc-cfg=adc_ch583"),
        Some("ch573") => println!("cargo:rustc-cfg=adc_ch573"),
        Some("ch595") => println!("cargo:rustc-cfg=adc_ch595"),
        Some("ch592") => {
            println!("cargo:rustc-cfg=adc_ch592");
            println!("cargo:rustc-cfg=adc_scan");
        }
        _ => {}
    }
    // The gain encoding moved to `CFG.PGA_GAIN10` plus `CONVERT.PGA_GAIN2` on
    // the newer blocks; CH573 and CH583 keep the single `PGA_GAIN` field, and
    // CH573/CH595 have no direction bit at all.
    if register_has_field(&chip_name, "adc", "PGA_GAIN10") {
        println!("cargo:rustc-cfg=adc_pga_gain10");
    }
    if register_has_field(&chip_name, "adc", "PGA_GAIN") {
        println!("cargo:rustc-cfg=adc_pga_gain");
    }
    if register_has_field(&chip_name, "adc", "PGA_GAIN2") {
        println!("cargo:rustc-cfg=adc_pga_gain2");
    }
    if register_has_field(&chip_name, "adc", "DMA_LOOP") {
        println!("cargo:rustc-cfg=adc_dma_loop");
    }
    if register_has_field(&chip_name, "adc", "CONT_EN") {
        println!("cargo:rustc-cfg=adc_cont_en");
    }
    // CH583 and CH573 program DMA_BEG/DMA_END as byte addresses; the newer
    // blocks store halfword offsets.
    if matches!(
        register_version_opt(&chip_name, "adc").as_deref(),
        Some("ch583" | "ch573")
    ) {
        println!("cargo:rustc-cfg=adc_dma_bytes");
    }
    // The touch-key block is the ADC's charge-transfer front end. CH573, CH583,
    // CH585 and CH592 keep the COUNT/CONVERT pair and the CFG power bit that
    // the sampling driver programs; CH595 replaced them with the CFG2 transfer
    // registers and CH572 has no block. Gate on the register set rather than on
    // the version, because the unused CFG bits differ between the shared dies.
    if let Some(version) = register_version_opt(&chip_name, "touchkey") {
        let path = data_root().join(format!("registers/touchkey_{version}.yaml"));
        let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
        let names = register_names(&text);
        if names.iter().any(|name| name == "COUNT") && names.iter().any(|name| name == "CONVERT") {
            println!("cargo:rustc-cfg=touchkey_charge");
        }
    }
    if register_has_field(&chip_name, "touchkey", "PWR_ON") {
        println!("cargo:rustc-cfg=touchkey_pwr_on");
    }
    if register_has_field(&chip_name, "touchkey", "DRV_EN") {
        println!("cargo:rustc-cfg=touchkey_drv_en");
    }
    // PWMx has three layouts: the CH572 block drives PWM1-PWM5 with 16-bit
    // cycle registers, the CH573 block drives PWM4-PWM11 with byte duty
    // registers, and the CH585/CH592 blocks add the wide mode and four stagger
    // pairs.
    match register_version_opt(&chip_name, "pwmx").as_deref() {
        Some("ch572") => println!("cargo:rustc-cfg=pwmx_ch572"),
        Some("ch573") => println!("cargo:rustc-cfg=pwmx_ch573"),
        Some("ch585") => println!("cargo:rustc-cfg=pwmx_ch585"),
        Some("ch592") => println!("cargo:rustc-cfg=pwmx_ch592"),
        _ => {}
    }
    // The calendar counter is the CH585 layout, which the CH573 version shares.
    // CH572 counts on a different set of registers.
    if let Some("ch585" | "ch573") = register_version_opt(&chip_name, "rtc").as_deref() {
        println!("cargo:rustc-cfg=rtc_calendar");
    }
    // The battery detector fields are the CH585 ones, which CH581 shares; CH592
    // has the same `BAT_DET_*` and `BAT_STATUS` layout. The window watchdog
    // itself is in `system_control` and is common to every selectable chip.
    if let Some("ch585" | "ch581" | "ch592") = register_version_opt(&chip_name, "sys").as_deref() {
        println!("cargo:rustc-cfg=battery_ch585");
    }
    // The reset-status register shares its byte with the ROM configuration;
    // these are the system-control versions that model it.
    if let Some("ch585" | "ch583" | "ch592" | "ch595" | "ch573" | "ch572") =
        register_version_opt(&chip_name, "system_control").as_deref()
    {
        println!("cargo:rustc-cfg=reset_status");
    }
    // TMR instances are not all the same block. CH592 and CH595 omit the
    // timer-local DMA registers on TMR0/TMR3, while the other dies have them on
    // every instance. Emit one cfg per no-DMA instance so the driver can gate
    // the DMA entry points.
    // The timer and UART groups are not complete on every die: CH572/CH570
    // have TMR0 and UART0 only. Emit one cfg per optional instance so the
    // driver does not name a singleton the PAC lacks.
    for (name, block) in peripheral_blocks(&chip_name, "TMR") {
        let cfg = name.to_ascii_lowercase();
        if cfg != "tmr0" {
            println!("cargo:rustc-cfg={cfg}");
        }
        if block == "TMR_NODMA" {
            println!("cargo:rustc-cfg={cfg}_nodma");
        }
    }
    // TMR0/TMR3 can run the NFC 13.56 MHz clock only on dies with an NFC
    // front end; the CH572 family has none. NFC is a register-less
    // placeholder, so this reads the name rather than a register block.
    if family_has_peripheral(&chip_name, "NFC") {
        println!("cargo:rustc-cfg=nfc_clock");
    }
    // Not every die routes every timer to a pin. The `PIN_TMRn` remap fields
    // in PIN_ALTERNATE are the metadata's record of it; the pin tables and the
    // remap writer are gated on them.
    for name in ["TMR0", "TMR1", "TMR2", "TMR3"] {
        let field = format!("PIN_{name}");
        if sys_has_registers(&chip_name, &[field.as_str()]) {
            println!("cargo:rustc-cfg=remap_{}", name.to_ascii_lowercase());
        }
    }
    // UART1-3 drop the modem-control block on several dies (CH592, CH583,
    // CH595). The instance type and the register fields differ, so the driver
    // needs to know which block each instance is.
    for (name, block) in peripheral_blocks(&chip_name, "UART") {
        let cfg = name.to_ascii_lowercase();
        if cfg != "uart0" {
            println!("cargo:rustc-cfg={cfg}");
        }
        if block == "UART_NOMODEM" {
            println!("cargo:rustc-cfg={cfg}_nomodem");
        }
    }
    // Not every die has an I2C block (CH573 does not).
    if register_version_opt(&chip_name, "i2c").is_some() {
        println!("cargo:rustc-cfg=i2c_present");
    }
    // USBFS is the WCH full-speed device controller. USBHS is a separate block
    // and is not this driver.
    if register_version_opt(&chip_name, "usbfs").as_deref() == Some("ch585") {
        println!("cargo:rustc-cfg=usbfs_ch585");
    }
    // CH592 records the same device controller as kind `usb`, version `ch592`.
    // The device-mode register offsets and field names match the CH585 block;
    // this layout has no host-mode aliases, which the driver does not use.
    if register_version_opt(&chip_name, "usb").as_deref() == Some("ch592") {
        println!("cargo:rustc-cfg=usb_ch592");
    }
    if register_version_opt(&chip_name, "usb").as_deref() == Some("ch583") {
        println!("cargo:rustc-cfg=usb_ch583");
    }
    // USBHS has a separate controller, PHY and device interrupt.
    if register_version_opt(&chip_name, "usbhs").as_deref() == Some("ch585") {
        println!("cargo:rustc-cfg=usbhs_ch585");
    }
    // Register fields are not identical across the family. Emit a cfg for each
    // capability a shared driver would otherwise name unconditionally. The
    // field scan is textual, so only exact `- name:` entries count.
    for (field, cfg) in [
        ("SLP_USB2_WAKE", "slp_usb2_wake"),
        ("WAKE_DLY_MOD", "wake_dly_mod"),
        ("PWR_RAM2K", "pwr_ram2k"),
        ("PWR_RAM8K", "pwr_ram8k"),
        ("PWR_RAM12K", "pwr_ram12k"),
        ("PWR_RAM16K", "pwr_ram16k"),
        ("PWR_RAM24K", "pwr_ram24k"),
        ("PWR_RAM30K", "pwr_ram30k"),
        ("PWR_RAM32K", "pwr_ram32k"),
        ("PWR_RAM96K", "pwr_ram96k"),
        ("CLK_RC16M_PON", "hfck_rc16m_pon"),
        ("XROM_SCLK_SEL", "clk_cfg_xrom"),
        ("OSC32M_SEL", "clk_cfg_osc32m"),
        ("PLL_GATE_DISS", "clk_cfg_pll_gate"),
        ("PLL_GATE_TIME", "clk_cfg_pll_gate_time"),
        ("PIN_UART0", "pin_uart0"),
        ("PIN_UART1", "pin_uart1"),
        ("PIN_UART2", "pin_uart2"),
        ("PIN_UART3", "pin_uart3"),
        ("PIN_I2C", "pin_i2c"),
        ("PIN_SPI0", "pin_spi0"),
        ("PIN_PWMX", "pin_pwmx"),
        ("PIN_TMR0", "pin_tmr0"),
        ("PIN_TMR1", "pin_tmr1"),
        ("PIN_TMR2", "pin_tmr2"),
        ("PIN_TMR3", "pin_tmr3"),
    ] {
        if sys_has_field(&chip_name, field) {
            println!("cargo:rustc-cfg={cfg}");
        }
    }
    if sys_has_registers(&chip_name, &["PIN_ALTERNATE_H"]) {
        println!("cargo:rustc-cfg=pin_alternate_h");
    }
    // USBFS PHY enable lives in a different SYS register on each cut. The
    // field names are the same on CH585/CH581/CH584 and CH595; CH572 uses the
    // low PIN_ALTERNATE byte.
    if sys_has_registers(&chip_name, &["PIN_CONFIG"]) {
        println!("cargo:rustc-cfg=usb_phy_pin_config");
    } else if sys_has_registers(&chip_name, &["PIN_ANALOG_IE"]) {
        println!("cargo:rustc-cfg=usb_phy_pin_analog_ie");
    } else {
        println!("cargo:rustc-cfg=usb_phy_pin_alternate");
    }
    if register_has_field(&chip_name, "systick", "MODE") {
        println!("cargo:rustc-cfg=systick_mode");
    }
    // CH573 names the reload-enable bit `STRELOAD` and moves it to bit 8.
    if register_has_field(&chip_name, "systick", "STRE") {
        println!("cargo:rustc-cfg=systick_stre");
    }
    if register_has_field(&chip_name, "systick", "STRELOAD") {
        println!("cargo:rustc-cfg=systick_streload");
    }
    // CH585-CH59x keep the flag in `SR`; CH572 and CH573 call it `CNTFG`.
    // CH572 has no high counter half at all.
    if register_has_register(&chip_name, "systick", "SR") {
        println!("cargo:rustc-cfg=systick_sr");
    }
    if register_has_register(&chip_name, "systick", "CNTFG") {
        println!("cargo:rustc-cfg=systick_cntfg");
    }
    if register_has_register(&chip_name, "systick", "CNTH") {
        println!("cargo:rustc-cfg=systick_cnth");
    }
    if register_has_field(&chip_name, "uart", "MCR_INT_OE") {
        println!("cargo:rustc-cfg=mcr_int_oe");
    }
    if register_has_field(&chip_name, "uart", "MCR_OUT2_RB_MCR_INT_OE") {
        println!("cargo:rustc-cfg=mcr_int_oe_combined");
    }
    // CH572/CH570's UART has no software-reset bit, no FIFO-clear bits and no
    // modem block, so the fields the shared setup writes do not all exist.
    if register_has_field(&chip_name, "uart", "IER_RESET") {
        println!("cargo:rustc-cfg=uart_ier_reset");
    }
    if register_has_field(&chip_name, "uart", "FCR_RX_FIFO_CLR") {
        println!("cargo:rustc-cfg=uart_fcr_clear");
    }
    if register_has_field(&chip_name, "uart", "MCR_LOOP") {
        println!("cargo:rustc-cfg=uart_mcr_loop");
    }
    if sys_has_field(&chip_name, "CLK_RC16M_PON") {
        println!("cargo:rustc-cfg=has_hsi");
    }
    // The PLL multiplier is fixed per family: 312 MHz on CH581/CH584/CH585,
    // 480 MHz on CH573/CH595 and the CH59x/CH58x crystal tree, 600 MHz on
    // CH572. The centre frequency feeds presets and validation.
    match chip_name.as_str() {
        "ch572" => println!("cargo:rustc-cfg=pll_600"),
        "ch573" | "ch595" => println!("cargo:rustc-cfg=pll_480"),
        "ch581" | "ch584" | "ch585" => println!("cargo:rustc-cfg=pll_312"),
        _ => {}
    }

    // A `time-driver-tmr*` feature takes that TMR for the timebase. Drop it from
    // the singletons so `init()` cannot hand the same peripheral to a second
    // user; the driver reaches it through the PAC anyway. Exactly one such
    // feature may be enabled (`embassy::mod` enforces that), so a lookup is
    // enough.
    let time_driver_peripheral: Option<String> = ["TMR0", "TMR1", "TMR2", "TMR3"]
        .into_iter()
        .find(|n| env::var_os(format!("CARGO_FEATURE_TIME_DRIVER_{n}")).is_some())
        .map(str::to_owned);

    // Peripheral singletons. Skip the register-less placeholders (BLE) and the
    // core-internal blocks (PFIC, SysTick) — the HAL never hands those out.
    const SKIP_KINDS: &[&str] = &["pfic", "systick"];
    let mut singletons: Vec<String> = Vec::new();
    for p in METADATA.peripherals {
        if p.registers.is_none() {
            continue;
        }
        if SKIP_KINDS.contains(&p.registers.as_ref().unwrap().kind) {
            continue;
        }
        if p.registers.as_ref().unwrap().kind == "gpio" {
            // Pins are the user-facing singletons; the port block itself is not.
            continue;
        }
        let name = normalize(p.name);
        if time_driver_peripheral.as_deref() == Some(name.as_str()) {
            continue;
        }
        singletons.push(name);
    }

    // One singleton per pin.
    let mut pins: Vec<(String, char, u8, u8)> = Vec::new();
    for (port, width) in ports {
        for n in 0..*width {
            pins.push((format!("P{port}{n}"), *port, (*port as u8) - b'A', n));
        }
    }

    // ---- _generated.rs -------------------------------------------------
    let mut g = TokenStream::new();

    let singleton_tokens: Vec<_> = singletons.iter().map(|s| format_ident!("{}", s)).collect();
    let pin_tokens: Vec<_> = pins.iter().map(|(p, ..)| format_ident!("{}", p)).collect();

    g.extend(quote! {
        crate::peripherals_definition!(#(#singleton_tokens,)* #(#pin_tokens),*);
        crate::peripherals_struct!(#(#singleton_tokens,)* #(#pin_tokens),*);
    });

    // Interrupt declarations go into their own file: `interrupt_mod!` expands
    // to `pub mod interrupt`, whose methods name `crate::interrupt`, which does
    // not resolve from inside the `_generated` module.
    let irqs: Vec<_> = METADATA
        .interrupts
        .iter()
        .map(|irq| format_ident!("{}", irq.name))
        .collect();
    let irqs_file = quote! {
        crate::interrupt_mod!(#(#irqs,)*);
    };
    fs::write(out_dir.join("_interrupts.rs"), irqs_file.to_string()).unwrap();

    // ---- _macros.rs ----------------------------------------------------
    let mut m = String::new();

    // foreach_peripheral: kind, name
    let peripherals_table: Vec<Vec<String>> = METADATA
        .peripherals
        .iter()
        .filter_map(|p| {
            p.registers
                .as_ref()
                .map(|r| vec![r.kind.to_string(), normalize(p.name)])
        })
        .collect();
    make_table(&mut m, "foreach_peripheral", &peripherals_table);

    // foreach_pin: pin, port_name, port_num, pin_num
    let pins_table: Vec<Vec<String>> = pins
        .iter()
        .map(|(pin, port, port_num, pin_num)| {
            vec![
                pin.clone(),
                format!("GPIO{port}"),
                port_num.to_string(),
                pin_num.to_string(),
            ]
        })
        .collect();
    make_table(&mut m, "foreach_pin", &pins_table);

    // foreach_interrupt: peripheral, kind, block, signal, interrupt.
    // Register-less placeholders (BLE, NFC) are skipped: the table's `block`
    // column names a register block, which they do not have. Their vectors are
    // still in `pac::Interrupt`, so a driver can bind them directly.
    let interrupts_table: Vec<Vec<String>> = METADATA
        .peripherals
        .iter()
        .filter_map(|p| {
            let r = p.registers.as_ref()?;
            (!p.interrupts.is_empty()).then(|| {
                p.interrupts
                    .iter()
                    .map(|irq| {
                        vec![
                            normalize(p.name),
                            r.kind.to_string(),
                            r.block.to_string(),
                            irq.signal.to_string(),
                            irq.interrupt.to_ascii_uppercase(),
                        ]
                    })
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect();
    make_table(&mut m, "foreach_interrupt", &interrupts_table);

    // ---- write ---------------------------------------------------------
    fs::write(out_dir.join("_generated.rs"), g.to_string()).unwrap();
    fs::write(out_dir.join("_macros.rs"), m).unwrap();

    println!("cargo:rerun-if-changed=build.rs");
}

/// Register names of the chip's GPIO block, read from the sibling data tree.
///
/// `ch5xx-metapac`'s `METADATA` cannot answer this. Its build script only
/// fills the const when a chip feature is enabled, and a build-dependency is
/// compiled with `metadata` alone, so the const is empty here.
fn gpio_items(chip: &str) -> Vec<String> {
    let version = register_version(chip, "gpio");
    let path = data_root().join(format!("registers/gpio_{version}.yaml"));
    register_names(&fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display())))
}

/// `PIN_ALTERNATE.PIN_INTX` selects which pair the shared flag bits follow.
/// A die can bond the high pins and still have no such field.
fn sys_has_pin_intx(chip: &str) -> bool {
    // The field lives in the `PIN_ALTERNATE` fieldset. Matching the name is
    // enough: no other fieldset in these blocks reuses it.
    sys_has_registers(chip, &["PIN_INTX"])
}

/// Whether the chip's `SYS` block defines every one of `names`.
fn sys_has_registers(chip: &str, names: &[&str]) -> bool {
    let version = register_version(chip, "sys");
    let path = data_root().join(format!("registers/sys_{version}.yaml"));
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    names
        .iter()
        .all(|name| text.lines().any(|line| line.trim() == format!("- name: {name}")))
}

/// Whether the chip's `SYS` block defines the named field.
fn sys_has_field(chip: &str, field: &str) -> bool {
    register_has_field(chip, "sys", field)
}

/// Whether the chip's selected register block of `kind` has a register named
/// `name` in its first block.
fn register_has_register(chip: &str, kind: &str, name: &str) -> bool {
    let Some(version) = register_version_opt(chip, kind) else {
        return false;
    };
    let path = data_root().join(format!("registers/{kind}_{version}.yaml"));
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    register_names(&text).iter().any(|candidate| candidate == name)
}

/// Whether the chip's selected register block of `kind` defines `field`.
fn register_has_field(chip: &str, kind: &str, field: &str) -> bool {
    let Some(version) = register_version_opt(chip, kind) else {
        return false;
    };
    let path = data_root().join(format!("registers/{kind}_{version}.yaml"));
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    text.lines().any(|line| line.trim() == format!("- name: {field}"))
}

fn data_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../ch5xx-data/data")
}

/// `(instance name, block)` for every peripheral whose name starts with
/// `prefix`, e.g. `TMR` or `UART`.
///
/// The family file names the register block next to each instance so a die can
/// mix block variants (DMA-capable and DMA-less timers, full and no-modem
/// UARTs); the HAL keys its instance types on this instead of assuming one
/// block for the whole group.
fn peripheral_blocks(chip: &str, prefix: &str) -> Vec<(String, String)> {
    let chip_path = data_root().join(format!("chips/{}.yaml", chip.to_ascii_uppercase()));
    let family = family_file(
        &fs::read_to_string(&chip_path).unwrap_or_else(|err| panic!("reading {}: {err}", chip_path.display())),
    );
    let family_path = data_root().join(format!("family/{family}"));
    let text =
        fs::read_to_string(&family_path).unwrap_or_else(|err| panic!("reading {}: {err}", family_path.display()));

    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(name) = line.trim().strip_prefix("- name: ") else {
            continue;
        };
        if !name.starts_with(prefix) {
            continue;
        }
        // The block sits in the same `registers:` entry, before the next one.
        let block = lines[index + 1..]
            .iter()
            .map(|candidate| candidate.trim())
            .take_while(|candidate| !candidate.starts_with("- name:"))
            .find_map(|candidate| candidate.strip_prefix("block: "))
            .unwrap_or_else(|| panic!("{name} has no register block"));
        blocks.push((name.to_string(), block.to_string()));
    }
    blocks
}

/// The `version:` of the `kind` register block the chip's family selects.
fn register_version(chip: &str, kind: &str) -> String {
    register_version_opt(chip, kind).unwrap_or_else(|| panic!("family has no `{kind}` register block"))
}

/// The `version:` of the `kind` register block, when the family has one.
fn register_version_opt(chip: &str, kind: &str) -> Option<String> {
    let chip_path = data_root().join(format!("chips/{}.yaml", chip.to_ascii_uppercase()));
    let family = family_file(
        &fs::read_to_string(&chip_path).unwrap_or_else(|err| panic!("reading {}: {err}", chip_path.display())),
    );
    let family_path = data_root().join(format!("family/{family}"));
    let text =
        fs::read_to_string(&family_path).unwrap_or_else(|err| panic!("reading {}: {err}", family_path.display()));
    version_of(&text, kind)
}

/// Whether the chip's family file lists a peripheral by name.
///
/// Works for register-less placeholders such as NFC, where
/// [`peripheral_blocks`] cannot be used.
fn family_has_peripheral(chip: &str, name: &str) -> bool {
    let chip_path = data_root().join(format!("chips/{}.yaml", chip.to_ascii_uppercase()));
    let family = family_file(
        &fs::read_to_string(&chip_path).unwrap_or_else(|err| panic!("reading {}: {err}", chip_path.display())),
    );
    let family_path = data_root().join(format!("family/{family}"));
    let text =
        fs::read_to_string(&family_path).unwrap_or_else(|err| panic!("reading {}: {err}", family_path.display()));
    text.lines().any(|line| line.trim() == format!("- name: {name}"))
}

/// The family file named by `include_peripherals`.
fn family_file(chip_yaml: &str) -> String {
    chip_yaml
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- \"../family/")
                .or_else(|| trimmed.strip_prefix("- '../family/"))
                .and_then(|rest| rest.strip_suffix('"').or_else(|| rest.strip_suffix('\'')))
        })
        .expect("chip yaml has no include_peripherals family entry")
        .to_string()
}

fn version_of(family_yaml: &str, kind: &str) -> Option<String> {
    let lines: Vec<&str> = family_yaml.lines().collect();
    lines.iter().enumerate().find_map(|(index, line)| {
        if line.trim() != format!("kind: {kind}") {
            return None;
        }
        // The version sits next to its kind, before the next register block.
        // Reading past that boundary picks up the following block's version.
        lines[index + 1..]
            .iter()
            .map(|candidate| candidate.trim())
            .take_while(|candidate| !candidate.starts_with("- name:"))
            .find_map(|candidate| candidate.strip_prefix("version: "))
            .map(str::to_string)
    })
}

/// Names of the registers in the first block of a register YAML file.
///
/// Fieldsets are skipped. Some files list them before the block and some after,
/// so the scan follows the `block/` section instead of assuming an order.
fn register_names(yaml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_block = false;
    let mut in_items = false;
    for line in yaml.lines() {
        if line.starts_with("block/") {
            in_block = true;
            in_items = false;
            continue;
        }
        if !in_block {
            continue;
        }
        if line.starts_with("fieldset/") || (!line.starts_with(' ') && !line.is_empty()) {
            break;
        }
        if line.trim() == "items:" {
            in_items = true;
            continue;
        }
        if in_items {
            if let Some(name) = line.trim().strip_prefix("- name: ") {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Metadata names are free-form (`System_control`, `PWMx`); make them valid,
/// conventional Rust singleton idents (`SYSTEM_CONTROL`, `PWMX`).
fn normalize(name: &str) -> String {
    name.to_ascii_uppercase().replace('-', "_")
}

fn make_table(out: &mut String, name: &str, rows: &[Vec<String>]) {
    // Deduplicate: one row per (peripheral, signal, irq) is already unique, but
    // the generic helper is also used for tables where a chip can repeat a key.
    let mut seen = BTreeSet::new();
    write!(
        out,
        "#[allow(unused)]
macro_rules! {name} {{
    ($($pat:tt => $code:tt;)*) => {{
        macro_rules! __{name}_inner {{
            $(($pat) => $code;)*
            ($_:tt) => {{}}
        }}
"
    )
    .unwrap();

    for row in rows {
        if !seen.insert(row.join(",")) {
            continue;
        }
        writeln!(out, "        __{name}_inner!(({}));", row.join(",")).unwrap();
    }

    write!(out, "    }};\n}}").unwrap();
}
