fn main() {
    // `compositor_v1.display_layout` installs `display_layout_v1`, which lives
    // beside it.
    let include = ["../interfaces"];
    libmeddle::build::interface_crate("../interfaces/compositor_v1.if", &include);
    libmeddle::build::interface_crate("../interfaces/display_layout_v1.if", &include);
}
