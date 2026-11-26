// Include generated types
include!(concat!(env!("OUT_DIR"), "/kernel_sections.rs"));

/// Privileged components bundled from `image.toml` (`PRIVILEGED`).
pub mod privileged {
    include!(concat!(env!("OUT_DIR"), "/privileged.rs"));
}

/// Start-method components bundled from `image.toml` (`CPU_START`).
pub mod cpu_start {
    include!(concat!(env!("OUT_DIR"), "/cpu_start.rs"));
}
