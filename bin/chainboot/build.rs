/// This build script is used to link chainboot binary.

const LINKER_SCRIPT: &str = "bin/chainboot/src/link.ld";
const LINKER_SCRIPT_AUX: &str = "machine/src/arch/aarch64/linker/aarch64-exceptions.ld";

fn main() {
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT);
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT_AUX);
    println!("cargo:rustc-link-arg=--script={}", LINKER_SCRIPT);
}
