//! The engine's log: each record of the `log` crate as a line on Wood's
//! `log_v1` channel to the log server (`liblog-client`), which writes it to the
//! UART, and on the semihosting console under QEMU (`wood: INFO text`).
//! Logging never waits, so the playback Thread logs too: a line that finds no
//! buffer free is dropped and counted.

use {liblog_client::Logger, log::LevelFilter};

/// The logger Wood installs.
static LOGGER: Logger = Logger::new("wood");

/// Send the `log` crate's records to the log, from `Info` up.
pub fn install() {
    LOGGER.install(LevelFilter::Info);
}
