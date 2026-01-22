fn main() {
    build_rs::output::rustc_link_arg(
        format!("--script={}/kernel.ld", env!("CARGO_MANIFEST_DIR")).as_ref(),
    );
}
