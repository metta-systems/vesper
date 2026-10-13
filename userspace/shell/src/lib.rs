#![no_std]

//! The shell's command interpreter: line editing, command parsing and the
//! commands themselves, independent of the hardware so they run on the host
//! as well.
//!
//! The commands (see [`HELP`]) are `help`, `led on`, `led off`, `draw` and
//! `end`. They act through a [`Board`], which the EL0 program implements with
//! calls into the mailbox, power and fb components, and print through a
//! [`Console`], which it implements on the serial channel.

/// The console font resource: 8x8 glyphs for the characters from space to `~`,
/// 8 bytes each, top row first, bit 0 the leftmost pixel.
#[derive(Clone, Copy, Debug)]
pub struct Font<'data> {
    data: &'data [u8],
}

impl<'data> Font<'data> {
    /// Bytes of one glyph.
    pub const GLYPH_BYTES: usize = 8;
    /// The first character the font has.
    pub const FIRST: u8 = b' ';

    /// The font held in `data`, if it is a whole number of glyphs.
    pub const fn new(data: &'data [u8]) -> Option<Self> {
        if data.is_empty() || !data.len().is_multiple_of(Self::GLYPH_BYTES) {
            None
        } else {
            Some(Self { data })
        }
    }

    /// How many glyphs the font has.
    pub const fn glyph_count(&self) -> usize {
        self.data.len() / Self::GLYPH_BYTES
    }

    /// The rows of the glyph for `character`, if the font has it.
    pub fn glyph(&self, character: u8) -> Option<&'data [u8; 8]> {
        let index = usize::from(character.checked_sub(Self::FIRST)?);
        self.data
            .get(index * Self::GLYPH_BYTES..)?
            .first_chunk::<8>()
    }
}

/// Longest command line, in bytes; more are ignored until the line ends.
pub const LINE_BYTES: usize = 64;

/// One parsed command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'line> {
    /// A blank line
    Empty,
    Help,
    LedOn,
    LedOff,
    Draw,
    /// Leave the shell and reset the board
    End,
    /// A line that is no command, as typed (trimmed)
    Unknown(&'line [u8]),
}

fn is_space(byte: u8) -> bool {
    byte.is_ascii_whitespace()
}

/// The words of `line`: runs of bytes between whitespace.
fn words(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|&byte| is_space(byte))
        .filter(|word| !word.is_empty())
}

/// The command `line` names. Words are separated by any whitespace.
pub fn parse(line: &[u8]) -> Command<'_> {
    let mut parts = words(line);
    let (Some(first), second, third) = (parts.next(), parts.next(), parts.next()) else {
        return Command::Empty;
    };
    match (first, second, third) {
        (b"help", None, None) => Command::Help,
        (b"led", Some(b"on"), None) => Command::LedOn,
        (b"led", Some(b"off"), None) => Command::LedOff,
        (b"draw", None, None) => Command::Draw,
        (b"end", None, None) => Command::End,
        _ => Command::Unknown(trim(line)),
    }
}

/// `line` without leading and trailing whitespace.
fn trim(line: &[u8]) -> &[u8] {
    let start = line
        .iter()
        .position(|&byte| !is_space(byte))
        .unwrap_or(line.len());
    let end = line
        .iter()
        .rposition(|&byte| !is_space(byte))
        .map_or(start, |last| last + 1);
    &line[start..end]
}

/// What a line editor did with a byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Nothing to show
    Nothing,
    /// Show the byte just added to the line
    Echo(u8),
    /// A byte was erased: show the terminal sequence that erases it
    Erase,
    /// The line is complete: [`LineEditor::line`]
    Line,
}

/// Collects typed bytes into a line: printable bytes are added, backspace
/// erases, carriage return or line feed ends the line.
#[derive(Clone, Debug)]
pub struct LineEditor {
    buffer: [u8; LINE_BYTES],
    len: usize,
    /// The last byte was a carriage return, so a line feed right after it is
    /// part of the same line ending.
    after_return: bool,
}

impl LineEditor {
    pub const fn new() -> Self {
        Self {
            buffer: [0; LINE_BYTES],
            len: 0,
            after_return: false,
        }
    }

    /// The terminal sequence that erases the character before the cursor.
    pub const ERASE: &'static str = "\x08 \x08";

    /// Feed one received byte.
    pub fn push(&mut self, byte: u8) -> Edit {
        let after_return = core::mem::take(&mut self.after_return);
        match byte {
            b'\r' => {
                self.after_return = true;
                Edit::Line
            }
            b'\n' if after_return => Edit::Nothing,
            b'\n' => Edit::Line,
            0x08 | 0x7f => {
                if self.len == 0 {
                    Edit::Nothing
                } else {
                    self.len -= 1;
                    Edit::Erase
                }
            }
            0x20..=0x7e if self.len < LINE_BYTES => {
                self.buffer[self.len] = byte;
                self.len += 1;
                Edit::Echo(byte)
            }
            _ => Edit::Nothing,
        }
    }

    /// The line typed so far.
    pub fn line(&self) -> &[u8] {
        &self.buffer[..self.len]
    }

    /// Start a new line.
    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

/// Where the shell prints.
pub trait Console {
    fn print(&mut self, text: &str);
}

/// Why a [`Board`] operation failed: its status and one detail word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub status: u64,
    pub detail: u64,
}

/// What the commands act on.
pub trait Board {
    /// Switch the activity LED.
    fn set_led(&mut self, on: bool) -> Result<(), Failure>;

    /// Draw the demo graphics.
    fn draw(&mut self) -> Result<(), Failure>;
}

/// What a command asks of the shell loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    /// Leave the shell
    End,
}

/// The help text, one line per entry.
pub const HELP: &[&str] = &[
    "Supported console commands:",
    "  help          - print this help",
    "  led [on|off]  - change RPi LED status",
    "  draw          - draw some graphics on the display",
    "  end           - leave console and reset board",
];

/// The prompt printed before each line.
pub const PROMPT: &str = "> ";

/// Print `lines`, each followed by a line break.
fn print_lines(console: &mut dyn Console, lines: &[&str]) {
    for line in lines {
        console.print(line);
        console.print("\r\n");
    }
}

/// Run `command`, printing its output.
pub fn execute(command: Command<'_>, console: &mut dyn Console, board: &mut dyn Board) -> Outcome {
    match command {
        Command::Empty => {}
        Command::Help => print_lines(console, HELP),
        Command::LedOn => report(console, "led on", board.set_led(true)),
        Command::LedOff => report(console, "led off", board.set_led(false)),
        Command::Draw => report(console, "draw", board.draw()),
        Command::End => return Outcome::End,
        Command::Unknown(text) => {
            console.print("[!] Unknown command ");
            match core::str::from_utf8(text) {
                Ok(text) => console.print(text),
                Err(_) => console.print("(not text)"),
            }
            console.print(", try 'help'\r\n");
        }
    }
    Outcome::Continue
}

/// Print a failure of `what`; silent on success.
fn report(console: &mut dyn Console, what: &str, result: Result<(), Failure>) {
    if let Err(failure) = result {
        console.print("[!] ");
        console.print(what);
        console.print(" failed: status ");
        print_number(console, failure.status);
        console.print(", detail ");
        print_number(console, failure.detail);
        console.print("\r\n");
    }
}

/// Print `value` in decimal.
pub fn print_number(console: &mut dyn Console, mut value: u64) {
    let mut digits = [0_u8; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        // The remainder of a division by ten is a single digit.
        digits[start] = b'0' + u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
        if value == 0 {
            break;
        }
    }
    if let Ok(text) = core::str::from_utf8(&digits[start..]) {
        console.print(text);
    }
}
