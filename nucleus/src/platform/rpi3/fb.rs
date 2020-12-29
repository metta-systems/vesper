/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    super::{
        mailbox::{channel, read, write, MailboxOps, RegisterBlock, Result},
        BcmHost,
    },
    core::ops::Deref,
};

/// FrameBuffer channel supported structure - use with mailbox::channel::FrameBuffer
/// Must have the same alignment as the mailbox buffers.
#[repr(C)]
#[repr(align(16))]
pub struct FrameBuffer {
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
    // Must be after HW-dictated fields to not break structure alignment.
    base_addr: usize,
}

// @todo rewrite in terms of using the Mailbox

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.STATUS.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*FrameBuffer::ptr()).STATUS.read() }
/// ```
impl Deref for FrameBuffer {
    type Target = RegisterBlock; // mailbox RegisterBlock reused here

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl core::fmt::Debug for FrameBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "\n\n\n#### FrameBuffer({}x{}, {}x{}, d{}, --{}--, +{}x{}, {}@{:x})\n\n\n",
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

impl FrameBuffer {
    pub fn new(reg_base: usize, width: u32, height: u32, depth: u32) -> FrameBuffer {
        FrameBuffer {
            width,
            height,
            vwidth: width,
            vheight: height,
            pitch: 0,
            depth,
            x_offset: 0,
            y_offset: 0,
            pointer: 0, // could be 4096 for the alignment?
            size: 0,

            base_addr: reg_base,
        }
    }
}

impl MailboxOps for FrameBuffer {
    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    /// <https://github.com/raspberrypi/firmware/wiki/Accessing-mailboxes> says:
    /// **With the exception of the property tags mailbox channel,**
    /// when passing memory addresses as the data part of a mailbox message,
    /// the addresses should be **bus addresses as seen from the VC.**
    fn write(&self, _channel: u32) -> Result<()> {
        write(
            self,
            BcmHost::phys2bus(&self as *const _ as usize) as *const _,
            channel::FrameBuffer,
        )
    }

    fn read(&self, _channel: u32) -> Result<()> {
        read(self, 0, channel::FrameBuffer)
    }
}
