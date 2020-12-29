/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    super::{
        display::{Display, PixelOrder, CHARSIZE_X, CHARSIZE_Y},
        mailbox::{self, channel, response::VAL_LEN_FLAG, Mailbox, MailboxOps},
        BcmHost,
    },
    crate::println,
    core::convert::TryInto,
    snafu::Snafu,
};

pub struct VC;

#[derive(Debug, Snafu)]
pub enum VcError {
    #[snafu(display("VC setup failed in mailbox operation"))]
    MailboxError,
    #[snafu(display("VC setup failed due to bad mailbox response {:x}", response))]
    MailboxResponseError { response: u32 },
    #[snafu(display("Unknown pixel order received in mailbox response"))]
    InvalidPixelOrder,
}
type Result<T, E = VcError> = ::core::result::Result<T, E>;

impl VC {
    // Use framebuffer mailbox interface to initialize
    // https://www.raspberrypi.org/forums/viewtopic.php?f=72&t=185116
    pub fn init_fb(w: u32, h: u32, depth: u32) -> Result<Display> {
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

        let mut mbox = Mailbox::default();
        let index = mbox.request();
        let index = mbox.set_physical_wh(index, w, h);
        let index = mbox.set_virtual_wh(index, w, h);
        let index = mbox.set_depth(index, depth);
        let index = mbox.allocate_buffer_aligned(index, 16);
        let mbox = mbox.end(index);

        mbox.call(channel::PropertyTagsArmToVc).map_err(|e| {
            println!("Mailbox call returned error {}", e);
            println!("Mailbox contents: {:?}", mbox);
            VcError::MailboxError
        })?;

        if (mbox.value_at(18) & VAL_LEN_FLAG) == 0 {
            return Err(VcError::MailboxResponseError {
                response: mbox.value_at(18),
            });
        }

        let fb_ptr = BcmHost::bus2phys(mbox.value_at(19).try_into().unwrap());
        let fb_size = mbox.value_at(20);

        // SetPixelOrder doesn't work in QEMU, however TestPixelOrder does.
        // Apparently, QEMU doesn't care about intermixing Get/Set and Test tags either.
        let mut mbox = Mailbox::default();
        let index = mbox.request();
        #[cfg(qemu)]
        let index = mbox.test_pixel_order(index, 1);
        #[cfg(not(qemu))]
        let index = mbox.set_pixel_order(index, 1);
        let index = mbox.set_alpha_mode(index, mailbox::alpha_mode::IGNORED);
        let index = mbox.get_pitch(index);
        let mbox = mbox.end(index);

        // let index = mbox.test_pixel_order(index, 1);

        mbox.call(channel::PropertyTagsArmToVc)
            .map_err(|_| VcError::MailboxError)?;

        if (mbox.value_at(4) & VAL_LEN_FLAG) == 0 {
            return Err(VcError::MailboxResponseError {
                response: mbox.value_at(4),
            });
        }
        if (mbox.value_at(12) & VAL_LEN_FLAG) == 0 {
            return Err(VcError::MailboxResponseError {
                response: mbox.value_at(12),
            });
        }

        let order = match mbox.value_at(5) {
            0 => PixelOrder::BGR,
            1 => PixelOrder::RGB,
            _ => return Err(VcError::InvalidPixelOrder),
        };

        let pitch = mbox.value_at(13);

        /* Need to set up max_x/max_y before using Display::write */
        let max_x = w / CHARSIZE_X;
        let max_y = h / CHARSIZE_Y;

        let x_offset = 0;
        let y_offset = 0;

        println!(
            "[i] VC init: {}x{}, {}x{}, d{}, --{}--, +{}x{}, {}@{:x}",
            w, h, w, h, depth, pitch, x_offset, y_offset, fb_size, fb_ptr
        );

        Ok(Display::new(
            fb_ptr.try_into().unwrap(),
            fb_size,
            depth,
            pitch,
            max_x,
            max_y,
            w,
            h,
            order,
        ))
    }
}
