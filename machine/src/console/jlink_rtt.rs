//
// Drop this, use rtt-target instead!
//

/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

/// A blocking output stream allowing data to be logged from the
/// target to the host.
/// Implements fmt::Write.
pub struct Output {}

impl Output {
    /// Create a blocking output stream
    #[inline]
    pub fn new() -> Self {
        unsafe {
            _SEGGER_RTT.init();
        }
        Self {}
    }
}

impl Drop for Output {
    fn drop(&mut self) {}
}

impl crate::devices::ConsoleOps for Output {
    fn puts(&self, s: &str) {
        unsafe {
            _SEGGER_RTT.up.write(s.as_bytes(), true);
        }
    }

    fn putc(&self, c: char) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.puts(s);
    }

    fn getc(&self) -> char {
        ' '
        // _SEGGER_RTT.down.read(true)
    }

    fn flush(&self) {} // @todo wait for write buffer to drain
}

impl fmt::Write for Output {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        use crate::devices::console::ConsoleOps;
        self.puts(s);
        Ok(())
    }
}

// This is probably not very useful...

/// A non-blocking output stream allowing data to be logged from the
/// target to the host.
/// Implements fmt::Write.
pub struct NonBlockingOutput {
    blocked: bool,
}

impl NonBlockingOutput {
    /// Create a non-blocking output stream
    #[inline]
    pub fn new() -> Self {
        unsafe {
            _SEGGER_RTT.init();
        }
        Self { blocked: false }
    }
}

impl fmt::Write for NonBlockingOutput {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if !self.blocked {
            unsafe {
                if !_SEGGER_RTT.up.write(s.as_bytes(), false) {
                    self.blocked = true;
                }
            }
        }
        Ok(())
    }
}
