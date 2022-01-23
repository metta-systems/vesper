const LINKER_SCRIPT: &str = "bin/chainboot/src/link.ld";

fn main() {
    println!("cargo:rerun-if-changed={}", LINKER_SCRIPT);
    println!("cargo:rustc-link-arg=--script={}", LINKER_SCRIPT);
}
