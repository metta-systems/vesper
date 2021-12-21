/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

#![allow(dead_code)]

use {crate::platform, core::fmt};

/// A trait that must be implemented by devices that are candidates for the
/// global console.
#[allow(unused_variables)]
pub trait ConsoleOps {
    /// Send a character
    fn write_char(&self, c: char) {}
    /// Display a string
    fn write_string(&self, string: &str) {}
    /// Receive a character
    fn read_char(&self) -> char {
        ' '
    }
    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self) {}
    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self) {}
}

/// A dummy console that just ignores its inputs.
pub struct NullConsole;

impl Drop for NullConsole {
    fn drop(&mut self) {}
}

impl ConsoleOps for NullConsole {}

/// Possible outputs which the console can store.
pub enum Output {
    None(NullConsole),
    MiniUart(platform::rpi3::mini_uart::PreparedMiniUart),
    Uart(platform::rpi3::pl011_uart::PreparedPL011Uart),
}

/// Generate boilerplate for converting into one of Output enum values
macro output_from($name:ty, $optname:ident) {
    impl From<$name> for Output {
        fn from(instance: $name) -> Self {
            Output::$optname(instance)
        }
    }
}

output_from!(NullConsole, None);
output_from!(platform::rpi3::mini_uart::PreparedMiniUart, MiniUart);
output_from!(platform::rpi3::pl011_uart::PreparedPL011Uart, Uart);

pub struct Console {
    output: Output,
}

impl Default for Console {
    fn default() -> Self {
        Console {
            output: (NullConsole {}).into(),
        }
    }
}

impl Console {
    pub const fn new() -> Console {
        Console {
            output: Output::None(NullConsole {}),
        }
    }

    #[inline(always)]
    fn current_ptr(&self) -> &dyn ConsoleOps {
        match &self.output {
            Output::None(i) => i,
            Output::MiniUart(i) => i,
            Output::Uart(i) => i,
        }
    }

    /// Overwrite the current output. The old output will go out of scope and
    /// it's Drop function will be called.
    pub fn replace_with(&mut self, x: Output) {
        self.current_ptr().flush();

        self.output = x;
    }

    /// A command prompt.
    pub fn command_prompt<'a>(&self, buf: &'a mut [u8]) -> &'a [u8] {
        self.write_string("\n$> ");

        let mut i = 0;
        let mut input;
        loop {
            input = self.read_char();

            if input == '\n' {
                self.write_char('\n'); // do \r\n output
                return &buf[..i];
            } else {
                if i < buf.len() {
                    buf[i] = input as u8;
                    i += 1;
                } else {
                    return &buf[..i];
                }

                self.write_char(input);
            }
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {}
}

/// Dispatch the respective function to the currently stored output device.
impl ConsoleOps for Console {
    fn write_char(&self, c: char) {
        self.current_ptr().write_char(c);
    }

    fn write_string(&self, string: &str) {
        self.current_ptr().write_string(string);
    }

    fn read_char(&self) -> char {
        self.current_ptr().read_char()
    }

    fn flush(&self) {
        self.current_ptr().flush()
    }
}

/// Implementing this trait enables usage of the format_args! macros, which in
/// turn are used to implement the kernel's print! and println! macros.
///
/// See src/macros.rs.
impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.current_ptr().write_string(s);
        Ok(())
    }
}
