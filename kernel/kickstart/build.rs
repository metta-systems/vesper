//! Embed the nucleus named in `image.toml`: extract its sections and generate
//! the `KERNEL` image description (see `vesper-image-build`).

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    libimage_build::generate_kernel("image.toml");
}
