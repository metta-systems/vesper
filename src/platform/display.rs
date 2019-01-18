/* Character cells are 8x8 */
pub const CHARSIZE_X: u32 = 8;
pub const CHARSIZE_Y: u32 = 8;

pub struct Size2d {
    pub x: u32,
    pub y: u32,
}

pub struct Color(pub u32);

impl Color {
    pub fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color(u32::from(b) << 16 | u32::from(g) << 8 | u32::from(r))
    }
}

#[derive(PartialEq)]
pub enum PixelOrder {
    BGR,
    RGB,
}

pub struct Display {
    base: u32,
    size: u32,
    depth: u32,
    pitch: u32,
    max_x: u32,
    max_y: u32,
    width: u32,
    height: u32,
    order: PixelOrder,
}

// https://github.com/david-griffith/rust-bitmap/blob/master/src/lib.rs
#[rustfmt::skip]
static CHAR_ARRAY: [u64; 95] = [
    0x0000_0000_0000_0000,                                                // space
    0x183c_3c18_1800_1800, 0x3636_0000_0000_0000, 0x3636_7f36_7f36_3600,  // ! " #
    0x0c3e_031e_301f_0c00, 0x0063_3318_0c66_6300, 0x1c36_1c6e_3b33_6e00,  // $ % &
    0x0606_0300_0000_0000, 0x180c_0606_060c_1800, 0x060c_1818_180c_0600,  // ' ( )
    0x0066_3cff_3c66_0000, 0x000c_0c3f_0c0c_0000, 0x0000_0000_000c_0c06,  // * + ,
    0x0000_003f_0000_0000, 0x0000_0000_000c_0c00, 0x6030_180c_0603_0100,  // - . /
    0x3e63_737b_6f67_3e00, 0x0c0e_0c0c_0c0c_3f00, 0x1e33_301c_0633_3f00,  // 0 1 2
    0x1e33_301c_3033_1e00, 0x383c_3633_7f30_7800, 0x3f03_1f30_3033_1e00,  // 3 4 5
    0x1c06_031f_3333_1e00, 0x3f33_3018_0c0c_0c00, 0x1e33_331e_3333_1e00,  // 6 7 8
    0x1e33_333e_3018_0e00, 0x000c_0c00_000c_0c00, 0x000c_0c00_000c_0c06,  // 9 : ;
    0x180c_0603_060c_1800, 0x0000_3f00_003f_0000, 0x060c_1830_180c_0600,  // < = >
    0x1e33_3018_0c00_0c00, 0x3e63_7b7b_7b03_1e00, 0x0c1e_3333_3f33_3300,  // ? @ A
    0x3f66_663e_6666_3f00, 0x3c66_0303_0366_3c00, 0x1f36_6666_6636_1f00,  // B C D
    0x7f46_161e_1646_7f00, 0x7f46_161e_1606_0f00, 0x3c66_0303_7366_7c00,  // E F G
    0x3333_333f_3333_3300, 0x1e0c_0c0c_0c0c_1e00, 0x7830_3030_3333_1e00,  // H I J
    0x6766_361e_3666_6700, 0x0f06_0606_4666_7f00, 0x6377_7f7f_6b63_6300,  // K L M
    0x6367_6f7b_7363_6300, 0x1c36_6363_6336_1c00, 0x3f66_663e_0606_0f00,  // N O P
    0x1e33_3333_3b1e_3800, 0x3f66_663e_3666_6700, 0x1e33_070e_3833_1e00,  // Q R S
    0x3f2d_0c0c_0c0c_1e00, 0x3333_3333_3333_3f00, 0x3333_3333_331e_0c00,  // T U V
    0x6363_636b_7f77_6300, 0x6363_361c_1c36_6300, 0x3333_331e_0c0c_1e00,  // W X Y
    0x7f63_3118_4c66_7f00, 0x1e06_0606_0606_1e00, 0x0306_0c18_3060_4000,  // Z [ \
    0x1e18_1818_1818_1e00, 0x081c_3663_0000_0000, 0x0000_0000_0000_00ff,  // ] ^ _
    0x0c0c_1800_0000_0000, 0x0000_1e30_3e33_6e00, 0x0706_063e_6666_3b00,  // ` a b
    0x0000_1e33_0333_1e00, 0x3830_303e_3333_6e00, 0x0000_1e33_3f03_1e00,  // c d e
    0x1c36_060f_0606_0f00, 0x0000_6e33_333e_301f, 0x0706_366e_6666_6700,  // f g h
    0x0c00_0e0c_0c0c_1e00, 0x3000_3030_3033_331e, 0x0706_6636_1e36_6700,  // i j k
    0x0e0c_0c0c_0c0c_1e00, 0x0000_337f_7f6b_6300, 0x0000_1f33_3333_3300,  // l m n
    0x0000_1e33_3333_1e00, 0x0000_3b66_663e_060f, 0x0000_6e33_333e_3078,  // o p q
    0x0000_3b6e_6606_0f00, 0x0000_3e03_1e30_1f00, 0x080c_3e0c_0c2c_1800,  // r s t
    0x0000_3333_3333_6e00, 0x0000_3333_331e_0c00, 0x0000_636b_7f7f_3600,  // u v w
    0x0000_6336_1c36_6300, 0x0000_3333_333e_301f, 0x0000_3f19_0c26_3f00,  // x y z
    0x380c_0c07_0c0c_3800, 0x1818_1800_1818_1800, 0x070c_0c38_0c0c_0700,  // { | }
    0x6e3b_0000_0000_0000,                                                // ~
];

impl Display {
    pub fn new(
        base: u32,
        size: u32,
        depth: u32,
        pitch: u32,
        max_x: u32,
        max_y: u32,
        width: u32,
        height: u32,
        order: PixelOrder,
    ) -> Self {
        Display {
            base,
            size,
            depth,
            pitch,
            max_x,
            max_y,
            width,
            height,
            order,
        }
    }

    #[inline]
    fn color_component(&self, chan: u16) -> u32 {
        u32::from(if self.order == PixelOrder::BGR {
            2 - chan
        } else {
            chan
        })
    }

    #[inline(never)]
    fn write_pixel_component(&self, x: u32, y: u32, chan: u16, c: u32) {
        unsafe {
            *(self.base as *mut u8).offset(
                (y * self.pitch
                    + x * 4//(self.depth / 8)
                    + self.color_component(chan)) as isize,
            ) = c as u8;
        }
    }

    /// Set a pixel value on display at given coordinates.
    #[inline(never)]
    pub fn putpixel(&mut self, x: u32, y: u32, color: u32) {
        self.write_pixel_component(x, y, 0, color & 0xff);
        self.write_pixel_component(x, y, 1, (color >> 8) & 0xff);
        self.write_pixel_component(x, y, 2, (color >> 16) & 0xff);
    }

    pub fn rect(&mut self, x1: u32, y1: u32, x2: u32, y2: u32, color: u32) {
        for y in y1..y2 {
            for x in x1..x2 {
                self.putpixel(x, y, color);
            }
        }
    }

    pub fn draw_text(&mut self, x: u32, y: u32, text: &str, color: u32) {
        for i in 0..8 {
            // Take an 8 bit slice from each array value.
            for (char_off, my_char) in text.as_bytes().iter().enumerate() {
                let off = (char_off * 8) as u32;

                if (*my_char as isize - 0x20 > 95) || (*my_char as isize - 0x20 < 0) {
                    return; // Err("Character not in font.");
                }

                let mut myval = CHAR_ARRAY[*my_char as usize - 0x20];
                myval = myval.swap_bytes();
                // do initial shr.
                myval >>= i * 8;
                for mycount in 0..8 {
                    if myval & 1 == 1 {
                        self.putpixel(x + off + mycount, y + i, color);
                    }
                    myval >>= 1;
                    if myval == 0 {
                        break;
                    }
                }
            }
        }
    }
}
