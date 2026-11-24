//! Host tests of the panel's screen geometry: which rectangles `flush`
//! accepts, the rows it reads, and the size after a turn.

#[cfg(test)]
mod tests {
    use panel_ili9341::{BYTES_PER_PIXEL, ROTATIONS, Rectangle, Screen, rgb565_be};

    /// The MSP2202, unturned.
    const PANEL: Screen = Screen {
        width: 240,
        height: 320,
    };

    const fn rectangle(x: u32, y: u32, width: u32, height: u32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_whole_screen_fits_the_display_v1_data_area() {
        assert_eq!(PANEL.pitch(), 480);
        assert_eq!(PANEL.bytes(), 153_600);
        // `data 155648;` in display_v1.if: the screen in whole pages, and
        // not a page more.
        assert!(PANEL.bytes() <= 155_648);
        assert!(PANEL.bytes() > 155_648 - 4096);
        assert_eq!(BYTES_PER_PIXEL, 2);
    }

    #[test]
    fn a_turn_swaps_width_and_height_on_quarter_turns_only() {
        assert_eq!(PANEL.turned(0), PANEL);
        assert_eq!(
            PANEL.turned(1),
            Screen {
                width: 320,
                height: 240
            }
        );
        assert_eq!(PANEL.turned(2), PANEL);
        assert_eq!(PANEL.turned(3), PANEL.turned(1));
        // Turned or not, the screen is the same bytes, so it fits the area.
        assert_eq!(PANEL.turned(1).bytes(), PANEL.bytes());
        assert_eq!(PANEL.turned(1).pitch(), 640);
        assert_eq!(ROTATIONS, 0b1111, "the controller does all four turns");
    }

    #[test]
    fn only_non_empty_rectangles_inside_the_screen_are_accepted() {
        assert!(rectangle(0, 0, 240, 320).within(PANEL));
        assert!(rectangle(239, 319, 1, 1).within(PANEL));
        assert!(!rectangle(0, 0, 0, 10).within(PANEL), "no width");
        assert!(!rectangle(0, 0, 10, 0).within(PANEL), "no height");
        assert!(!rectangle(1, 0, 240, 1).within(PANEL), "a column too far");
        assert!(!rectangle(0, 300, 1, 21).within(PANEL), "a row too far");
        assert!(!rectangle(240, 0, 1, 1).within(PANEL));
        assert!(
            !rectangle(u32::MAX, 0, 2, 1).within(PANEL),
            "an end that overflows"
        );
        // A turned screen takes rectangles of its own shape.
        assert!(rectangle(0, 0, 320, 240).within(PANEL.turned(1)));
        assert!(!rectangle(0, 0, 320, 240).within(PANEL));
    }

    #[test]
    fn the_last_column_and_row_are_inclusive() {
        assert_eq!(rectangle(0, 0, 240, 320).last(), (239, 319));
        assert_eq!(rectangle(10, 20, 1, 1).last(), (10, 20));
    }

    #[test]
    fn rows_are_the_rectangles_byte_spans_top_to_bottom() {
        let spans: Vec<_> = rectangle(10, 2, 4, 3).rows(PANEL).collect();
        assert_eq!(
            spans,
            [(2 * 480 + 20, 8), (3 * 480 + 20, 8), (4 * 480 + 20, 8)]
        );
        // The whole screen is its rows back to back.
        let whole: Vec<_> = rectangle(0, 0, 240, 320).rows(PANEL).collect();
        assert_eq!(whole.len(), 320);
        assert_eq!(whole.first(), Some(&(0, 480)));
        assert_eq!(whole.last(), Some(&(319 * 480, 480)));
        assert_eq!(
            whole.iter().map(|(_, length)| length).sum::<u64>(),
            PANEL.bytes()
        );
        // On a turned screen rows are its own, longer ones.
        let turned: Vec<_> = rectangle(0, 1, 320, 1).rows(PANEL.turned(1)).collect();
        assert_eq!(turned, [(640, 640)]);
    }

    #[test]
    fn pixels_are_rgb565_high_byte_first() {
        assert_eq!(rgb565_be([0xf8, 0x00]), 0xf800, "red");
        assert_eq!(rgb565_be([0x07, 0xe0]), 0x07e0, "green");
        assert_eq!(rgb565_be([0x00, 0x1f]), 0x001f, "blue");
    }
}
