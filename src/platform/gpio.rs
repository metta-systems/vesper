use platform::rpi3::PERIPHERAL_BASE;
use register::mmio::*;

const GPIO_BASE: u32 = PERIPHERAL_BASE + 0x20_0000;

// The offsets for reach register.
// From https://wiki.osdev.org/Raspberry_Pi_Bare_Bones

//const GPFSEL0: u32 = GPIO_BASE + 0x00;
//const GPFSEL2: u32 = GPIO_BASE + 0x08;
//const GPFSEL3: u32 = GPIO_BASE + 0x0C;
//const GPFSEL4: u32 = GPIO_BASE + 0x10;
//const GPFSEL5: u32 = GPIO_BASE + 0x14;
//const GPSET0: u32 = GPIO_BASE + 0x1C;
//const GPSET1: u32 = GPIO_BASE + 0x20;
//const GPCLR0: u32 = GPIO_BASE + 0x28;
//const GPLEV0: u32 = GPIO_BASE + 0x34;
//const GPLEV1: u32 = GPIO_BASE + 0x38;
//const GPEDS0: u32 = GPIO_BASE + 0x40;
//const GPEDS1: u32 = GPIO_BASE + 0x44;
//const GPHEN0: u32 = GPIO_BASE + 0x64;
//const GPHEN1: u32 = GPIO_BASE + 0x68;
//
//const GPPUDCLK1: u32 = GPIO_BASE + 0x9C;

/*
 * MIT License
 *
 * Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
register_bitfields! {
    u32,

    /// GPIO Function Select 1
    GPFSEL1 [
        /// Pin 15
        FSEL15 OFFSET(15) NUMBITS(3) [
            Input = 0b000,
            Output = 0b001,
            RXD1 = 0b010  // Mini UART - Alternate function 5

        ],

        /// Pin 14
        FSEL14 OFFSET(12) NUMBITS(3) [
            Input = 0b000,
            Output = 0b001,
            TXD1 = 0b010  // Mini UART - Alternate function 5
        ]
    ],

    /// GPIO Pull-up/down Clock Register 0
    GPPUDCLK0 [
        /// Pin 15
        PUDCLK15 OFFSET(15) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ],

        /// Pin 14
        PUDCLK14 OFFSET(14) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ]
    ]
}

pub const GPFSEL1: *const ReadWrite<u32, GPFSEL1::Register> =
    (GPIO_BASE + 0x04) as *const ReadWrite<u32, GPFSEL1::Register>;

/// Controls actuation of pull up/down to ALL GPIO pins.
pub const GPPUD: *const ReadWrite<u32> = (GPIO_BASE + 0x94) as *const ReadWrite<u32>;

/// Controls actuation of pull up/down for specific GPIO pin.
pub const GPPUDCLK0: *const ReadWrite<u32, GPPUDCLK0::Register> =
    (GPIO_BASE + 0x98) as *const ReadWrite<u32, GPPUDCLK0::Register>;
