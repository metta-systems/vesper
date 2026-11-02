fn main() {
    // `usb_hcd_v1` installs `pipe_v1`, which lives in the same directory.
    let include = ["../interfaces"];
    libmeddle::build::interface_crate("../interfaces/usb_hcd_v1.if", &include);
    libmeddle::build::interface_crate("../interfaces/pipe_v1.if", &include);
}
