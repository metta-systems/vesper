/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

/// No-alloc write!() implementation from <https://stackoverflow.com/a/50201632/145434>
/// Requires you to allocate a buffer somewhere manually.
// @todo Try to use arrayvec::ArrayString here instead?
use core::{cmp::min, fmt};

pub struct WriteTo<'a> {
    buffer: &'a mut [u8],
    // on write error (i.e. not enough space in buffer) this grows beyond
    // `buffer.len()`.
    used: usize,
}

impl<'a> WriteTo<'a> {
    #[allow(unused)]
    pub fn new(buffer: &'a mut [u8]) -> Self {
        WriteTo { buffer, used: 0 }
    }

    #[allow(unused)]
    pub fn into_str(self) -> Option<&'a str> {
        (self.used <= self.buffer.len()).then(||
            // SAFETY: only successful concats of str - must be a valid str.
            unsafe { core::str::from_utf8_unchecked(&self.buffer[..self.used]) })
    }

    #[allow(unused)]
    pub fn into_cstr(self) -> Option<&'a str> {
        (self.used < self.buffer.len()).then(|| {
            self.buffer[self.used] = 0; // Terminate the string
            // SAFETY: only successful concats of str - must be a valid str.
            unsafe { core::str::from_utf8_unchecked(&self.buffer[..=self.used]) }
        })
    }
}

impl fmt::Write for WriteTo<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.used > self.buffer.len() {
            return Err(fmt::Error);
        }
        let remaining_buf = &mut self.buffer[self.used..];
        let raw_s = s.as_bytes();
        let write_num = min(raw_s.len(), remaining_buf.len());
        remaining_buf[..write_num].copy_from_slice(&raw_s[..write_num]);
        self.used += raw_s.len();
        if write_num < raw_s.len() {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

#[allow(unused)]
pub fn show<'a>(buffer: &'a mut [u8], args: fmt::Arguments) -> Result<&'a str, fmt::Error> {
    let mut w = WriteTo::new(buffer);
    fmt::write(&mut w, args)?;
    w.into_str().ok_or(fmt::Error)
}

// Return a zero-terminated str
#[allow(unused)]
pub fn c_show<'a>(buffer: &'a mut [u8], args: fmt::Arguments) -> Result<&'a str, fmt::Error> {
    let mut w = WriteTo::new(buffer);
    fmt::write(&mut w, args)?;
    w.into_cstr().ok_or(fmt::Error)
}
