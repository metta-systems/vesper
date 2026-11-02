//! Bundle the components listed in `image.toml` (see `vesper-image-build`).

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    libimage_build::bundle_components("image.toml");
}
