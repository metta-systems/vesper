const LINKER_SCRIPT: &str = "linker/aarch64.ld";
const LINKER_SCRIPT_AUX: &str = "linker/aarch64-exceptions.ld";

fn main() {
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT);
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT_AUX);
    println!("cargo:rustc-link-arg=--script={}", LINKER_SCRIPT);
}
