//! Embed the nucleus and bundle the privileged components named in
//! `image.toml` (see `vesper-image-build`).

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    libimage_build::generate_kernel("image.toml");
    libimage_build::bundle_privileged("image.toml");
}
