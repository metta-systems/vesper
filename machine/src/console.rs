/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

#![allow(dead_code)]

use crate::sync::NullLock;

pub mod null_console;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Console interfaces.
pub mod interface {
    use {crate::devices::SerialOps, core::fmt};

    /// Console write functions.
    pub trait Write {
        /// Write a Rust format string.
        fn write_fmt(&self, args: fmt::Arguments) -> fmt::Result;
    }

    /// A trait that must be implemented by devices that are candidates for the
    /// global console.
    #[allow(unused_variables)]
    pub trait ConsoleOps: SerialOps {
        /// Send a character
        fn write_char(&self, c: char);
        /// Display a string
        fn write_string(&self, string: &str);
        /// Receive a character
        fn read_char(&self) -> char;
    }

    pub trait ConsoleTools {
        fn command_prompt<'a>(&self, buf: &'a mut [u8]) -> &'a [u8];
    }

    /// Trait alias for a full-fledged console.
    pub trait All: Write + ConsoleOps + ConsoleTools {}
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static CONSOLE: NullLock<&'static (dyn interface::All + Sync)> =
    NullLock::new(&null_console::NULL_CONSOLE);

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

use crate::sync::interface::Mutex;

/// Register a new console.
pub fn register_console(new_console: &'static (dyn interface::All + Sync)) {
    CONSOLE.lock(|con| *con = new_console);
}

/// Return a reference to the currently registered console.
///
/// This is the global console used by all printing macros.
pub fn console() -> &'static dyn interface::All {
    CONSOLE.lock(|con| *con)
}