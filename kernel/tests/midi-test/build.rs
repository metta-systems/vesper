//! Bundle the components listed in `image.toml` (see `vesper-image-build`);
//! with the `fp-mode` feature, the synthesiser with the f64 engine,
//! `midi-synth-fp`, in place of `midi-synth` (`just _test-midi-fp`).

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_FP_MODE").is_some() {
        libimage_build::bundle_components_replacing(
            "image.toml",
            &[("midi-synth", "midi-synth-fp")],
        );
    } else {
        libimage_build::bundle_components("image.toml");
    }
}
