//! Host tests of the drawing library.

#[cfg(test)]
mod tests {
    use vesper_graphics::{Color, DrawError, Font, Lava, Painter, PixelOrder, Surface};

    struct Canvas {
        width: u32,
        height: u32,
        pixels: Vec<u32>,
    }

    impl Canvas {
        fn new(width: u32, height: u32) -> Self {
            Self {
                width,
                height,
                pixels: vec![0; (width * height) as usize],
            }
        }

        fn at(&self, x: u32, y: u32) -> u32 {
            self.pixels[(y * self.width + x) as usize]
        }
    }

    impl Surface for Canvas {
        fn width(&self) -> u32 {
            self.width
        }
        fn height(&self) -> u32 {
            self.height
        }
        fn put(&mut self, x: u32, y: u32, pixel: u32) {
            assert!(x < self.width && y < self.height, "({x}, {y}) is outside");
            self.pixels[(y * self.width + x) as usize] = pixel;
        }
    }

    const FONT: &[u8] = include_bytes!("../../../kernel/kickstart/resources/font8x8.bin");

    #[test]
    fn colors_encode_in_the_framebuffer_order() {
        let color = Color::rgb(0x11, 0x22, 0x33);
        assert_eq!(color.encode(PixelOrder::Rgb), 0x0033_2211);
        assert_eq!(color.encode(PixelOrder::Bgr), 0x0011_2233);
        assert_eq!(Color::white().encode(PixelOrder::Rgb), 0x00ff_ffff);
        assert_eq!(Color::black().encode(PixelOrder::Bgr), 0);
    }

    #[test]
    fn rectangles_are_clipped_to_the_surface() {
        let mut canvas = Canvas::new(8, 4);
        let mut painter = Painter::new(&mut canvas, PixelOrder::Rgb);
        painter.rect(6, 2, 100, 100, Color::red());
        let red = Color::red().encode(PixelOrder::Rgb);
        assert_eq!(canvas.at(6, 2), red);
        assert_eq!(canvas.at(7, 3), red);
        assert_eq!(canvas.at(5, 2), 0);
        assert_eq!(canvas.at(6, 1), 0);
        assert_eq!(
            canvas.pixels.iter().filter(|&&pixel| pixel == red).count(),
            4
        );
    }

    #[test]
    fn clear_fills_every_pixel() {
        let mut canvas = Canvas::new(5, 3);
        Painter::new(&mut canvas, PixelOrder::Bgr).clear(Color::blue());
        assert!(canvas.pixels.iter().all(|&pixel| pixel == 0x0000_00ff));
    }

    #[test]
    fn font_has_every_printable_glyph() {
        let font = Font::new(FONT).expect("whole glyphs");
        assert_eq!(font.glyph_count(), 95);
        assert_eq!(font.glyph(b' '), Some(&[0; 8]));
        // `!`: a stem and a dot.
        assert_eq!(
            font.glyph(b'!'),
            Some(&[0x18, 0x3c, 0x3c, 0x18, 0x18, 0x00, 0x18, 0x00])
        );
        assert!(font.glyph(b'~').is_some());
        assert_eq!(font.glyph(0x7f), None);
        assert_eq!(font.glyph(0x1f), None);
        assert!(Font::new(&[0; 7]).is_none());
    }

    #[test]
    fn text_draws_each_glyph_in_its_cell() {
        let font = Font::new(FONT).expect("font");
        let mut canvas = Canvas::new(32, 16);
        let mut painter = Painter::new(&mut canvas, PixelOrder::Rgb);
        painter
            .text(4, 2, "!!", &font, Color::white())
            .expect("drawn");
        let white = Color::white().encode(PixelOrder::Rgb);
        // Row 0 of `!` is 0x18: bits 3 and 4, so columns 3 and 4 of the cell.
        assert_eq!(canvas.at(4 + 3, 2), white);
        assert_eq!(canvas.at(4 + 4, 2), white);
        assert_eq!(canvas.at(4 + 2, 2), 0);
        // The second glyph starts one cell to the right.
        assert_eq!(canvas.at(4 + 8 + 3, 2), white);
        // Row 5 of `!` is blank, row 6 is 0x18 again.
        assert_eq!(canvas.at(4 + 3, 2 + 5), 0);
        assert_eq!(canvas.at(4 + 3, 2 + 6), white);
    }

    #[test]
    fn text_stops_at_a_missing_glyph_and_clips_at_the_edge() {
        let font = Font::new(FONT).expect("font");
        let mut canvas = Canvas::new(10, 10);
        let mut painter = Painter::new(&mut canvas, PixelOrder::Rgb);
        assert_eq!(
            painter.text(0, 0, "!\u{7f}!", &font, Color::white()),
            Err(DrawError::CharacterNotInFont(0x7f))
        );
        // The first glyph is drawn, then the text clipped at the right edge.
        assert_eq!(canvas.at(3, 0), Color::white().encode(PixelOrder::Rgb));
        let mut painter = Painter::new(&mut canvas, PixelOrder::Rgb);
        painter
            .text(6, 6, "!!", &font, Color::white())
            .expect("clipped, not refused");
    }

    #[test]
    fn lava_blobs_stay_inside_and_glow() {
        let mut lava = Lava::new(40, 30, 4, 12345);
        let mut peak = 0;
        for _ in 0..2000 {
            lava.step();
            for row in (0..30).step_by(3) {
                for column in (0..40).step_by(3) {
                    peak = peak.max(lava.field(column, row));
                }
            }
        }
        // Somewhere a blob's core is above the edge level.
        assert!(peak >= 16, "peak {peak}");
    }

    #[test]
    fn lava_ramp_runs_from_dark_to_yellow() {
        assert_eq!(Lava::ramp(0), Color::black());
        assert_eq!(Lava::ramp(7), Color::black());
        let edge = Lava::ramp(8);
        assert!(edge.red > 0 && edge.green == 0 && edge.blue == 0);
        let core = Lava::ramp(255);
        assert_eq!((core.red, core.green), (255, 255));
    }

    #[test]
    fn lava_draws_inside_its_area_only() {
        let mut canvas = Canvas::new(100, 60);
        let lava = Lava::new(10, 5, 4, 7);
        let mut painter = Painter::new(&mut canvas, PixelOrder::Rgb);
        lava.draw(&mut painter, 50, 20);
        for y in 0..60 {
            for x in 0..100 {
                let inside = (50..90).contains(&x) && (20..40).contains(&y);
                if !inside {
                    assert_eq!(canvas.at(x, y), 0, "({x}, {y}) is outside the lamp");
                }
            }
        }
    }
}
