/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
cfg_if::cfg_if! {
    if #[cfg(target_arch = "aarch64")] {
        #[macro_use]
        pub mod aarch64;
        pub use self::aarch64::*;
    }
    if #[cfg(target_arch = "x86_64")] {
        #[macro_use]
        pub mod x86_64;
        pub use self::x86_64::*;
    }
}
