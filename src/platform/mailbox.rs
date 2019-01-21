use crate::arch::*;
use crate::platform::{
    display::Size2d,
    rpi3::{phys2bus, PERIPHERAL_BASE},
    // uart::MiniUart,
};
use core::ops::Deref;
use register::mmio::*;

// Public interface to the mailbox
#[repr(C)]
#[repr(align(16))]
pub struct Mailbox {
    // The address for the buffer needs to be 16-byte aligned
    // so that the VideoCore can handle it properly.
    pub buffer: [u32; 36],
}

// Identity mapped first 1Gb by u-boot
const MAILBOX_BASE: u32 = PERIPHERAL_BASE + 0xb880;
/* Lower 4-bits are channel ID */
const CHANNEL_MASK: u32 = 0xf;

// Mailbox Peek  Read/Write  Status  Sender  Config
//    0    0x10  0x00        0x18    0x14    0x1c
//    1    0x30  0x20        0x38    0x34    0x3c
//
// Only mailbox 0's status can trigger interrupts on the ARM, so Mailbox 0 is
// always for communication from VC to ARM and Mailbox 1 is for ARM to VC.
//
// The ARM should never write Mailbox 0 or read Mailbox 1.

// Based on https://github.com/rust-embedded/rust-raspi3-tutorial/blob/master/04_mailboxes/src/mbox.rs
// by Andre Richter of Tock OS.

register_bitfields! {
    u32,

    STATUS [
        /* Bit 31 set in status register if the write mailbox is full */
        FULL  OFFSET(31) NUMBITS(1) [],
        /* Bit 30 set in status register if the read mailbox is empty */
        EMPTY OFFSET(30) NUMBITS(1) []
    ]
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    READ: ReadOnly<u32>,    // 0x00  This is Mailbox0 read for ARM, can't write
    __reserved_0: [u32; 5], // 0x04
    STATUS: ReadOnly<u32, STATUS::Register>, // 0x18
    __reserved_1: u32,      // 0x1C
    WRITE: WriteOnly<u32>,  // 0x20  This is Mailbox1 write for ARM, can't read
}

pub enum MboxError {
    ResponseError,
    UnknownError,
    Timeout,
}

pub type Result<T> = ::core::result::Result<T, MboxError>;

/*
 * Source https://elinux.org/RPi_Framebuffer
 * Source for channels 8 and 9: https://github.com/raspberrypi/firmware/wiki/Mailboxes
 */
#[allow(non_upper_case_globals)]
pub mod channel {
    pub const Power: u32 = 0;
    pub const FrameBuffer: u32 = 1;
    pub const VirtualUart: u32 = 2;
    pub const VChiq: u32 = 3;
    pub const Leds: u32 = 4;
    pub const Buttons: u32 = 5;
    pub const TouchScreen: u32 = 6;
    // Count = 7,
    pub const PropertyTagsArmToVc: u32 = 8;
    pub const PropertyTagsVcToArm: u32 = 9;
}

// FrameBuffer channel supported structure - use with channel::FrameBuffer
#[repr(C)]
#[repr(align(16))]
pub struct GpuFb {
    pub width: u32,
    pub height: u32,
    pub vwidth: u32,
    pub vheight: u32,
    pub pitch: u32,
    pub depth: u32,
    pub x_offset: u32,
    pub y_offset: u32,
    pub pointer: u32,
    pub size: u32,
}

pub const REQUEST: u32 = 0;

// Responses
pub mod response {
    pub const SUCCESS: u32 = 0x8000_0000;
    pub const ERROR: u32 = 0x8000_0001; // error parsing request buffer (partial response)
    /** When responding, the VC sets this bit in val_len to indicate a response */
    pub const VAL_LEN_FLAG: u32 = 0x8000_0000;
}

#[allow(non_upper_case_globals)]
pub mod tag {
    pub const GetBoardRev: u32 = 0x0001_0002;
    pub const GetMacAddress: u32 = 0x0001_0003;
    pub const GetBoardSerial: u32 = 0x0001_0004;
    pub const GetArmMemory: u32 = 0x0001_0005;
    pub const GetPowerState: u32 = 0x0002_0001;
    pub const SetPowerState: u32 = 0x0002_8001;
    pub const GetClockRate: u32 = 0x0003_0002;
    pub const AllocateBuffer: u32 = 0x0004_0001;
    pub const ReleaseBuffer: u32 = 0x0004_8001;
    pub const BlankScreen: u32 = 0x0004_0002;
    /* Physical means output signal */
    pub const GetPhysicalWH: u32 = 0x0004_0003;
    pub const TestPhysicalWH: u32 = 0x0004_4003;
    pub const SetPhysicalWH: u32 = 0x0004_8003;
    /* Virtual means display buffer */
    pub const GetVirtualWH: u32 = 0x0004_0004;
    pub const TestVirtualWH: u32 = 0x0004_4004;
    pub const SetVirtualWH: u32 = 0x0004_8004;
    pub const GetDepth: u32 = 0x0004_0005;
    pub const TestDepth: u32 = 0x0004_4005;
    pub const SetDepth: u32 = 0x0004_8005;
    pub const GetPixelOrder: u32 = 0x0004_0006;
    pub const TestPixelOrder: u32 = 0x0004_4006;
    pub const SetPixelOrder: u32 = 0x0004_8006;
    pub const GetAlphaMode: u32 = 0x0004_0007;
    pub const TestAlphaMode: u32 = 0x0004_4007;
    pub const SetAlphaMode: u32 = 0x0004_8007;
    pub const GetPitch: u32 = 0x0004_0008;
    /* Offset of display window within buffer */
    pub const GetVirtualOffset: u32 = 0x0004_0009;
    pub const TestVirtualOffset: u32 = 0x0004_4009;
    pub const SetVirtualOffset: u32 = 0x0004_8009;
    pub const GetOverscan: u32 = 0x0004_000a;
    pub const TestOverscan: u32 = 0x0004_400a;
    pub const SetOverscan: u32 = 0x0004_800a;
    pub const GetPalette: u32 = 0x0004_000b;
    pub const TestPalette: u32 = 0x0004_400b;
    pub const SetPalette: u32 = 0x0004_800b;
    pub const End: u32 = 0;
}

pub mod power {
    pub const SDHCI: u32 = 0;
    pub const UART0: u32 = 1;
    pub const UART1: u32 = 2;
    pub const USB_HCD: u32 = 3;
    pub const I2C0: u32 = 4;
    pub const I2C1: u32 = 5;
    pub const I2C2: u32 = 6;
    pub const SPI: u32 = 7;
    pub const CCP2TX: u32 = 8;

    pub mod response {
        pub const ON: u32 = 1;
        pub const NO_DEV: u32 = 2; /* Device doesn't exist */
    }
    pub mod request {
        pub const ON: u32 = 1;
        pub const WAIT: u32 = 2;
    }
}

pub mod clock {
    pub const EMMC: u32 = 1;
    pub const UART: u32 = 2;
    pub const ARM: u32 = 3;
    pub const CORE: u32 = 4;
    pub const V3D: u32 = 5;
    pub const H264: u32 = 6;
    pub const ISP: u32 = 7;
    pub const SDRAM: u32 = 8;
    pub const PIXEL: u32 = 9;
    pub const PWM: u32 = 10;
}

pub mod alpha_mode {
    pub const OPAQUE_0: u32 = 0; // 255 - transparent
    pub const TRANSPARENT_0: u32 = 1; // 255 - opaque
    pub const IGNORED: u32 = 2;
}

fn write(regs: &RegisterBlock, buf_ptr: u32, channel: u32) -> Result<()> {
    let mut count: u32 = 0;

    // {
    //     let mut uart = MiniUart::new();
    //     uart.init();
    //     writeln!(uart, "Mailbox::write {:x}/{:x}", buf_ptr, channel);
    // }

    while regs.STATUS.is_set(STATUS::FULL) {
        count += 1;
        if count > (1 << 25) {
            return Err(MboxError::Timeout);
        }
    }
    dmb();
    regs.WRITE
        .set(phys2bus(buf_ptr & !CHANNEL_MASK) | (channel & CHANNEL_MASK));
    Ok(())
}

fn read(regs: &RegisterBlock, expected: u32, channel: u32) -> Result<()> {
    let mut count: u32 = 0;

    // let mut uart = MiniUart::new();
    // uart.init();

    loop {
        while regs.STATUS.is_set(STATUS::EMPTY) {
            count += 1;
            if count > (1 << 25) {
                return Err(MboxError::Timeout);
            }
        }

        /* Read the data
         * Data memory barriers as we've switched peripheral
         */
        dmb();
        let data: u32 = regs.READ.get();
        dmb();

        // is it a response to our message?
        if ((data & CHANNEL_MASK) == channel) && ((data & !CHANNEL_MASK) == expected) {
            // is it a valid successful response?
            return Ok(());
        } else {
            return Err(MboxError::ResponseError); //@fixme ignore invalid responses and loop again?
        }
    }
}

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.STATUS.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*Mbox::ptr()).STATUS.read() }
/// ```
impl Deref for Mailbox {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*Self::ptr() }
    }
}

impl core::fmt::Display for Mailbox {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let count = self.buffer[0] / 4;
        assert_eq!(self.buffer[0], count * 4);
        assert!(count <= 36);
        for i in 0usize..count as usize {
            writeln!(f, "[{:02}] {:08x}", i, self.buffer[i]);
        }
        Ok(())
    }
}

impl Mailbox {
    pub fn new() -> Mailbox {
        Mailbox { buffer: [0; 36] }
    }

    /// Returns a pointer to the register block
    fn ptr() -> *const RegisterBlock {
        MAILBOX_BASE as *const _
    }

    pub fn write(&self, channel: u32) -> Result<()> {
        write(self, self.buffer.as_ptr() as u32, channel)
    }

    pub fn read(&self, channel: u32) -> Result<()> {
        read(self, phys2bus(self.buffer.as_ptr() as u32), channel)?;

        // let mut uart = MiniUart::new();
        // uart.init();

        match self.buffer[1] {
            response::SUCCESS => {
                // writeln!(uart, "\n######\nMailbox::returning SUCCESS");
                Ok(())
            }
            response::ERROR => {
                // writeln!(uart, "\n######\nMailbox::returning ResponseError");
                Err(MboxError::ResponseError)
            }
            _ => {
                // writeln!(uart, "\n######\nMailbox::returning UnknownError");
                Err(MboxError::UnknownError)
            }
        }
    }

    pub fn call(&self, channel: u32) -> Result<()> {
        self.write(channel)?;
        self.read(channel)
    }
}

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.STATUS.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*Mbox::ptr()).STATUS.read() }
/// ```
impl Deref for GpuFb {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*Self::ptr() }
    }
}

impl core::fmt::Display for GpuFb {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "\n\n\n#### GpuFb({}x{}, {}x{}, d{}, --{}--, +{}x{}, {}@{:x})\n\n\n",
            self.width,
            self.height,
            self.vwidth,
            self.vheight,
            self.depth,
            self.pitch,
            self.x_offset,
            self.y_offset,
            self.size,
            self.pointer,
        )
    }
}

impl GpuFb {
    pub fn new(size: Size2d, depth: u32) -> GpuFb {
        GpuFb {
            width: size.x,
            height: size.y,
            vwidth: size.x,
            vheight: size.y,
            pitch: 0,
            depth,
            x_offset: 0,
            y_offset: 0,
            pointer: 0, // could be 4096 for alignment?
            size: 0,
        }
    }

    /// Returns a pointer to the register block
    fn ptr() -> *const RegisterBlock {
        MAILBOX_BASE as *const _
    }

    pub fn write(&self) -> Result<()> {
        write(self, &self.width as *const u32 as u32, channel::FrameBuffer)
    }

    pub fn read(&mut self) -> Result<()> {
        read(self, 0, channel::FrameBuffer)
    }

    pub fn call(&mut self) -> Result<()> {
        self.write()?;
        self.read()
    }
}
