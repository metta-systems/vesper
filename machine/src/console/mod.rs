/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

#![allow(dead_code)]

pub mod null_console;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Console interfaces.
pub mod interface {
    use {crate::devices::serial::SerialOps, core::fmt};

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
        fn write_char(&self, c: char) {
            let mut bytes = [0u8; 4];
            let _ = c.encode_utf8(&mut bytes);
            for &b in bytes.iter().take(c.len_utf8()) {
                self.write_byte(b);
            }
        }
        /// Display a string
        fn write_string(&self, string: &str) {
            for c in string.chars() {
                // convert newline to carriage return + newline
                if c == '\n' {
                    self.write_char('\r')
                }

                self.write_char(c);
            }
        }
        /// Receive a character -- FIXME: needs a state machine to read UTF-8 chars!
        fn read_char(&self) -> char {
            let mut ret = self.read_byte() as char;

            // convert carriage return to newline
            if ret == '\r' {
                ret = '\n'
            }

            ret
        }
    }

    /// Trait alias for a full-fledged console.
    pub trait All: Write + ConsoleOps {}
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static CONSOLE: InitStateLock<&'static (dyn interface::All + Sync)> =
    InitStateLock::new(&null_console::NULL_CONSOLE);

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

use crate::synchronization::{interface::ReadWriteEx, InitStateLock};

/// Register a new console.
pub fn register_console(new_console: &'static (dyn interface::All + Sync)) {
    CONSOLE.write(|con| *con = new_console);
}

/// Return a reference to the currently registered console.
///
/// This is the global console used by all printing macros.
pub fn console() -> &'static dyn interface::All {
    CONSOLE.read(|con| *con)
}

/// A command prompt.
pub fn command_prompt(buf: &mut [u8]) -> &[u8] {
    use interface::ConsoleOps;

    console().write_string("\n$> ");

    let mut i = 0;
    let mut input;
    loop {
        input = console().read_char();

        if input == '\n' {
            console().write_char('\n'); // do \r\n output
            return &buf[..i];
        } else {
            if i < buf.len() {
                buf[i] = input as u8;
                i += 1;
            } else {
                return &buf[..i];
            }

            console().write_char(input);
        }
    }
}
