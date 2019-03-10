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
        let mut mbox = Mailbox::default();

        /*
         *  * All tags in the request are processed in one operation.
         *  * It is not valid to mix Test tags with Get/Set tags
         *    in the same operation and no tags will be returned.
         *  * Get tags will be processed after all Set tags.
         *  * If an allocate buffer tag is omitted when setting parameters,
         *    then no change occurs unless it can be accommodated without changing
         *    the buffer base or size.
         *  * When an allocate buffer response is returned, the old buffer area
         *    (if the base or size has changed) is implicitly freed.
         */

        let index = mbox.request();
        let index = mbox.set_physical_wh(index, size.x, size.y);
        let index = mbox.set_virtual_wh(index, size.x, size.y);
        let index = mbox.set_depth(index, depth);
        let index = mbox.allocate_buffer_aligned(index, 16);
        mbox.end(index);

        mbox.call(channel::PropertyTagsArmToVc).map_err(|e| {
            println!("Mailbox call returned error {}", e);
            println!("Mailbox contents: {}", mbox);
            ()
        });

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
