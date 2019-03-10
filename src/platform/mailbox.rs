use crate::{
    platform::{display::Size2d, rpi3::BcmHost},
    println,
};
use core::ops::Deref;
use core::sync::atomic::{compiler_fence, Ordering};
use cortex_a::barrier;
use register::mmio::*;

// Public interface to the mailbox.
// The address for the buffer needs to be 16-byte aligned
// so that the VideoCore can handle it properly.
// The reason is that lowest 4 bits of the address will contain the channel number.
pub struct Mailbox<'a> {
    pub buffer: &'a mut [u32],
    base_addr: u32,
}

const MAILBOX_ALIGNMENT: usize = 16;
const MAILBOX_ITEMS_COUNT: usize = 36;

// Identity mapped first 1Gb by u-boot
const MAILBOX_BASE: u32 = BcmHost::get_peripheral_address() + 0xb880;
// Lowest 4-bits are channel ID.
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

impl core::fmt::Display for MboxError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                MboxError::ResponseError => "ResponseError",
                MboxError::UnknownError => "UnknownError",
                MboxError::Timeout => "Timeout",
            }
        )
    }
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

// Single code indicating request
pub const REQUEST: u32 = 0;

// Possible responses
pub mod response {
    pub const SUCCESS: u32 = 0x8000_0000;
    pub const ERROR: u32 = 0x8000_0001; // error parsing request buffer (partial response)
    /** When responding, the VC sets this bit in val_len to indicate a response. */
    /** Each tag with this bit set will contain VC response data. */
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
    pub const SetClockRate: u32 = 0x0003_8002;
    // GPU
    pub const AllocateMemory: u32 = 0x0003_000c; //< Allocate contiguous memory buffer
    pub const LockMemory: u32 = 0x0003_000d;
    pub const UnlockMemory: u32 = 0x0003_000e;
    pub const ReleaseMemory: u32 = 0x003_000f;
    pub const ExecuteCode: u32 = 0x0003_0010;
    pub const GetDispmanxResourceMemHandle: u32 = 0x0003_0014;
    pub const GetEdidBlock: u32 = 0x0003_0020;
    // FB
    pub const AllocateBuffer: u32 = 0x0004_0001; //< Allocate framebuffer
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
    pub const SetCursorInfo: u32 = 0x0000_8010;
    pub const SetCursorState: u32 = 0x0000_8011;
    pub const GetGpioState: u32 = 0x0003_0041;
    pub const SetGpioState: u32 = 0x0003_8041;
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
    pub const OPAQUE_0: u32 = 0; // 255 is transparent
    pub const TRANSPARENT_0: u32 = 1; // 255 is opaque
    pub const IGNORED: u32 = 2;
}

fn write(regs: &RegisterBlock, buf_ptr: u32, channel: u32) -> Result<()> {
    let mut count: u32 = 0;

    // let buf_ptr = BcmHost::phys2bus(buf_ptr); not used for PropertyTags channel

    println!("Mailbox::write {:x}/{:x}", buf_ptr, channel);

    // Insert a compiler fence that ensures that all stores to the
    // mbox buffer are finished before the GPU is signaled (which is
    // done by a store operation as well).
    compiler_fence(Ordering::Release);

    while regs.STATUS.is_set(STATUS::FULL) {
        count += 1;
        if count > (1 << 25) {
            return Err(MboxError::Timeout);
        }
    }
    unsafe {
        barrier::dmb(barrier::SY);
    }
    regs.WRITE
        .set((buf_ptr & !CHANNEL_MASK) | (channel & CHANNEL_MASK));
    Ok(())
}

fn read(regs: &RegisterBlock, expected: u32, channel: u32) -> Result<()> {
    loop {
        let mut count: u32 = 0;
        while regs.STATUS.is_set(STATUS::EMPTY) {
            count += 1;
            if count > (1 << 25) {
                println!("Timed out waiting for mbox response");
                return Err(MboxError::Timeout);
            }
        }

        /* Read the data
         * Data memory barriers as we've switched peripheral
         */
        unsafe {
            barrier::dmb(barrier::SY);
        }
        let data: u32 = regs.READ.get();
        unsafe {
            barrier::dmb(barrier::SY);
        }

        println!(
            "Received mbox response {:#08x}, expecting {:#08x}",
            data, expected
        );

        // is it a response to our message?
        if ((data & CHANNEL_MASK) == channel) && ((data & !CHANNEL_MASK) == expected) {
            // is it a valid successful response?
            return Ok(());
        } else {
            // will return on Timeout if no response received...
            // return Err(MboxError::ResponseError); //@fixme ignore invalid responses and loop again?
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
impl<'a> Deref for Mailbox<'a> {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl<'a> core::fmt::Display for Mailbox<'a> {
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

impl<'a> Default for Mailbox<'a> {
    fn default() -> Self {
        Self::new_default().expect("Couldn't allocate a mailbox")
    }
}

impl<'a> Mailbox<'a> {
    pub fn new_default() -> ::core::result::Result<Mailbox<'a>, ()> {
        let ret = crate::DMA_ALLOCATOR
            .lock(|d| d.alloc_slice_zeroed(MAILBOX_ITEMS_COUNT, MAILBOX_ALIGNMENT));

        if ret.is_err() {
            return Err(());
        }

        Ok(Mailbox {
            base_addr: MAILBOX_BASE,
            buffer: ret.unwrap(),
        })
    }

    pub fn new(base_addr: usize) -> ::core::result::Result<Mailbox<'a>, ()> {
        let ret = crate::DMA_ALLOCATOR
            .lock(|d| d.alloc_slice_zeroed(MAILBOX_ITEMS_COUNT, MAILBOX_ALIGNMENT));

        if ret.is_err() {
            return Err(());
        }

        use core::convert::TryFrom;
        let base_addr = u32::try_from(base_addr).unwrap();

        Ok(Mailbox {
            base_addr,
            buffer: ret.unwrap(),
        })
    }

    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    pub fn write(&self, channel: u32) -> Result<()> {
        write(self, self.buffer.as_ptr() as u32, channel)
    }

    pub fn read(&self, channel: u32) -> Result<()> {
        read(self, self.buffer.as_ptr() as u32, channel)?;

        match self.buffer[1] {
            response::SUCCESS => {
                println!("\n######\nMailbox::returning SUCCESS");
                Ok(())
            }
            response::ERROR => {
                println!("\n######\nMailbox::returning ResponseError");
                Err(MboxError::ResponseError)
            }
            _ => {
                println!("\n######\nMailbox::returning UnknownError");
                println!("{:x}\n######", self.buffer[1]);
                Err(MboxError::UnknownError)
            }
        }
    }

    pub fn call(&self, channel: u32) -> Result<()> {
        self.write(channel)?;
        self.read(channel)
    }

    // Specific mailbox functions

    #[inline]
    pub fn request(&mut self) -> usize {
        self.buffer[1] = REQUEST;
        2
    }

    #[inline]
    pub fn end(&mut self, index: usize) -> () {
        // @todo return Result
        self.buffer[index] = tag::End;
        self.buffer[0] = (index as u32 + 1) * 4;
    }

    #[inline]
    pub fn set_physical_wh(&mut self, index: usize, width: u32, height: u32) -> usize {
        self.buffer[index] = tag::SetPhysicalWH;
        self.buffer[index + 1] = 8; // Buffer size   // val buf size
        self.buffer[index + 2] = 8; // Request size  // val size
        self.buffer[index + 3] = width; // Space for horizontal resolution
        self.buffer[index + 4] = height; // Space for vertical resolution
        index + 5
    }

    #[inline]
    pub fn set_virtual_wh(&mut self, index: usize, width: u32, height: u32) -> usize {
        self.buffer[index] = tag::SetVirtualWH;
        self.buffer[index + 1] = 8; // Buffer size   // val buf size
        self.buffer[index + 2] = 8; // Request size  // val size
        self.buffer[index + 3] = width; // Space for horizontal resolution
        self.buffer[index + 4] = height; // Space for vertical resolution
        index + 5
    }

    #[inline]
    pub fn set_depth(&mut self, index: usize, depth: u32) -> usize {
        self.buffer[index] = tag::SetDepth;
        self.buffer[index + 1] = 4; // Buffer size   // val buf size
        self.buffer[index + 2] = 4; // Request size  // val size
        self.buffer[index + 3] = depth; // bpp
        index + 4
    }

    #[inline]
    pub fn allocate_buffer_aligned(&mut self, index: usize, alignment: u32) -> usize {
        self.buffer[index] = tag::AllocateBuffer;
        self.buffer[index + 1] = 8; // Buffer size   // val buf size
        self.buffer[index + 2] = 4; // Request size  // val size
        self.buffer[index + 3] = alignment; // Alignment = 16 -- fb_ptr will be here
        self.buffer[index + 4] = 0; // Space for response -- fb_size will be here
        index + 5
    }

    #[inline]
    pub fn set_led_on(&mut self, index: usize, enable: bool) -> usize {
        self.buffer[index] = tag::SetGpioState;
        self.buffer[index + 1] = 8; // Buffer size   // val buf size
        self.buffer[index + 2] = 0; // Response size  // val size
        self.buffer[index + 3] = 130; // Pin Number
        self.buffer[index + 4] = if enable { 1 } else { 0 };
        index + 5
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

    // https://github.com/raspberrypi/firmware/wiki/Accessing-mailboxes says:
    // **With the exception of the property tags mailbox channel,**
    // when passing memory addresses as the data part of a mailbox message,
    // the addresses should be **bus addresses as seen from the VC.**
    pub fn write(&self) -> Result<()> {
        write(
            self,
            BcmHost::phys2bus(&self.width as *const u32 as u32),
            channel::FrameBuffer,
        )
    }

    pub fn read(&mut self) -> Result<()> {
        read(self, 0, channel::FrameBuffer)
    }

    pub fn call(&mut self) -> Result<()> {
        self.write()?;
        self.read()
    }
}
