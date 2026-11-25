//! Host tests of the shell's command interpreter.

#[cfg(test)]
mod tests {
    use shell::{
        Board, Command, Console, Edit, Failure, HELP, LINE_BYTES, LineEditor, MAX_OUTPUTS, Outcome,
        Output, execute, parse, print_number,
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
        outputs: Vec<Output>,
    }

    impl FakeBoard {
        /// The board's pair: HDMI, and the panel to its right.
        fn with_outputs() -> Self {
            Self {
                outputs: vec![
                    Output {
                        id: 1,
                        x: 0,
                        y: 0,
                        width: 1024,
                        height: 600,
                        turns: 0,
                        width_mm: 180,
                        height_mm: 130,
                    },
                    Output {
                        id: 0,
                        x: 1024,
                        y: 0,
                        width: 240,
                        height: 320,
                        turns: 0,
                        width_mm: 34,
                        height_mm: 45,
                    },
                ],
                ..Self::default()
            }
        }

        fn output(&mut self, id: u32) -> Result<&mut Output, Failure> {
            self.outputs
                .iter_mut()
                .find(|output| output.id == id)
                .ok_or(Failure {
                    status: 9,
                    detail: u64::from(id),
                })
        }
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

        fn outputs(&mut self, into: &mut [Output; MAX_OUTPUTS]) -> Result<usize, Failure> {
            if let Some(failure) = self.fail {
                return Err(failure);
            }
            for (slot, output) in into.iter_mut().zip(&self.outputs) {
                *slot = *output;
            }
            Ok(self.outputs.len().min(MAX_OUTPUTS))
        }

        fn turn_output(&mut self, id: u32, turns: u8) -> Result<(), Failure> {
            let output = self.output(id)?;
            if (output.turns + turns) % 2 == 1 {
                core::mem::swap(&mut output.width, &mut output.height);
            }
            output.turns = turns;
            Ok(())
        }

        fn move_output(&mut self, id: u32, x: i32, y: i32) -> Result<(), Failure> {
            let output = self.output(id)?;
            output.x = x;
            output.y = y;
            Ok(())
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
        assert_eq!(parse(b"layout"), Command::Layout);
        assert_eq!(
            parse(b"layout turn 0 90"),
            Command::LayoutTurn { id: 0, turns: 1 }
        );
        assert_eq!(
            parse(b"layout turn 1 270"),
            Command::LayoutTurn { id: 1, turns: 3 }
        );
        assert_eq!(
            parse(b"layout move 0 -240 16"),
            Command::LayoutMove {
                id: 0,
                x: -240,
                y: 16
            }
        );
    }

    #[test]
    fn layout_commands_with_bad_numbers_are_unknown() {
        for text in [
            &b"layout turn 0 45"[..],
            b"layout turn 0",
            b"layout turn x 90",
            b"layout turn -1 90",
            b"layout move 0 1",
            b"layout move 0 a 1",
            b"layout move 0 1 2 3",
            b"layout spin",
        ] {
            assert_eq!(parse(text), Command::Unknown(text), "{text:?}");
        }
    }

    #[test]
    fn layout_lists_turns_and_moves_the_displays() {
        let mut board = FakeBoard::with_outputs();
        let (outcome, output) = run("layout", &mut board);
        assert_eq!(outcome, Outcome::Continue);
        assert_eq!(
            output,
            "display 1: 1024x600 at 0,0, turned 0, 180x130 mm\r\n\
             display 0: 240x320 at 1024,0, turned 0, 34x45 mm\r\n"
        );
        // A turn or a move lists the displays as they are afterwards.
        let (_, output) = run("layout turn 0 90", &mut board);
        assert!(
            output.contains("display 0: 320x240 at 1024,0, turned 90"),
            "{output}"
        );
        let (_, output) = run("layout move 0 -320 0", &mut board);
        assert!(output.contains("display 0: 320x240 at -320,0"), "{output}");
        // A refused one says so, and lists them still.
        let (_, output) = run("layout turn 5 180", &mut board);
        assert!(
            output.starts_with("[!] layout turn failed: status 9, detail 5\r\ndisplay 1"),
            "{output}"
        );
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
}
