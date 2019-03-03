// Custom implementation of JLink RTT debug protocol
// jlink_rtt crate has too many strange bugs

/// This module implements a limited version of the Segger
/// Real Time Transfer protocol between the debugger host
/// and the target program.
/// RTT works by scanning memory to look for a control block
/// containing a magic string (it is also possible to tell
/// the monitor exactly where to find this block).
/// The control block defines a set of "up" channels
/// and "down" channels that are named pipes of communication
/// between the two systems.
/// Each of these channels is implemented as a simple
/// ring buffer.
/// The cost of logging data to RTT is the cost of formatting
/// and writing it to the ring buffer in memory.
use core::fmt;
use core::mem::size_of;
use core::ptr;
use static_assertions::const_assert_eq;

static mut UP_BUF: [u8; 1024] = [0u8; 1024];
static mut DOWN_BUF: [u8; 16] = [0u8; 16];

/// Ring buffer for communicating between target and host.
/// This must be binary compatible with the RTT implementation
/// in the JLINK device.
#[repr(C)]
struct Buffer {
    name: u32,      //*const u8,
    buf_start: u32, // *mut u8,
    size_of_buffer: u32,
    /// Position of next item to be written
    /// Volatile as the host may change it.
    write_offset: u32,
    /// Position of next item to be read by host.
    /// Volatile as the host may change it.
    read_offset: u32,
    /// In the segger library these flags control blocking
    /// or non-blocking behavior.
    flags: u32,
}

// Assumed by OpenOCD and probably JLink too...
const_assert_eq!(size_of::<Buffer>(), 24);

impl Buffer {
    fn init(&mut self, buf: &mut [u8]) {
        self.name = b"Terminal\0".as_ptr() as u32;
        self.buf_start = buf.as_mut_ptr() as u32;
        self.size_of_buffer = buf.len() as u32;
        self.write_offset = 0;
        self.read_offset = 0;
        self.flags = 0; // Non-blocking mode
    }

    fn get_read_offset(&self) -> u32 {
        unsafe { ptr::read_volatile(&self.read_offset as *const u32) }
    }

    #[allow(unused)]
    fn set_read_offset(&mut self, offset: u32) {
        unsafe {
            ptr::write_volatile(&mut self.read_offset as *mut u32, offset);
        }
    }

    fn get_write_offset(&self) -> u32 {
        unsafe { ptr::read_volatile(&self.write_offset as *const u32) }
    }

    fn set_write_offset(&mut self, offset: u32) {
        unsafe {
            ptr::write_volatile(&mut self.write_offset as *mut u32, offset);
        }
    }

    /// Write data to the ring buffer.
    /// Returns true if all of the data was written, which
    /// will always be the case if blocking==true.
    /// Returns false if blocking==false and the buffer was
    /// full.
    fn write(&mut self, buf: &[u8], blocking: bool) -> bool {
        let mut buf = buf;
        let mut write_off = self.get_write_offset() as usize;
        let size_of_buffer = self.size_of_buffer as usize;
        while buf.len() > 0 {
            let read_off = self.get_read_offset() as usize;

            let wrapping_capacity = if read_off > write_off {
                read_off - write_off - 1
            } else {
                size_of_buffer - (write_off - read_off + 1)
            };

            // If we're full and non-blocking, return now.
            // Otherwise, we'll spin with a series of 0 byte
            // length increments until the host consumes data
            // from the ring buffer.
            if wrapping_capacity == 0 && !blocking {
                return false;
            }

            let flat_capacity = size_of_buffer - write_off;

            let to_copy = buf.len().min(flat_capacity).min(wrapping_capacity);

            unsafe {
                ptr::copy(
                    buf.as_ptr(),
                    (self.buf_start as *mut u8).offset(write_off as isize),
                    to_copy,
                );
            }

            write_off += to_copy;
            if write_off == size_of_buffer {
                write_off = 0;
            }
            self.set_write_offset(write_off as u32);

            buf = &buf[to_copy..];
        }

        true
    }
}

/// The ControlBlock is the magic struct that the JLINK looks
/// for to discover the ring buffers.
#[repr(C)]
pub struct ControlBlock {
    /// Initialized to "SEGGER RTT"
    id: [u8; 16],
    /// Initialized to NUM_UP
    max_up_buffers: i32,
    /// Initialized to NUM_DOWN
    max_down_buffers: i32,
    /// Note that RTT allows for this to be an array of
    /// "up" buffers of size max_up_buffers, but for simplicity
    /// just a single buffer is implemented here.
    up: Buffer,
    /// Note that RTT allows for this to be an array of
    /// "down" buffers of size max_down_buffers, but for simplicity
    /// just a single buffer is implemented here.
    down: Buffer,
}

const_assert_eq!(size_of::<ControlBlock>(), 24 + 24 + 24);

unsafe impl Sync for ControlBlock {}

impl ControlBlock {
    fn init(&mut self) {
        if self.id[0] == b'S' {
            return;
        }

        // Unsafe: use of mutable static
        // mutable statics can be mutated by multiple threads: aliasing violations
        // or data races will cause undefined behavior
        unsafe {
            self.up.init(&mut UP_BUF);
            self.down.init(&mut DOWN_BUF);
        }

        // Compose the ident string such that we won't
        // emit the string sequence in flash
        self.id.copy_from_slice(b"_EGGER:RTT\0\0\0\0\0\0");
        self.id[0] = b'S';
        self.id[6] = b' ';
    }
}

#[no_mangle]
pub static mut _SEGGER_RTT: ControlBlock = ControlBlock {
    id: [0u8; 16],
    max_up_buffers: 1,
    max_down_buffers: 1,
    up: Buffer {
        name: 0,
        buf_start: 0,
        read_offset: 0,
        write_offset: 0,
        flags: 0,
        size_of_buffer: 0,
    },
    down: Buffer {
        name: 0,
        buf_start: 0,
        write_offset: 0,
        read_offset: 0,
        flags: 0,
        size_of_buffer: 0,
    },
};

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

impl fmt::Write for Output {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            _SEGGER_RTT.up.write(s.as_bytes(), true);
        }
        Ok(())
    }
}

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
        Self { blocked: false }
    }
}

impl fmt::Write for NonBlockingOutput {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if !self.blocked {
            unsafe {
                _SEGGER_RTT.init();
                if !_SEGGER_RTT.up.write(s.as_bytes(), false) {
                    self.blocked = true;
                }
            }
        }
        Ok(())
    }
}
