//! This build script is used to create chainboot binary.

const LINKER_SCRIPT: &str = "bin/chainboot/src/link.ld";
const LINKER_SCRIPT_AUX: &str = "libs/exception/src/arch/aarch64/linker/aarch64-exceptions.ld";

fn main() {
    println!("cargo:rerun-if-env-changed=TARGET_BOARD");
    println!("cargo:rerun-if-changed={LINKER_SCRIPT}");
    println!("cargo:rerun-if-changed={LINKER_SCRIPT_AUX}");
}
