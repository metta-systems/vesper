/// This build script is used to create lib tests.

const LINKER_SCRIPT: &str = "machine/src/platform/raspberrypi/linker/kernel.ld";
const LINKER_SCRIPT_AUX: &str = "machine/src/arch/aarch64/linker/aarch64-exceptions.ld";

fn main() {
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT);
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT_AUX);
    println!("cargo:rustc-link-arg=--script={}", LINKER_SCRIPT);
}
