#![no_std]

//! The shell's command interpreter: line editing, command parsing and the
//! commands themselves, independent of the hardware so they run on the host
//! as well.
//!
//! The commands (see [`HELP`]) are `help`, `led on`, `led off`, `draw`,
//! `layout` (list, turn and move the compositor's outputs) and `end`. They act
//! through a [`Board`], which the EL0 program implements with calls into the
//! mailbox and power components and the compositor, and print through a
//! [`Console`], which it implements on the serial channel.

/// Longest command line, in bytes; more are ignored until the line ends.
pub const LINE_BYTES: usize = 64;

/// Outputs `layout` lists at most.
pub const MAX_OUTPUTS: usize = 4;

/// One parsed command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'line> {
    /// A blank line
    Empty,
    Help,
    LedOn,
    LedOff,
    Draw,
    /// List the compositor's outputs
    Layout,
    /// Turn output `id` to `turns` clockwise quarter turns
    LayoutTurn {
        id: u32,
        turns: u8,
    },
    /// Put output `id`'s top left corner at (`x`, `y`) of the desktop
    LayoutMove {
        id: u32,
        x: i32,
        y: i32,
    },
    /// Leave the shell and reset the board
    End,
    /// A line that is no command, as typed (trimmed)
    Unknown(&'line [u8]),
}

/// One of the compositor's outputs, as `layout` lists it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub id: u32,
    /// Its viewport: the desktop rectangle it shows.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Clockwise quarter turns.
    pub turns: u8,
    /// Its physical size upright, in millimetres (0 when unknown).
    pub width_mm: u32,
    pub height_mm: u32,
}

/// The decimal number `word`, if it is one.
fn number(word: &[u8]) -> Option<u32> {
    core::str::from_utf8(word).ok()?.parse().ok()
}

/// The decimal number `word`, perhaps negative, if it is one.
fn signed(word: &[u8]) -> Option<i32> {
    core::str::from_utf8(word).ok()?.parse().ok()
}

/// The quarter turns of a turn in degrees: 0, 90, 180 or 270.
fn quarter_turns(word: &[u8]) -> Option<u8> {
    match word {
        b"0" => Some(0),
        b"90" => Some(1),
        b"180" => Some(2),
        b"270" => Some(3),
        _ => None,
    }
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
    let mut word = [&[][..]; 6];
    let mut count = 0;
    for (slot, part) in word.iter_mut().zip(&mut parts) {
        *slot = part;
        count += 1;
    }
    if count == 0 {
        return Command::Empty;
    }
    let unknown = Command::Unknown(trim(line));
    if parts.next().is_some() {
        return unknown;
    }
    let parsed = match &word[..count] {
        [b"help"] => Some(Command::Help),
        [b"led", b"on"] => Some(Command::LedOn),
        [b"led", b"off"] => Some(Command::LedOff),
        [b"draw"] => Some(Command::Draw),
        [b"layout"] => Some(Command::Layout),
        [b"layout", b"turn", id, degrees] => number(id)
            .zip(quarter_turns(degrees))
            .map(|(id, turns)| Command::LayoutTurn { id, turns }),
        [b"layout", b"move", id, x, y] => match (number(id), signed(x), signed(y)) {
            (Some(id), Some(x), Some(y)) => Some(Command::LayoutMove { id, x, y }),
            _ => None,
        },
        [b"end"] => Some(Command::End),
        _ => None,
    };
    parsed.unwrap_or(unknown)
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

    /// Paint the static graphics and start the lava blobs.
    fn draw(&mut self) -> Result<(), Failure>;

    /// The compositor's outputs, into `into`; how many there are.
    fn outputs(&mut self, into: &mut [Output; MAX_OUTPUTS]) -> Result<usize, Failure>;

    /// Turn output `id` to `turns` clockwise quarter turns.
    fn turn_output(&mut self, id: u32, turns: u8) -> Result<(), Failure>;

    /// Put output `id`'s top left corner at (`x`, `y`) of the desktop.
    fn move_output(&mut self, id: u32, x: i32, y: i32) -> Result<(), Failure>;
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
    "  draw          - draw some graphics and lava blobs on the display",
    "  layout        - list the displays: id, place on the desktop, turn, size",
    "  layout turn <id> <0|90|180|270> - turn display <id> clockwise",
    "  layout move <id> <x> <y>        - put display <id>'s corner at <x>,<y>",
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
        Command::Layout => list_outputs(console, board),
        Command::LayoutTurn { id, turns } => {
            report(console, "layout turn", board.turn_output(id, turns));
            list_outputs(console, board);
        }
        Command::LayoutMove { id, x, y } => {
            report(console, "layout move", board.move_output(id, x, y));
            list_outputs(console, board);
        }
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

/// Print the compositor's outputs, a line each:
/// `display 1: 240x320 at 1024,0, turned 90, 34x45 mm`.
fn list_outputs(console: &mut dyn Console, board: &mut dyn Board) {
    let mut outputs = [Output::default(); MAX_OUTPUTS];
    let count = match board.outputs(&mut outputs) {
        Ok(count) => count,
        Err(failure) => {
            report(console, "layout", Err(failure));
            return;
        }
    };
    for output in outputs.iter().take(count) {
        console.print("display ");
        print_number(console, u64::from(output.id));
        console.print(": ");
        print_number(console, u64::from(output.width));
        console.print("x");
        print_number(console, u64::from(output.height));
        console.print(" at ");
        print_signed(console, output.x);
        console.print(",");
        print_signed(console, output.y);
        console.print(", turned ");
        print_number(console, u64::from(output.turns) * 90);
        console.print(", ");
        print_number(console, u64::from(output.width_mm));
        console.print("x");
        print_number(console, u64::from(output.height_mm));
        console.print(" mm\r\n");
    }
}

/// Print `value` in decimal, with a minus sign when it is negative.
fn print_signed(console: &mut dyn Console, value: i32) {
    if value < 0 {
        console.print("-");
    }
    print_number(console, u64::from(value.unsigned_abs()));
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

// FIXME: use normal formatting fns
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
