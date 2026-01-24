#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

pub mod console;

pub trait SerialOps {
    /// Read one byte from serial without translation.
    fn read_byte(&self) -> u8;
    /// Write one byte to serial without translation.
    fn write_byte(&self, byte: u8);
    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self);
    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self);
}

// main.rs
use liblog::{Level, Record, SetLoggerError};

struct ConsoleLogger;

impl liblog::Log for ConsoleLogger {
    fn enabled(&self, _level: Level) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        #[cfg(any(test, feature = "qemu"))]
        {
            let mut buf = [0_u8; 4096]; // Increase this buffer size to allow dumping larger panic texts.
            libqemu::semihosting::sys_write0_call(
                libprint::format_cstr(&mut buf, *record.args()).unwrap(),
            );
        }

        #[cfg(not(any(test, feature = "qemu")))]
        if self.enabled(record.level()) {
            //           let timestamp = libtime::_time();
            //           concat!("[  {:>3}.{:06}] ", $string),
            // -            timestamp.as_secs(),
            // -            timestamp.subsec_micros(),
            // -        ));

            crate::console::console().write_fmt(*record.args()).unwrap();
        }
    }

    fn flush(&self) {}
}

static LOGGER: ConsoleLogger = ConsoleLogger;

pub fn init_logger() -> Result<(), SetLoggerError> {
    liblog::set_logger(&LOGGER)?;
    liblog::set_max_level(Level::Info); // Allow up to Info by default
    Ok(())
}
