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

    // Per-chip GPIO port widths. Metadata only lists pins that carry a
    // peripheral signal, so pads with no alternate function would not be
    // discovered; the widths differ per die and stay here.
    let ports: &[(char, u8)] = match chip_name.as_str() {
        "ch581" | "ch582" | "ch583" | "ch584" | "ch585" | "ch592" | "ch595" | "ch573" => &[('A', 16), ('B', 24)],
        "ch572" => &[('A', 24)],
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
        let name = normalize(&p.name);
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
                .map(|r| vec![r.kind.to_string(), normalize(&p.name)])
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
                            normalize(&p.name),
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
    let version = register_version(chip, "sys");
    let path = data_root().join(format!("registers/sys_{version}.yaml"));
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    // The field lives in the `PIN_ALTERNATE` fieldset. Matching the name is
    // enough: no other fieldset in these blocks reuses it.
    text.lines().any(|line| line.trim() == "- name: PIN_INTX")
}

fn data_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../ch5xx-data/data")
}

/// The `version:` of the `kind` register block the chip's family selects.
fn register_version(chip: &str, kind: &str) -> String {
    let chip_path = data_root().join(format!("chips/{}.yaml", chip.to_ascii_uppercase()));
    let family = family_file(
        &fs::read_to_string(&chip_path).unwrap_or_else(|err| panic!("reading {}: {err}", chip_path.display())),
    );
    let family_path = data_root().join(format!("family/{family}"));
    let text =
        fs::read_to_string(&family_path).unwrap_or_else(|err| panic!("reading {}: {err}", family_path.display()));
    version_of(&text, kind).unwrap_or_else(|| panic!("family {family} has no `{kind}` register block"))
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
