/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 *
 * Based on https://github.com/rust-embedded/rust-raspi3-tutorial/blob/master/04_mailboxes/src/mbox.rs
 * by Andre Richter of Tock OS.
 */

//! Broadcom mailbox interface between the VideoCore and the ARM Core.
//!

#![allow(dead_code)]

use {
    super::BcmHost,
    crate::{platform::MMIODerefWrapper, println, DMA_ALLOCATOR},
    core::{
        alloc::{AllocError, Allocator, Layout},
        mem,
        ptr::NonNull,
        result::Result as CoreResult,
        sync::atomic::{compiler_fence, Ordering},
    },
    cortex_a::asm::barrier,
    snafu::Snafu,
    tock_registers::{
        interfaces::{Readable, Writeable},
        register_bitfields, register_structs,
        registers::{ReadOnly, WriteOnly},
    },
};

/// Public interface to the mailbox.
/// The address for the buffer needs to be 16-byte aligned
/// so that the VideoCore can handle it properly.
/// The reason is that lowest 4 bits of the address will contain the channel number.
pub struct Mailbox<const N_SLOTS: usize, Storage = DmaBackedMailboxStorage<N_SLOTS>> {
    registers: Registers,
    pub buffer: Storage,
}

/// Mailbox that is ready to be called.
/// This prevents invalid use of the mailbox until it is fully prepared.
pub struct PreparedMailbox<const N_SLOTS: usize, Storage = DmaBackedMailboxStorage<N_SLOTS>>(
    Mailbox<N_SLOTS, Storage>,
);

const MAILBOX_ALIGNMENT: usize = 16;
const MAILBOX_ITEMS_COUNT: usize = 36;

/// We've identity mapped the MMIO register region on kernel start.
const MAILBOX_BASE: usize = BcmHost::get_peripheral_address() + 0xb880;
/// Lowest 4-bits are channel ID.
const CHANNEL_MASK: u32 = 0xf;

// Mailbox Peek  Read/Write  Status  Sender  Config
//    0    0x10  0x00        0x18    0x14    0x1c
//    1    0x30  0x20        0x38    0x34    0x3c
//
// Only mailbox 0's status can trigger interrupts on the ARM, so Mailbox 0 is
// always for communication from VC to ARM and Mailbox 1 is for ARM to VC.
//
// The ARM should never write Mailbox 0 or read Mailbox 1.
//
// There are 32 mailboxes on the ARM, which could be used for in-processor or inter-processor comms,
// TODO: allow using all of them.

register_bitfields! {
    u32,

    STATUS [
        /* Bit 31 set in status register if the write mailbox is full */
        FULL  OFFSET(31) NUMBITS(1) [],
        /* Bit 30 set in status register if the read mailbox is empty */
        EMPTY OFFSET(30) NUMBITS(1) []
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub RegisterBlock {
        (0x00 => READ: ReadOnly<u32>), // This is Mailbox0 read for ARM, can't write
        (0x04 => __reserved_1),
        (0x18 => STATUS: ReadOnly<u32, STATUS::Register>),
        (0x1c => __reserved_2),
        (0x20 => WRITE: WriteOnly<u32>), // This is Mailbox1 write for ARM, can't read
        (0x24 => @END),
    }
}

// Hide RegisterBlock from public api.
type Registers = MMIODerefWrapper<RegisterBlock>;

#[derive(Snafu, Debug)]
pub enum MailboxError {
    #[snafu(display("ResponseError"))]
    Response,
    #[snafu(display("UnknownError"))]
    Unknown,
    #[snafu(display("Timeout"))]
    Timeout,
    #[snafu(display("AllocError"))]
    Alloc,
}

pub type Result<T> = CoreResult<T, MailboxError>;

/// Typical operations with a mailbox.
pub trait MailboxOps {
    fn write(&self, channel: u32) -> Result<()>;
    fn read(&self, channel: u32) -> Result<()>;
    fn call(&self, channel: u32) -> Result<()> {
        self.write(channel)?;
        self.read(channel)
    }
}

pub trait MailboxStorage {
    fn new() -> Result<Self>
    where
        Self: Sized;
}

pub trait MailboxStorageRef {
    fn as_ref(&self) -> &[u32];
    fn as_mut(&mut self) -> &mut [u32];
    fn as_ptr(&self) -> *const u32;
    fn value_at(&self, index: usize) -> u32;
}

// TODO: allow from 2 to 36 slots (2 because you need at least an End tag)
#[repr(align(16))] // MAILBOX_ALIGNMENT
pub struct LocalMailboxStorage<const N_SLOTS: usize> {
    pub storage: [u32; N_SLOTS],
}

pub struct DmaBackedMailboxStorage<const N_SLOTS: usize> {
    pub storage: *mut u32,
}

impl<const N_SLOTS: usize> MailboxStorage for LocalMailboxStorage<N_SLOTS> {
    fn new() -> Result<Self> {
        Ok(Self {
            storage: [0u32; N_SLOTS],
        })
    }
}

impl<const N_SLOTS: usize> MailboxStorage for DmaBackedMailboxStorage<N_SLOTS> {
    fn new() -> Result<Self> {
        Ok(Self {
            storage: DMA_ALLOCATOR
                .lock(|a| {
                    a.allocate(
                        Layout::from_size_align(N_SLOTS * mem::size_of::<u32>(), 16)
                            .map_err(|_| AllocError)?,
                    )
                })
                .map_err(|_| MailboxError::Alloc)?
                .as_mut_ptr() as *mut u32,
        })
    }
}

impl<const N_SLOTS: usize> Drop for DmaBackedMailboxStorage<N_SLOTS> {
    fn drop(&mut self) {
        DMA_ALLOCATOR
            .lock::<_, Result<()>>(|a| unsafe {
                #[allow(clippy::unit_arg)]
                Ok(a.deallocate(
                    NonNull::new_unchecked(self.storage as *mut u8),
                    Layout::from_size_align(N_SLOTS * mem::size_of::<u32>(), 16)
                        .map_err(|_| MailboxError::Alloc)?,
                ))
            })
            .unwrap_or(())
    }
}

impl<const N_SLOTS: usize> MailboxStorageRef for LocalMailboxStorage<N_SLOTS> {
    fn as_ref(&self) -> &[u32] {
        &self.storage
    }

    fn as_mut(&mut self) -> &mut [u32] {
        &mut self.storage
    }

    fn as_ptr(&self) -> *const u32 {
        self.storage.as_ptr()
    }

    // @todo Probably need a ResultMailbox for accessing data after call()?
    fn value_at(&self, index: usize) -> u32 {
        self.storage[index]
    }
}

impl<const N_SLOTS: usize> MailboxStorageRef for DmaBackedMailboxStorage<N_SLOTS> {
    fn as_ref(&self) -> &[u32] {
        unsafe { core::slice::from_raw_parts(self.storage.cast(), N_SLOTS) }
    }

    fn as_mut(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.storage.cast(), N_SLOTS) }
    }

    fn as_ptr(&self) -> *const u32 {
        self.storage.cast()
    }

    // @todo Probably need a ResultMailbox for accessing data after call()?
    fn value_at(&self, index: usize) -> u32 {
        self.as_ref()[index]
    }
}

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
    /// Channel number is ignored. Use for implementations of MailboxOps that use hardcoded
    /// channel number.
    pub const Ignored: u32 = !0;
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

impl<const N_SLOTS: usize> core::fmt::Debug for Mailbox<N_SLOTS> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let count = self.buffer.as_ref()[0] / 4;
        assert_eq!(self.buffer.as_ref()[0], count * 4);
        assert!(count <= 36);
        for i in 0usize..count as usize {
            writeln!(f, "[{:02}] {:08x}", i, self.buffer.value_at(i))?;
        }
        Ok(())
    }
}

impl<const N_SLOTS: usize> core::fmt::Debug for PreparedMailbox<N_SLOTS> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<const N_SLOTS: usize> Default for Mailbox<N_SLOTS> {
    fn default() -> Self {
        unsafe { Self::new(MAILBOX_BASE) }.expect("Couldn't allocate a default mailbox")
    }
}

impl<const N_SLOTS: usize, Storage: MailboxStorage + MailboxStorageRef> Mailbox<N_SLOTS, Storage> {
    /// Create a new mailbox locally in an aligned stack space.
    /// # Safety
    /// Caller is responsible for picking the correct MMIO register base address.
    pub unsafe fn new(base_addr: usize) -> Result<Mailbox<N_SLOTS, Storage>> {
        Ok(Mailbox {
            registers: Registers::new(base_addr),
            buffer: Storage::new()?,
        })
    }

    // Specific mailbox functions

    /// Start mailbox request.
    ///
    /// @returns index of the next available slot.
    #[inline]
    pub fn request(&mut self) -> usize {
        self.buffer.as_mut()[1] = REQUEST;
        2
    }

    /// Mark mailbox payload as completed.
    /// Consumes the Mailbox and returns a Preparedmailbox that can be called.
    #[inline]
    pub fn end(mut self, index: usize) -> PreparedMailbox<N_SLOTS, Storage> {
        // @todo return Result
        self.buffer.as_mut()[index] = tag::End;
        self.buffer.as_mut()[0] = (index as u32 + 1) * 4;
        PreparedMailbox(self)
    }

    ///
    /// @returns index of the next available slot.
    #[inline]
    pub fn set_physical_wh(&mut self, index: usize, width: u32, height: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetPhysicalWH;
        buf[index + 1] = 8; // Buffer size   // val buf size
        buf[index + 2] = 8; // Request size  // val size
        buf[index + 3] = width; // Space for horizontal resolution
        buf[index + 4] = height; // Space for vertical resolution
        index + 5
    }

    ///
    /// @returns index of the next available slot.
    #[inline]
    pub fn set_virtual_wh(&mut self, index: usize, width: u32, height: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetVirtualWH;
        buf[index + 1] = 8; // Buffer size   // val buf size
        buf[index + 2] = 8; // Request size  // val size
        buf[index + 3] = width; // Space for horizontal resolution
        buf[index + 4] = height; // Space for vertical resolution
        index + 5
    }

    ///
    /// @returns index of the next available slot.
    #[inline]
    pub fn set_depth(&mut self, index: usize, depth: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetDepth;
        buf[index + 1] = 4; // Buffer size   // val buf size
        buf[index + 2] = 4; // Request size  // val size
        buf[index + 3] = depth; // bpp
        index + 4
    }

    ///
    /// @returns index of the next available slot.
    #[inline]
    pub fn allocate_buffer_aligned(&mut self, index: usize, alignment: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::AllocateBuffer;
        buf[index + 1] = 8; // Buffer size   // val buf size
        buf[index + 2] = 4; // Request size  // val size
        buf[index + 3] = alignment; // Alignment = 16 -- fb_ptr will be here
        buf[index + 4] = 0; // Space for response -- fb_size will be here
        index + 5
    }

    ///
    /// @returns index of the next available slot.
    #[inline]
    pub fn set_led_on(&mut self, index: usize, enable: bool) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetGpioState;
        buf[index + 1] = 8; // Buffer size   // val buf size
        buf[index + 2] = 0; // Response size  // val size
        buf[index + 3] = 130; // Pin Number
        buf[index + 4] = enable.into();
        index + 5
    }

    #[inline]
    pub fn set_clock_rate(&mut self, index: usize, channel: u32, rate: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetClockRate;
        buf[index + 1] = 12; // Buffer size   // val buf size
        buf[index + 2] = 8; // Response size  // val size
        buf[index + 3] = channel; // mailbox::clock::*
        buf[index + 4] = rate;
        buf[index + 5] = 0; // skip turbo setting
        index + 6
    }

    /// NB: Do not intermix Get/Set and Test tags in one request!
    /// See <https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface>
    /// * It is not valid to mix Test tags with Get/Set tags in the same operation
    ///   and no tags will be returned.
    #[inline]
    pub fn set_pixel_order(&mut self, index: usize, order: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetPixelOrder;
        buf[index + 1] = 4; // Buffer size   // val buf size
        buf[index + 2] = 4; // Response size  // val size
        buf[index + 3] = order;
        index + 4
    }

    /// NB: Do not intermix Get/Set and Test tags in one request!
    /// See <https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface>
    /// * It is not valid to mix Test tags with Get/Set tags in the same operation
    ///   and no tags will be returned.
    #[inline]
    pub fn test_pixel_order(&mut self, index: usize, order: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::TestPixelOrder;
        buf[index + 1] = 4; // Buffer size   // val buf size
        buf[index + 2] = 4; // Response size  // val size
        buf[index + 3] = order;
        index + 4
    }

    #[inline]
    pub fn set_alpha_mode(&mut self, index: usize, mode: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetAlphaMode;
        buf[index + 1] = 4; // Buffer size   // val buf size
        buf[index + 2] = 4; // Response size  // val size
        buf[index + 3] = mode;
        index + 4
    }

    #[inline]
    pub fn get_pitch(&mut self, index: usize) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::GetPitch;
        buf[index + 1] = 4; // Buffer size   // val buf size
        buf[index + 2] = 4; // Response size  // val size
        buf[index + 3] = 0; // Result placeholder
        index + 4
    }

    #[inline]
    pub fn set_device_power(&mut self, index: usize, device_id: u32, power_flags: u32) -> usize {
        let buf = self.buffer.as_mut();
        buf[index] = tag::SetPowerState;
        buf[index + 1] = 8; // Buffer size   // val buf size
        buf[index + 2] = 8; // Response size  // val size
        buf[index + 3] = device_id;
        buf[index + 4] = power_flags; // bit 0: off, bit 1: no wait
        index + 5
    }

    // Actual work functions

    /// <https://github.com/raspberrypi/firmware/wiki/Accessing-mailboxes> says:
    /// **With the exception of the property tags mailbox channel,**
    /// when passing memory addresses as the data part of a mailbox message,
    /// the addresses should be **bus addresses as seen from the VC.**
    pub fn do_write(&self, channel: u32) -> Result<()> {
        let buf_ptr = self.buffer.as_ptr() as *const u32 as u32;
        let buf_ptr = if channel != channel::PropertyTagsArmToVc {
            BcmHost::phys2bus(buf_ptr as usize) as u32
        } else {
            buf_ptr
        };

        let mut count: u32 = 0;

        println!("Mailbox::write {:#08x}/{:#x}", buf_ptr, channel);

        // Insert a compiler fence that ensures that all stores to the
        // mailbox buffer are finished before the GPU is signaled (which is
        // done by a store operation as well).
        compiler_fence(Ordering::Release);

        while self.registers.STATUS.is_set(STATUS::FULL) {
            count += 1;
            if count > (1 << 25) {
                return Err(MailboxError::Timeout);
            }
        }
        barrier::dmb(barrier::SY);
        self.registers
            .WRITE
            .set((buf_ptr & !CHANNEL_MASK) | (channel & CHANNEL_MASK));
        Ok(())
    }

    /// Perform the mailbox read.
    ///
    /// # Safety
    ///
    /// Buffer will be mutated by the hardware before read operation is completed.
    pub unsafe fn do_read(&self, channel: u32, expected: u32) -> Result<()> {
        loop {
            let mut count: u32 = 0;
            while self.registers.STATUS.is_set(STATUS::EMPTY) {
                count += 1;
                if count > (1 << 25) {
                    println!("Timed out waiting for mailbox response");
                    return Err(MailboxError::Timeout);
                }
            }

            /* Read the data
             * Data memory barriers as we've switched peripheral
             */
            barrier::dmb(barrier::SY);
            let data: u32 = self.registers.READ.get();
            barrier::dmb(barrier::SY);

            println!(
                "Received mailbox response {:#08x}, expecting {:#08x}",
                data, expected
            );

            // is it a response to our message?
            if ((data & CHANNEL_MASK) == channel) && ((data & !CHANNEL_MASK) == expected) {
                // is it a valid successful response?
                return match self.buffer.value_at(1) {
                    response::SUCCESS => {
                        println!("\n######\nMailbox::returning SUCCESS");
                        Ok(())
                    }
                    response::ERROR => {
                        println!("\n######\nMailbox::returning ResponseError");
                        Err(MailboxError::Response)
                    }
                    _ => {
                        println!("\n######\nMailbox::returning UnknownError");
                        println!("{:x}\n######", self.buffer.value_at(1));
                        Err(MailboxError::Unknown)
                    }
                };
            } else {
                // ignore invalid responses and loop again.
                // will return Timeout above if no matching response is received.
            }
        }
    }
}

impl<const N_SLOTS: usize, Storage: MailboxStorage + MailboxStorageRef> MailboxOps
    for PreparedMailbox<N_SLOTS, Storage>
{
    fn write(&self, channel: u32) -> Result<()> {
        self.0.do_write(channel)
    }

    // @todo read() should probably consume PreparedMailbox completely - because request is overwritten with response
    fn read(&self, channel: u32) -> Result<()> {
        unsafe { self.0.do_read(channel, self.0.buffer.as_ptr() as u32) }
    }
}

impl<const N_SLOTS: usize, Storage: MailboxStorage + MailboxStorageRef> MailboxStorageRef
    for PreparedMailbox<N_SLOTS, Storage>
{
    fn as_ref(&self) -> &[u32] {
        self.0.buffer.as_ref()
    }

    fn as_mut(&mut self) -> &mut [u32] {
        self.0.buffer.as_mut()
    }

    fn as_ptr(&self) -> *const u32 {
        self.0.buffer.as_ptr()
    }

    // @todo Probably need a ResultMailbox for accessing data after call()?
    fn value_at(&self, index: usize) -> u32 {
        self.0.buffer.value_at(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Validate the buffer is filled correctly
    // Validate the buffer is properly terminated when call()ed -- this invariant must be maintained
    // by the end() fn.
    #[test_case]
    fn test_prepare_mailbox() {
        let mut mailbox = Mailbox::<8>::default();
        let index = mailbox.request();
        let index = mailbox.set_led_on(index, true);
        let mailbox = mailbox.end(index);
        // Instead of calling just check the filled buffer format:
        assert_eq!(
            unsafe { mailbox.0.buffer.as_ref()[0] } as usize,
            (index + 1) * 4
        );
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[1] }, REQUEST);
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[2] }, tag::SetGpioState);
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[3] }, 8);
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[4] }, 0);
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[5] }, 130);
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[6] }, 1);
        assert_eq!(unsafe { mailbox.0.buffer.as_ref()[7] }, tag::End);
    }
}
