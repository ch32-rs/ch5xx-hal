fn main() {
    // Order matters. `link.x` places orphan sections at `ORIGIN(FLASH)`, so
    // `defmt.x` — which assigns `.defmt` to a range past the vector table —
    // must be processed *after* it. Passing `-Tdefmt.x` via `rustflags` puts it
    // first and silently lands `.defmt` at 0x0, on top of the vector table.
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    println!("cargo:rustc-link-arg-bin=timer_acceptance=-Ttimer-acceptance.x");
    println!("cargo:rerun-if-changed=timer-acceptance.x");

}
