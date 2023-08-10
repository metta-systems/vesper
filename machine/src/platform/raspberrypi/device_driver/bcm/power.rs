/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

use {
    super::{
        device_driver::gpio,
        mailbox::{channel, Mailbox, MailboxOps},
        BcmHost,
    },
    crate::{
        memory::{Address, Virtual},
        platform::device_driver::common::MMIODerefWrapper,
    },
    snafu::Snafu,
    tock_registers::{
        interfaces::{Readable, Writeable},
        register_structs,
        registers::ReadWrite,
    },
};

register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => __reserved_1),
        (0x1c => PM_RSTC: ReadWrite<u32>),
        (0x20 => PM_RSTS: ReadWrite<u32>),
        (0x24 => PM_WDOG: ReadWrite<u32>),
        (0x28 => @END),
    }
}

const PM_PASSWORD: u32 = 0x5a00_0000;
const PM_RSTC_WRCFG_CLR: u32 = 0xffff_ffcf;
const PM_RSTC_WRCFG_FULL_RESET: u32 = 0x0000_0020;

// The Raspberry Pi firmware uses the RSTS register to know which
// partition to boot from. The partition value is spread into bits 0, 2,
// 4, 6, 8, 10. Partition 63 is a special partition used by the
// firmware to indicate halt.
const PM_RSTS_RASPBERRYPI_HALT: u32 = 0x555;

const POWER_STATE_OFF: u32 = 0;
const POWER_STATE_ON: u32 = 1;
const POWER_STATE_DO_NOT_WAIT: u32 = 0;
const POWER_STATE_WAIT: u32 = 2;

#[derive(Debug, Snafu)]
pub enum PowerError {
    #[snafu(display("Power setup failed in mailbox operation"))]
    MailboxError,
}

pub type Result<T> = ::core::result::Result<T, PowerError>;

type Registers = MMIODerefWrapper<RegisterBlock>;

/// Public interface to the Power subsystem
pub struct Power {
    registers: Registers,
}

impl Power {
    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(mmio_base_addr: Address<Virtual>) -> Power {
        Power {
            registers: Registers::new(mmio_base_addr),
        }
    }

    /// Shutdown the board
    pub fn off(&self, gpio: &gpio::GPIO) -> Result<()> {
        // power off devices one by one
        for dev_id in 0..16 {
            let mut mbox = Mailbox::<8>::default();
            let index = mbox.request();
            let index =
                mbox.set_device_power(index, dev_id, POWER_STATE_OFF | POWER_STATE_DO_NOT_WAIT);
            let mbox = mbox.end(index);

            mbox.call(channel::PropertyTagsArmToVc)
                .map_err(|_| PowerError::MailboxError)?;
        }

        gpio.power_off();

        // We set the watchdog hard reset bit here to distinguish this
        // reset from the normal (full) reset. bootcode.bin will not
        // reboot after a hard reset.
        let mut val = self.registers.PM_RSTS.get();
        val |= PM_PASSWORD | PM_RSTS_RASPBERRYPI_HALT;
        self.registers.PM_RSTS.set(val);

        // Continue with normal reset mechanism
        self.reset();
    }

    /// Reboot
    pub fn reset(&self) -> ! {
        // use a timeout of 10 ticks (~150us)
        self.registers.PM_WDOG.set(PM_PASSWORD | 10);
        let mut val = self.registers.PM_RSTC.get();
        val &= PM_RSTC_WRCFG_CLR;
        val |= PM_PASSWORD | PM_RSTC_WRCFG_FULL_RESET;
        self.registers.PM_RSTC.set(val);

        crate::cpu::endless_sleep()
    }
}
