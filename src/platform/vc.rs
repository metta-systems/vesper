use crate::{
    jtag_dbg_wait,
    platform::{
        display::{Display, PixelOrder, Size2d, CHARSIZE_X, CHARSIZE_Y},
        mailbox::{self, channel, response::VAL_LEN_FLAG, tag, Mailbox},
        rpi3::BcmHost,
    },
    println,
};

pub struct VC;

impl VC {
    // Use mailbox framebuffer interface to initialize
    // https://www.raspberrypi.org/forums/viewtopic.php?f=72&t=185116
    pub fn init_fb(size: Size2d, depth: u32) -> Option<Display> {
        // Use property channel
        let mut mbox = Mailbox::new();

        mbox.buffer[0] = 22 * 4;
        mbox.buffer[1] = mailbox::REQUEST;

        mbox.buffer[2] = tag::SetPhysicalWH;
        mbox.buffer[3] = 8; // Buffer size   // val buf size
        mbox.buffer[4] = 8; // Request size  // val size
        mbox.buffer[5] = size.x; // Space for horizontal resolution
        mbox.buffer[6] = size.y; // Space for vertical resolution

        mbox.buffer[7] = tag::SetVirtualWH as u32;
        mbox.buffer[8] = 8; // Buffer size   // val buf size
        mbox.buffer[9] = 8; // Request size  // val size
        mbox.buffer[10] = size.x; // Space for horizontal resolution
        mbox.buffer[11] = size.y; // Space for vertical resolution

        mbox.buffer[12] = tag::SetDepth as u32;
        mbox.buffer[13] = 4; // Buffer size   // val buf size
        mbox.buffer[14] = 4; // Request size  // val size
        mbox.buffer[15] = depth; // bpp

        mbox.buffer[16] = tag::AllocateBuffer as u32;
        mbox.buffer[17] = 8; // Buffer size   // val buf size
        mbox.buffer[18] = 4; // Request size  // val size
        mbox.buffer[19] = 16; // Alignment = 16 -- fb_ptr will be here
        mbox.buffer[20] = 0; // Space for response -- fb_size will be here

        mbox.buffer[21] = tag::End as u32;

        mbox.call(channel::PropertyTagsArmToVc).map_err(|_| ());

        jtag_dbg_wait();

        if (mbox.buffer[18] & VAL_LEN_FLAG) == 0 {
            return None;
        }

        let fb_ptr = BcmHost::bus2phys(mbox.buffer[19]);
        let fb_size = mbox.buffer[20];

        mbox.buffer[0] = 15 * 4;
        mbox.buffer[1] = mailbox::REQUEST;

        // SetPixelOrder doesn't work in QEMU, however TestPixelOrder does.
        mbox.buffer[2] = tag::TestPixelOrder;
        mbox.buffer[3] = 4;
        mbox.buffer[4] = 4;
        mbox.buffer[5] = 1; // PixelOrder

        mbox.buffer[6] = tag::SetAlphaMode;
        mbox.buffer[7] = 4;
        mbox.buffer[8] = 4;
        mbox.buffer[9] = mailbox::alpha_mode::IGNORED;

        mbox.buffer[10] = tag::GetPitch;
        mbox.buffer[11] = 4;
        mbox.buffer[12] = 0;
        mbox.buffer[13] = 0;

        mbox.buffer[14] = tag::End;

        mbox.call(channel::PropertyTagsArmToVc).map_err(|_| ());

        if (mbox.buffer[4] & VAL_LEN_FLAG) == 0 || (mbox.buffer[12] & VAL_LEN_FLAG) == 0 {
            return None;
        }

        let order = match mbox.buffer[5] {
            0 => PixelOrder::BGR,
            1 => PixelOrder::RGB,
            _ => return None,
        };

        let pitch = mbox.buffer[13];

        /* Need to set up max_x/max_y before using Display::write */
        let max_x = size.x / CHARSIZE_X;
        let max_y = size.y / CHARSIZE_Y;

        println!(
            "[i] VC init: {}x{}, {}x{}, d{}, --{}--, +{}x{}, {}@{:x}",
            size.x,
            size.y,
            size.x,
            size.y,
            depth,
            pitch,
            0, // x_offset
            0, // y_offset
            fb_size,
            fb_ptr
        );

        Some(Display::new(
            fb_ptr, fb_size, depth, pitch, max_x, max_y, size.x, size.y, order,
        ))
    }
}
