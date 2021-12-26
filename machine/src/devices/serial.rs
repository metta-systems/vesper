pub trait SerialOps {
    /// Read one byte from serial without translation.
    fn read_byte(&self) -> u8 {
        0
    }
    /// Write one byte to serial without translation.
    fn write_byte(&self, _byte: u8) {}
    /// Wait until the TX FIFO is empty, aka all characters have been put on the
    /// line.
    fn flush(&self) {}
    /// Consume input until RX FIFO is empty, aka all pending characters have been
    /// consumed.
    fn clear_rx(&self) {}
}
