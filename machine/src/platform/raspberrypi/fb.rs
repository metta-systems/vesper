use {
    super::mailbox::{self, LocalMailboxStorage, Mailbox, MailboxError, MailboxOps},
    crate::memory::{Address, Virtual},
};

/// FrameBuffer channel supported structure - use with mailbox::channel::FrameBuffer
/// Must have the same alignment as the mailbox buffers.
type FrameBufferData = LocalMailboxStorage<10>;

mod index {
    pub const WIDTH: usize = 0;
    pub const HEIGHT: usize = 1;
    pub const VIRTUAL_WIDTH: usize = 2;
    pub const VIRTUAL_HEIGHT: usize = 3;
    pub const PITCH: usize = 4;
    pub const DEPTH: usize = 5;
    pub const X_OFFSET: usize = 6;
    pub const Y_OFFSET: usize = 7;
    pub const POINTER: usize = 8; // FIXME: Value could be 4096 for the alignment restriction.
    pub const SIZE: usize = 9;
}

// control: MailboxCommand<10, FrameBufferData>
pub struct FrameBuffer {
    mailbox: Mailbox<10, FrameBufferData>,
}

impl core::fmt::Debug for FrameBufferData {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "\n\n\n#### FrameBuffer({}x{}, {}x{}, d{}, --{}--, +{}x{}, {}@{:x})\n\n\n",
            self.storage[index::WIDTH],
            self.storage[index::HEIGHT],
            self.storage[index::VIRTUAL_WIDTH],
            self.storage[index::VIRTUAL_HEIGHT],
            self.storage[index::HEIGHT],
            self.storage[index::PITCH],
            self.storage[index::X_OFFSET],
            self.storage[index::Y_OFFSET],
            self.storage[index::SIZE],
            self.storage[index::POINTER],
        )
    }
}

impl FrameBuffer {
    pub fn new(
        mmio_base_addr: Address<Virtual>, // skip this, use MAILBOX driver
        width: u32,
        height: u32,
        depth: u32,
    ) -> Result<FrameBuffer, MailboxError> {
        let mut fb = FrameBuffer {
            mailbox: unsafe { Mailbox::<10, FrameBufferData>::new(mmio_base_addr)? },
        };
        fb.mailbox.buffer.storage[index::WIDTH] = width;
        fb.mailbox.buffer.storage[index::VIRTUAL_WIDTH] = width;
        fb.mailbox.buffer.storage[index::HEIGHT] = height;
        fb.mailbox.buffer.storage[index::VIRTUAL_HEIGHT] = height;
        fb.mailbox.buffer.storage[index::DEPTH] = depth;
        Ok(fb)
    }
}

impl MailboxOps for FrameBuffer {
    fn write(&self, _channel: u32) -> mailbox::Result<()> {
        self.mailbox.do_write(mailbox::channel::FrameBuffer)
    }

    fn read(&self, _channel: u32) -> mailbox::Result<()> {
        unsafe { self.mailbox.do_read(mailbox::channel::FrameBuffer, 0) }
    }
}
