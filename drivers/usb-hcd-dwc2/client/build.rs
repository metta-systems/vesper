fn main() {
    // `usb_hcd_v1` installs `pipe_v1`, which lives in the same directory.
    let include = ["../interfaces"];
    libmeddle::build::client_crate("../interfaces/usb_hcd_v1.if", "usb_hcd", &include);
    libmeddle::build::client_crate("../interfaces/pipe_v1.if", "usb_hcd", &include);
}
