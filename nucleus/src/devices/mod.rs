/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */
pub mod console;
pub mod serial;

pub use {
    console::{Console, ConsoleOps},
    serial::SerialOps,
};
