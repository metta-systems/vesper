//! The engine's log on Wood's console: each record of the `log` crate as a
//! line on the semihosting console under QEMU. Wood has no serial channel
//! (the compositor holds the PL011), so without `qemu` the records go
//! nowhere.

use log::{LevelFilter, Log, Metadata, Record};

/// The logger Wood installs.
struct Console;

impl Log for Console {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        cfg!(feature = "qemu")
    }

    // Without `qemu`, semihosting printing compiles away and `record` is unused.
    #[cfg_attr(not(feature = "qemu"), allow(unused_variables))]
    fn log(&self, record: &Record<'_>) {
        // The engine ends some messages with a line feed of their own.
        libuser::semihosting::println!(
            "wood: {} {}",
            record.level(),
            DropTrailingNewline(record.args())
        );
    }

    fn flush(&self) {}
}

/// Formats its arguments without the line feeds they end with.
#[cfg_attr(not(feature = "qemu"), allow(dead_code))]
struct DropTrailingNewline<'record>(&'record core::fmt::Arguments<'record>);

impl core::fmt::Display for DropTrailingNewline<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0.as_str() {
            Some(text) => formatter.write_str(text.trim_end_matches('\n')),
            None => self.0.fmt(formatter),
        }
    }
}

static CONSOLE: Console = Console;

/// Send the `log` crate's records to the console, from `Info` up.
pub fn install() {
    // Only fails if a logger is set already, which leaves that one working.
    let _installed = log::set_logger(&CONSOLE);
    log::set_max_level(LevelFilter::Info);
}
