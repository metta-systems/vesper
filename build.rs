const LINKER_SCRIPT: &str = "linker/aarch64.ld";

fn main() {
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT);
    println!("cargo:rustc-link-arg=--script={}", LINKER_SCRIPT);
}
