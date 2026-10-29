/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The `VideoCore` mailbox, property channel only: the ARM sends a buffer holding
//! one tag, the firmware answers in the same buffer.
//!
//! <https://github.com/raspberrypi/firmware/wiki/Mailboxes> and
//! <https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface>
//!
//! Its users run with the MMU off, or chainboot on a flat map with the data cache
//! off, so a buffer's physical address is its address, and nothing caches it.
//! Every wait is bounded: a firmware that does not answer must not stop the boot.

use {
    crate::PERIPHERALS,
    aarch64_cpu::asm::barrier,
    arbitrary_int::{u4, u28},
    bitbybit::bitfield,
    core::{ptr, time::Duration},
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
    libtime::time::time_manager,
};

/// Tags, from the firmware's property interface.
pub mod tag {
    /// The board's revision code: no request, answer the code.
    pub const GET_BOARD_REVISION: u32 = 0x0001_0002;
    /// The state of a clock: request the clock's id, answer its id and state
    /// (bit 0 set: the clock is on, bit 1 set: there is no such clock).
    pub const GET_CLOCK_STATE: u32 = 0x0003_0001;
    /// Set the state of a clock: request the clock's id and the state (bit 0 set
    /// runs the clock), answer the same.
    pub const SET_CLOCK_STATE: u32 = 0x0003_8001;
    /// The rate of a clock: request the clock's id, answer its id and rate in Hz.
    pub const GET_CLOCK_RATE: u32 = 0x0003_0002;
    /// The state of a GPIO the firmware owns: request the pin and the state.
    pub const SET_GPIO_STATE: u32 = 0x0003_8041;
}

/// How long a wait for the mailbox or the firmware's answer may take.
const TIMEOUT: Duration = Duration::from_millis(100);

/// The property channel.
const PROPERTY_CHANNEL: u4 = u4::new(8);

/// Buffer code of a buffer the firmware answered, and bit 31 of a tag's code word
/// once it answered the tag.
const ANSWERED: u32 = 0x8000_0000;

/// Mailbox status.
#[bitfield(u32, default = 0, debug)]
struct Status {
    /// The mailbox holds no word
    #[bit(30, r)]
    empty: bool,
    /// The mailbox cannot take another word
    #[bit(31, r)]
    full: bool,
}

/// A mailbox word: a channel and the top 28 bits of a 16-byte aligned bus address.
#[bitfield(u32, default = 0, debug)]
struct Word {
    #[bits(0..=3, rw)]
    channel: u4,
    #[bits(4..=31, rw)]
    address: u28,
}

/// The registers: mailbox 0 carries the firmware's answers to the ARM, mailbox 1
/// the ARM's requests to the firmware.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
struct RegisterBlock {
    #[mmio(Read)]
    from_firmware: Word,
    _reserved0: [u32; 5],
    #[mmio(PureRead)]
    from_firmware_status: Status,
    _reserved1: [u32; 1],
    #[mmio(Write)]
    to_firmware: Word,
    _reserved2: [u32; 5],
    #[mmio(PureRead)]
    to_firmware_status: Status,
}

assert_register_offsets!(RegisterBlock, 0x3c, {
    from_firmware: 0x00,
    from_firmware_status: 0x18,
    to_firmware: 0x20,
    to_firmware_status: 0x38,
});

/// Where the registers are.
const BASE: usize = PERIPHERALS + 0xb880;

/// The address the `VideoCore` reaches the ARM's memory at, uncached: the physical
/// address with these bits set.
const BUS_ALIAS: usize = 0xc000_0000;

/// Words of a message: the header, one tag with two value words, and the end tag.
const WORDS: usize = 8;

/// Bytes of a message, and of the value buffer of its tag.
const SIZE: u32 = 32;
const VALUE_BYTES: u32 = 8;
const _: () = assert!(SIZE as usize == size_of::<[u32; WORDS]>());

/// A message with one tag. The value buffer has room for two words, enough for
/// every tag sent here, for the request and for the answer.
#[repr(C, align(16))]
pub struct Message {
    words: [u32; WORDS],
}

impl Message {
    /// A request for `tag` with the value words `request`; more than two are cut.
    pub const fn new(tag: u32, request: &[u32]) -> Self {
        let mut words = [0; WORDS];
        words[0] = SIZE;
        // words[1], the buffer's request code, is zero.
        words[2] = tag;
        words[3] = VALUE_BYTES;
        // words[4], the tag's request code, is zero.
        let mut index = 0;
        while index < request.len() && index < VALUE_BYTES as usize / 4 {
            words[5 + index] = request[index];
            index += 1;
        }
        // words[7] is the end tag.
        Self { words }
    }

    /// The two value words of the answer, if the firmware answered the tag. Read
    /// with volatile loads: the firmware writes the message behind the compiler's back.
    pub fn answer(&self) -> Option<[u32; 2]> {
        let word = |index: usize| -> u32 {
            // SAFETY: `index` is below `WORDS`, inside the message.
            unsafe { ptr::read_volatile(self.words.as_ptr().add(index)) }
        };
        (word(1) == ANSWERED && word(4) & ANSWERED != 0).then(|| [word(5), word(6)])
    }

    /// The word that tells the firmware where the message is. The address
    /// escapes: the compiler has to treat the message as read and written by
    /// someone else from here on.
    fn word(&self) -> Word {
        let bus = (ptr::from_ref(self).expose_provenance() | BUS_ALIAS) >> 4;
        Word::ZERO
            .with_channel(PROPERTY_CHANNEL)
            .with_address(u28::new(u32::try_from(bus).unwrap_or(0)))
    }
}

/// Wait until `done` holds; `false` when the wait took longer than [`TIMEOUT`].
pub fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let start = time_manager().uptime();
    while !done() {
        let elapsed = time_manager()
            .uptime()
            .checked_sub(start)
            .unwrap_or(Duration::ZERO);
        if elapsed > TIMEOUT {
            return false;
        }
    }
    true
}

/// The mailbox. There is one core and nobody else uses it, so the handle is made
/// where it is used.
fn mailbox() -> MmioRegisterBlock<'static> {
    // SAFETY: `BASE` is the mailbox's address; its users run on one core with the
    // MMU off, and the mailbox is used from one place at a time.
    unsafe { RegisterBlock::new_mmio(ptr::with_exposed_provenance_mut(BASE)) }
}

/// Hand `message` to the firmware without waiting for the answer. Fails when the
/// mailbox does not take it. The message must stay where it is, unchanged, until
/// [`answered`] says the firmware is done with it.
pub fn send(message: &Message) -> Result<(), &'static str> {
    let mut registers = mailbox();
    if !wait_until(|| !registers.read_to_firmware_status().full()) {
        return Err("mailbox stayed full");
    }
    // The message is in memory before the firmware is told about it.
    barrier::dsb(barrier::SY);
    registers.write_to_firmware(message.word());
    Ok(())
}

/// Whether the firmware has answered a message: takes its answer word off the
/// mailbox, and never waits.
pub fn answered() -> bool {
    let mut registers = mailbox();
    if registers.read_from_firmware_status().empty() {
        return false;
    }
    let _word = registers.read_from_firmware();
    barrier::dsb(barrier::SY);
    true
}

/// Send `tag` with the value words `request` and wait for the answer's two value
/// words.
///
/// # Errors
///
/// If the mailbox or the firmware does not answer in time, or the firmware
/// refuses the tag.
pub fn call(tag: u32, request: &[u32]) -> Result<[u32; 2], &'static str> {
    let message = Message::new(tag, request);
    send(&message)?;
    if !wait_until(answered) {
        return Err("the firmware did not answer");
    }
    message.answer().ok_or("the firmware refused the tag")
}

/// The board's revision code, which tells the model.
///
/// # Errors
///
/// If the firmware does not answer.
pub fn board_revision() -> Result<u32, &'static str> {
    let [revision, _unused] = call(tag::GET_BOARD_REVISION, &[])?;
    Ok(revision)
}

/// The firmware's id of the PL011's clock.
const UART_CLOCK: u32 = 2;

/// Ask the firmware to run the UART clock. It leaves the clock stopped when it
/// does not use the UART itself, and a stopped clock means a UART that never
/// sends a byte and may hang the core that touches its registers.
///
/// # Errors
///
/// If the firmware does not answer.
pub fn start_uart_clock() -> Result<(), &'static str> {
    const ON: u32 = 1;
    call(tag::SET_CLOCK_STATE, &[UART_CLOCK, ON]).map(|_state| ())
}

/// Whether the UART clock is running.
///
/// # Errors
///
/// If the firmware does not answer.
pub fn uart_clock_running() -> Result<bool, &'static str> {
    const ON: u32 = 1;
    const NO_SUCH_CLOCK: u32 = 2;
    let [clock, state] = call(tag::GET_CLOCK_STATE, &[UART_CLOCK])?;
    Ok(clock == UART_CLOCK && state & ON != 0 && state & NO_SUCH_CLOCK == 0)
}

/// The rate of the UART clock in Hz.
///
/// # Errors
///
/// If the firmware does not answer, or answers a clock with no rate.
pub fn uart_clock_rate() -> Result<u32, &'static str> {
    let [clock, rate] = call(tag::GET_CLOCK_RATE, &[UART_CLOCK])?;
    if clock != UART_CLOCK || rate == 0 {
        return Err("the firmware reports no UART clock rate");
    }
    Ok(rate)
}
