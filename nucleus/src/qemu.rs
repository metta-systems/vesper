/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
use crate::devices::{ConsoleOps, SerialOps};

pub mod semihosting {
    pub fn exit_success() -> ! {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_success()
    }

    pub fn exit_failure() -> ! {
        use qemu_exit::QEMUExit;

        #[cfg(target_arch = "aarch64")]
        let qemu_exit_handle = qemu_exit::AArch64::new();

        qemu_exit_handle.exit_failure()
    }

    pub fn sys_write0_call(text: &str) {
        // SAFETY: text must be \0-terminated!
        let cmd = 0x04;
        unsafe {
            core::arch::asm!(
                "hlt #0xF000"
                , in("w0") cmd
                , in("x1") text.as_ptr() as u64
            );
        }
    }
}

pub struct QemuConsole;

impl SerialOps for QemuConsole {
    fn read_byte(&self) -> u8 {
        0
    }

    fn write_byte(&self, byte: u8) {
        unsafe {
            core::ptr::write_volatile(0x3F20_1000 as *mut u8, byte);
        }
    }

    fn flush(&self) {}

    fn clear_rx(&self) {}
}

impl ConsoleOps for QemuConsole {
    fn write_char(&self, c: char) {
        self.write_byte(c as u8);
    }

    fn write_string(&self, string: &str) {
        for c in string.chars() {
            // convert newline to carriage return + newline
            if c == '\n' {
                self.write_char('\r')
            }

            self.write_char(c);
        }
    }

    fn read_char(&self) -> char {
        let mut ret = self.read_byte() as char;

        // convert carriage return to newline
        if ret == '\r' {
            ret = '\n'
        }

        ret
    }
}
