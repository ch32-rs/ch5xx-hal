fn main() {
    // The board's RAMX/ROM split lives in this crate's `memory.x`, so the
    // linker has to see this directory before the HAL's generated one.
    println!("cargo:rustc-link-search={}", env!("CARGO_MANIFEST_DIR"));
    // Order matters. `link.x` places orphan sections at `ORIGIN(FLASH)`, so
    // `defmt.x` — which assigns `.defmt` to a range past the vector table —
    // must be processed *after* it. Passing `-Tdefmt.x` via `rustflags` puts it
    // first and silently lands `.defmt` at 0x0, on top of the vector table.
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
