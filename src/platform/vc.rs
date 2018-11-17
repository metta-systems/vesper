use core::fmt::Write;
use platform::display::{Display, PixelOrder, Size2d, CHARSIZE_X, CHARSIZE_Y};
use platform::mailbox::{self, channel, response::VAL_LEN_FLAG, tag, GpuFb, Mailbox};
use platform::rpi3::bus2phys;
use platform::uart::MiniUart;

pub struct VC;

impl VC {
    // Use mailbox framebuffer interface to initialize
    pub fn init_fb(size: Size2d /*, uart: &mut MiniUart*/) -> Option<Display> {
        let mut fb_info = GpuFb::new(size, 32);

        //        uart.puts("initing fb_info\n");
        fb_info.call().map_err(|_| {
            /*uart.puts("fb_info error\n");*/
            ()
        });

        //        write!(uart, "inited fb_info: {}\n", fb_info);

        //        let mut pixel_order = Mailbox::new();
        //
        //        pixel_order.buffer[0] = 24;
        //        pixel_order.buffer[1] = mailbox::REQUEST;
        //        pixel_order.buffer[2] = tag::SetPixelOrder;
        //        pixel_order.buffer[3] = 4;
        //        pixel_order.buffer[4] = 4;
        //        pixel_order.buffer[5] = 0; // 0 - BGR, 1 - RGB
        //
        //        pixel_order.call(channel::PropertyTagsArmToVc).map_err(|_| ());

        /* Need to set up max_x/max_y before using Display::write */
        let max_x = fb_info.vwidth / CHARSIZE_X;
        let max_y = fb_info.vheight / CHARSIZE_Y;
        //        uart.puts("inited fb_info #2\n");

        Some(Display::new(
            bus2phys(fb_info.pointer),
            fb_info.size,
            fb_info.depth,
            fb_info.pitch,
            max_x,
            max_y,
            fb_info.vwidth,
            fb_info.vheight,
            PixelOrder::BGR,
        ))
    }
    /*
        fn get_display_size() -> Option<Size2d> {
            let mut mbox = Mbox::new();
        
            mbox.0[0] = 8 * 4; // Total size
            mbox.0[1] = MAILBOX_REQ_CODE; // Request
            mbox.0[2] = Tag::GetPhysicalWH as u32; // Display size  // tag
            mbox.0[3] = 8; // Buffer size   // val buf size
            mbox.0[4] = 0; // Request size  // val size
            mbox.0[5] = 0; // Space for horizontal resolution
            mbox.0[6] = 0; // Space for vertical resolution
            mbox.0[7] = Tag::End as u32; // End tag
        
            mbox.call(Channel::PropertyTagsArmToVc)?;
        
    //        if mbox.0[1] != MAILBOX_RESP_CODE_SUCCESS {
    //            return None;
    //        }
            if mbox.0[5] == 0 && mbox.0[6] == 0 {
                // Qemu emulation returns 0x0
                return Some(Size2d { x: 640, y: 480 });
            }
            Some(Size2d {
                x: mbox.0[5],
                y: mbox.0[6],
            })
        }
        
        fn set_display_size(size: Size2d) -> Option<Display> {
            // @todo Make Display use VC functions internally instead
            let mut mbox = Mbox::new();
            let mut count: usize = 0;
        
            count += 1;
            mbox.0[count] = MAILBOX_REQ_CODE; // Request
            count += 1;
            mbox.0[count] = Tag::SetPhysicalWH as u32;
            count += 1;
            mbox.0[count] = 8; // Buffer size   // val buf size
            count += 1;
            mbox.0[count] = 8; // Request size  // val size
            count += 1;
            mbox.0[count] = size.x; // Space for horizontal resolution
            count += 1;
            mbox.0[count] = size.y; // Space for vertical resolution
            count += 1;
            mbox.0[count] = Tag::SetVirtualWH as u32;
            count += 1;
            mbox.0[count] = 8; // Buffer size   // val buf size
            count += 1;
            mbox.0[count] = 8; // Request size  // val size
            count += 1;
            mbox.0[count] = size.x; // Space for horizontal resolution
            count += 1;
            mbox.0[count] = size.y; // Space for vertical resolution
            count += 1;
            mbox.0[count] = Tag::SetDepth as u32;
            count += 1;
            mbox.0[count] = 4; // Buffer size   // val buf size
            count += 1;
            mbox.0[count] = 4; // Request size  // val size
            count += 1;
            mbox.0[count] = 16; // 16 bpp
            count += 1;
            mbox.0[count] = Tag::AllocateBuffer as u32;
            count += 1;
            mbox.0[count] = 8; // Buffer size   // val buf size
            count += 1;
            mbox.0[count] = 4; // Request size  // val size
            count += 1;
            mbox.0[count] = 4096; // Alignment = 4096
            count += 1;
            mbox.0[count] = 0; // Space for response
            count += 1;
            mbox.0[count] = Tag::End as u32;
            mbox.0[0] = (count * 4) as u32; // Total size
        
            let max_count = count;
        
            Mailbox::call(Channel::PropertyTagsArmToVc as u8, &mbox.0 as *const u32 as *const u8)?;
        
            if mbox.0[1] != MAILBOX_RESP_CODE_SUCCESS {
                return None;
            }
        
            count = 2; /* First tag */
    while mbox.0[count] != 0 {
    if mbox.0[count] == Tag::AllocateBuffer as u32 {
    break;
    }

    /* Skip to next tag
     * Advance count by 1 (tag) + 2 (buffer size/value size)
     *                          + specified buffer size
     */
    count += 3 + (mbox.0[count + 1] / 4) as usize;

    if count > max_count {
    return None;
    }
    }

    /* Must be 8 bytes, plus MSB set to indicate a response */
    if mbox.0[count + 2] != 0x8000_0008 {
    return None;
    }

    /* Framebuffer address/size in response */
    let physical_screenbase = mbox.0[count + 3];
    let screensize = mbox.0[count + 4];

    if physical_screenbase == 0 || screensize == 0 {
    return None;
    }

    /* physical_screenbase is the address of the screen in RAM
     * screenbase needs to be the screen address in virtual memory
     */
    // screenbase=mem_p2v(physical_screenbase);
    let screenbase = physical_screenbase;

    /* Get the framebuffer pitch (bytes per line) */
    mbox.0[0] = 7 * 4; // Total size
    mbox.0[1] = 0; // Request
    mbox.0[2] = Tag::GetPitch as u32; // Display size
    mbox.0[3] = 4; // Buffer size
    mbox.0[4] = 0; // Request size
    mbox.0[5] = 0; // Space for pitch
    mbox.0[6] = Tag::End as u32;

    Mailbox::call(Channel::PropertyTagsArmToVc as u8, &mbox.0 as *const u32 as *const u8)?;

    if mbox.0[1] != MAILBOX_RESP_CODE_SUCCESS {
    return None;
    }

    /* Must be 4 bytes, plus MSB set to indicate a response */
    if mbox.0[4] != 0x8000_0004 {
    return None;
    }

    let pitch = mbox.0[5];
    if pitch == 0 {
    return None;
    }

    /* Need to set up max_x/max_y before using Display::write */
    let max_x = size.x / CHARSIZE_X;
    let max_y = size.y / CHARSIZE_Y;

    Some(Display {
    base: screenbase,
    size: screensize,
    pitch: pitch,
    max_x: max_x,
    max_y: max_y,
    })
    }*/
}
