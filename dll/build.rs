fn main() {
    // Hand the export table with fixed ordinals to the GNU linker. Only the
    // cdylib gets it; the rlib (used by `cargo test`) has no export table.
    let def = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("xinput1_3.def");
    println!("cargo:rustc-cdylib-link-arg={}", def.display());
    println!("cargo:rerun-if-changed=xinput1_3.def");
}
