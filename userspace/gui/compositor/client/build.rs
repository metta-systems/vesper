fn main() {
    let include = ["../interfaces"];
    libmeddle::build::client_crate(
        "../interfaces/compositor_v1.if",
        "compositor_server",
        &include,
    );
    libmeddle::build::client_crate(
        "../interfaces/display_layout_v1.if",
        "compositor_server",
        &include,
    );
}
