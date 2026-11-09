// Build script: assemble entry.asm with NASM so its 'handoff_to_kernel'
// symbol is linked into the final UEFI PE32+ image.
//
// Only runs when building for a UEFI target; CI hosted builds skip it.

use std::process::Command;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("uefi") {
        return;
    }

    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let obj = format!("{}/entry.o", out);

    // 'win64' produces COFF x86_64, compatible with LLD's PE link step
    // used by the x86_64-unknown-uefi target.
    let status = Command::new("nasm")
        .args(["-f", "win64", "src/entry.asm", "-o", &obj])
        .status()
        .expect("nasm not found on PATH");

    assert!(status.success(), "nasm assembly of entry.asm failed");

    println!("cargo:rustc-link-search=native={}", out);
    println!("cargo:rerun-if-changed=src/entry.asm");
}
