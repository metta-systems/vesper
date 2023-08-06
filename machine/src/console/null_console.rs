use crate::{console::interface, devices::serial::SerialOps};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// A dummy console that just ignores all I/O.
pub struct NullConsole;

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

pub static NULL_CONSOLE: NullConsole = NullConsole {};

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl interface::Write for NullConsole {
    fn write_fmt(&self, args: core::fmt::Arguments) -> core::fmt::Result {
        Ok(())
    }
}

impl interface::ConsoleOps for NullConsole {
    fn write_char(&self, _c: char) {}

    fn write_string(&self, _string: &str) {}

    fn read_char(&self) -> char {
        ' '
    }
}

impl SerialOps for NullConsole {
    fn read_byte(&self) -> u8 {
        0
    }

    fn write_byte(&self, _byte: u8) {}

    fn flush(&self) {}

    fn clear_rx(&self) {}
}

impl interface::All for NullConsole {}
