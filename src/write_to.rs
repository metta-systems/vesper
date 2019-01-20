// No-alloc write!() implementation from https://stackoverflow.com/a/50201632/145434
// Requires you to allocate a buffer somewhere manually.
//

use core::cmp::min;
use core::fmt;

pub struct WriteTo<'a> {
    buffer: &'a mut [u8],
    // on write error (i.e. not enough space in buffer) this grows beyond
    // `buffer.len()`.
    used: usize,
}

impl<'a> WriteTo<'a> {
    pub fn new(buffer: &'a mut [u8]) -> Self {
        WriteTo { buffer, used: 0 }
    }

    pub fn as_str(self) -> Option<&'a str> {
        if self.used <= self.buffer.len() {
            // only successful concats of str - must be a valid str.
            use core::str::from_utf8_unchecked;
            Some(unsafe { from_utf8_unchecked(&self.buffer[..self.used]) })
        } else {
            None
        }
    }
}

impl<'a> fmt::Write for WriteTo<'a> {
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

pub fn show<'a>(buffer: &'a mut [u8], args: fmt::Arguments) -> Result<&'a str, fmt::Error> {
    let mut w = WriteTo::new(buffer);
    fmt::write(&mut w, args)?;
    w.as_str().ok_or(fmt::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test() {
        let mut buf = [0u8; 64];
        let s: &str = show(
            &mut buf,
            format_args!("write some stuff {:?}: {}", "foo", 42),
        )
        .unwrap();
        assert_eq!(s, "write some stuff foo: 42");
    }
}
