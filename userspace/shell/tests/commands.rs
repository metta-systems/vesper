//! Host tests of the shell's command interpreter.

#[cfg(test)]
mod tests {
    use shell::{
        Board, Command, Console, Edit, Failure, Font, HELP, LINE_BYTES, LineEditor, Outcome,
        execute, parse, print_number,
    };

    #[derive(Default)]
    struct Screen(String);

    impl Console for Screen {
        fn print(&mut self, text: &str) {
            self.0.push_str(text);
        }
    }

    #[derive(Default)]
    struct FakeBoard {
        led: Option<bool>,
        drawn: usize,
        fail: Option<Failure>,
    }

    impl Board for FakeBoard {
        fn set_led(&mut self, on: bool) -> Result<(), Failure> {
            match self.fail {
                Some(failure) => Err(failure),
                None => {
                    self.led = Some(on);
                    Ok(())
                }
            }
        }

        fn draw(&mut self) -> Result<(), Failure> {
            match self.fail {
                Some(failure) => Err(failure),
                None => {
                    self.drawn += 1;
                    Ok(())
                }
            }
        }
    }

    fn run(line: &str, board: &mut FakeBoard) -> (Outcome, String) {
        let mut screen = Screen::default();
        let outcome = execute(parse(line.as_bytes()), &mut screen, board);
        (outcome, screen.0)
    }

    #[test]
    fn commands_parse_from_words() {
        assert_eq!(parse(b""), Command::Empty);
        assert_eq!(parse(b"   \t "), Command::Empty);
        assert_eq!(parse(b"help"), Command::Help);
        assert_eq!(parse(b"  help  "), Command::Help);
        assert_eq!(parse(b"led on"), Command::LedOn);
        assert_eq!(parse(b"led   off"), Command::LedOff);
        assert_eq!(parse(b"\tled\ton "), Command::LedOn);
        assert_eq!(parse(b"draw"), Command::Draw);
        assert_eq!(parse(b"end"), Command::End);
    }

    #[test]
    fn everything_else_is_unknown_and_trimmed() {
        for text in [
            &b"feats"[..],
            b"mmu",
            b"map",
            b"trap",
            b"led",
            b"led on now",
            b"led blink",
            b"help me",
            b"draw twice",
            b"end it",
            b"HELP",
        ] {
            assert_eq!(parse(text), Command::Unknown(text), "{text:?}");
        }
        assert_eq!(parse(b"  feats  "), Command::Unknown(b"feats"));
        assert_eq!(parse(b" led\x01 on"), Command::Unknown(b"led\x01 on"));
    }

    #[test]
    fn the_line_editor_collects_printable_bytes() {
        let mut editor = LineEditor::new();
        for &byte in b"led on" {
            assert_eq!(editor.push(byte), Edit::Echo(byte));
        }
        assert_eq!(editor.line(), b"led on");
        assert_eq!(editor.push(b'\n'), Edit::Line);
        assert_eq!(editor.line(), b"led on", "the line stays until cleared");
        editor.clear();
        assert_eq!(editor.line(), b"");
    }

    #[test]
    fn backspace_erases_and_stops_at_the_start() {
        let mut editor = LineEditor::new();
        assert_eq!(editor.push(0x08), Edit::Nothing);
        editor.push(b'h');
        editor.push(b'x');
        assert_eq!(editor.push(0x7f), Edit::Erase);
        editor.push(b'i');
        assert_eq!(editor.line(), b"hi");
        assert_eq!(LineEditor::ERASE, "\x08 \x08");
    }

    #[test]
    fn a_carriage_return_line_feed_pair_is_one_line_ending() {
        let mut editor = LineEditor::new();
        editor.push(b'e');
        assert_eq!(editor.push(b'\r'), Edit::Line);
        assert_eq!(editor.push(b'\n'), Edit::Nothing);
        editor.clear();
        // A lone line feed ends a line, and so does a lone carriage return.
        assert_eq!(editor.push(b'\n'), Edit::Line);
        assert_eq!(editor.push(b'\r'), Edit::Line);
        assert_eq!(editor.push(b'\r'), Edit::Line);
        assert_eq!(editor.push(b'x'), Edit::Echo(b'x'));
    }

    #[test]
    fn the_line_editor_drops_control_bytes_and_stops_at_its_limit() {
        let mut editor = LineEditor::new();
        for byte in [0x00, 0x01, 0x1b, 0x80, 0xff] {
            assert_eq!(editor.push(byte), Edit::Nothing, "{byte:#x}");
        }
        assert_eq!(editor.line(), b"");
        for _ in 0..LINE_BYTES {
            assert_eq!(editor.push(b'a'), Edit::Echo(b'a'));
        }
        assert_eq!(editor.push(b'b'), Edit::Nothing, "the line is full");
        assert_eq!(editor.line().len(), LINE_BYTES);
        assert_eq!(editor.push(0x08), Edit::Erase);
        assert_eq!(editor.push(b'c'), Edit::Echo(b'c'));
        assert_eq!(editor.line().last(), Some(&b'c'));
    }

    #[test]
    fn help_lists_the_commands() {
        let mut board = FakeBoard::default();
        let (outcome, output) = run("help", &mut board);
        assert_eq!(outcome, Outcome::Continue);
        for line in HELP {
            assert!(output.contains(line), "{line}");
        }
        for command in ["help", "led", "draw", "end"] {
            assert!(output.contains(command));
        }
        for dropped in ["mmu", "feats", "trap", "map"] {
            assert!(!output.contains(dropped), "{dropped} is not a command");
        }
        assert!(output.ends_with("\r\n"));
    }

    #[test]
    fn led_commands_switch_the_led() {
        let mut board = FakeBoard::default();
        assert_eq!(
            run("led on", &mut board),
            (Outcome::Continue, String::new())
        );
        assert_eq!(board.led, Some(true));
        assert_eq!(
            run("led off", &mut board),
            (Outcome::Continue, String::new())
        );
        assert_eq!(board.led, Some(false));
    }

    #[test]
    fn draw_and_empty_lines_and_end() {
        let mut board = FakeBoard::default();
        assert_eq!(run("draw", &mut board).0, Outcome::Continue);
        assert_eq!(board.drawn, 1);
        assert_eq!(run("", &mut board), (Outcome::Continue, String::new()));
        assert_eq!(run("end", &mut board), (Outcome::End, String::new()));
    }

    #[test]
    fn unknown_commands_are_reported_and_the_shell_goes_on() {
        let mut board = FakeBoard::default();
        let (outcome, output) = run("feats", &mut board);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(output, "[!] Unknown command feats, try 'help'\r\n");
        let mut screen = Screen::default();
        execute(Command::Unknown(b"\xff\xfe"), &mut screen, &mut board);
        assert_eq!(screen.0, "[!] Unknown command (not text), try 'help'\r\n");
    }

    #[test]
    fn a_failing_board_is_reported_with_its_status() {
        let mut board = FakeBoard {
            fail: Some(Failure {
                status: 5,
                detail: 18_446_744_073_709_551_615,
            }),
            ..FakeBoard::default()
        };
        let (outcome, output) = run("led on", &mut board);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(
            output,
            "[!] led on failed: status 5, detail 18446744073709551615\r\n"
        );
        assert_eq!(board.led, None);
        assert!(run("draw", &mut board).1.starts_with("[!] draw failed"));
    }

    #[test]
    fn numbers_print_in_decimal() {
        for (value, text) in [
            (0, "0"),
            (7, "7"),
            (10, "10"),
            (u64::MAX, "18446744073709551615"),
        ] {
            let mut screen = Screen::default();
            print_number(&mut screen, value);
            assert_eq!(screen.0, text);
        }
    }

    #[test]
    fn bundled_font_has_every_printable_glyph() {
        let bytes = include_bytes!("../../../kernel/kickstart/resources/font8x8.bin");
        let font = Font::new(bytes).expect("whole glyphs");
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
}
