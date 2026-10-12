#![no_std]

//! Checks of the property buffers clients send through `mailbox_v1.property`.
//!
//! A property buffer is a header (total size in bytes, request code), a
//! sequence of tags (tag id, value buffer size in bytes, request code, the
//! value buffer padded to 32 bits) and an end tag (zero). The firmware
//! answers in place. The mailbox component copies a client's buffer into its
//! own page first and checks the copy here, so the client cannot change it
//! after the check.
//!
//! Only tags known to be safe for any client are allowed: read-only queries,
//! and the tags configuring the display. Everything else is refused, tags
//! that hand over the system (code execution, allocating and locking
//! `VideoCore` memory, cursor images read from `ARM` memory) and tags that
//! change clocks or power, which have typed functions or wait for per-client
//! policy.

/// Tag ids, from the firmware's "Mailbox property interface".
pub mod tag {
    /// Read-only queries.
    pub const GET_FIRMWARE_REVISION: u32 = 0x0000_0001;
    pub const GET_BOARD_MODEL: u32 = 0x0001_0001;
    pub const GET_BOARD_REVISION: u32 = 0x0001_0002;
    pub const GET_BOARD_MAC_ADDRESS: u32 = 0x0001_0003;
    pub const GET_BOARD_SERIAL: u32 = 0x0001_0004;
    pub const GET_ARM_MEMORY: u32 = 0x0001_0005;
    pub const GET_VC_MEMORY: u32 = 0x0001_0006;
    pub const GET_CLOCKS: u32 = 0x0001_0007;
    pub const GET_POWER_STATE: u32 = 0x0002_0001;
    pub const GET_TIMING: u32 = 0x0002_0002;
    pub const GET_CLOCK_STATE: u32 = 0x0003_0001;
    pub const GET_CLOCK_RATE: u32 = 0x0003_0002;
    pub const GET_VOLTAGE: u32 = 0x0003_0003;
    pub const GET_MAX_CLOCK_RATE: u32 = 0x0003_0004;
    pub const GET_TEMPERATURE: u32 = 0x0003_0006;
    pub const GET_MIN_CLOCK_RATE: u32 = 0x0003_0007;
    pub const GET_MAX_TEMPERATURE: u32 = 0x0003_000a;

    /// Display configuration: allocating and releasing the framebuffer (in
    /// `VideoCore` memory the firmware owns), and its geometry.
    pub const ALLOCATE_BUFFER: u32 = 0x0004_0001;
    pub const RELEASE_BUFFER: u32 = 0x0004_8001;
    pub const BLANK_SCREEN: u32 = 0x0004_0002;
    pub const GET_PHYSICAL_SIZE: u32 = 0x0004_0003;
    pub const TEST_PHYSICAL_SIZE: u32 = 0x0004_4003;
    pub const SET_PHYSICAL_SIZE: u32 = 0x0004_8003;
    pub const GET_VIRTUAL_SIZE: u32 = 0x0004_0004;
    pub const TEST_VIRTUAL_SIZE: u32 = 0x0004_4004;
    pub const SET_VIRTUAL_SIZE: u32 = 0x0004_8004;
    pub const GET_DEPTH: u32 = 0x0004_0005;
    pub const TEST_DEPTH: u32 = 0x0004_4005;
    pub const SET_DEPTH: u32 = 0x0004_8005;
    pub const GET_PIXEL_ORDER: u32 = 0x0004_0006;
    pub const TEST_PIXEL_ORDER: u32 = 0x0004_4006;
    pub const SET_PIXEL_ORDER: u32 = 0x0004_8006;
    pub const GET_ALPHA_MODE: u32 = 0x0004_0007;
    pub const TEST_ALPHA_MODE: u32 = 0x0004_4007;
    pub const SET_ALPHA_MODE: u32 = 0x0004_8007;
    pub const GET_PITCH: u32 = 0x0004_0008;
    pub const GET_VIRTUAL_OFFSET: u32 = 0x0004_0009;
    pub const TEST_VIRTUAL_OFFSET: u32 = 0x0004_4009;
    pub const SET_VIRTUAL_OFFSET: u32 = 0x0004_8009;
    pub const GET_OVERSCAN: u32 = 0x0004_000a;
    pub const TEST_OVERSCAN: u32 = 0x0004_400a;
    pub const SET_OVERSCAN: u32 = 0x0004_800a;
    pub const GET_PALETTE: u32 = 0x0004_000b;
    pub const SET_PALETTE: u32 = 0x0004_800b;

    /// Refused: they hand over the system. Listed for the tests, which check
    /// that the allow-list stays closed to them.
    pub const ALLOCATE_MEMORY: u32 = 0x0003_000c;
    pub const LOCK_MEMORY: u32 = 0x0003_000d;
    pub const UNLOCK_MEMORY: u32 = 0x0003_000e;
    pub const RELEASE_MEMORY: u32 = 0x0003_000f;
    pub const EXECUTE_CODE: u32 = 0x0003_0010;
    pub const SET_CURSOR_INFO: u32 = 0x0000_8010;
    pub const SET_CURSOR_STATE: u32 = 0x0000_8011;
    /// Refused: typed functions or per-client policy cover them.
    pub const SET_POWER_STATE: u32 = 0x0002_8001;
    pub const SET_CLOCK_STATE: u32 = 0x0003_8001;
    pub const SET_CLOCK_RATE: u32 = 0x0003_8002;
}

/// Tags any client may send.
const ALLOWED: &[u32] = &[
    tag::GET_FIRMWARE_REVISION,
    tag::GET_BOARD_MODEL,
    tag::GET_BOARD_REVISION,
    tag::GET_BOARD_MAC_ADDRESS,
    tag::GET_BOARD_SERIAL,
    tag::GET_ARM_MEMORY,
    tag::GET_VC_MEMORY,
    tag::GET_CLOCKS,
    tag::GET_POWER_STATE,
    tag::GET_TIMING,
    tag::GET_CLOCK_STATE,
    tag::GET_CLOCK_RATE,
    tag::GET_VOLTAGE,
    tag::GET_MAX_CLOCK_RATE,
    tag::GET_TEMPERATURE,
    tag::GET_MIN_CLOCK_RATE,
    tag::GET_MAX_TEMPERATURE,
    tag::ALLOCATE_BUFFER,
    tag::RELEASE_BUFFER,
    tag::BLANK_SCREEN,
    tag::GET_PHYSICAL_SIZE,
    tag::TEST_PHYSICAL_SIZE,
    tag::SET_PHYSICAL_SIZE,
    tag::GET_VIRTUAL_SIZE,
    tag::TEST_VIRTUAL_SIZE,
    tag::SET_VIRTUAL_SIZE,
    tag::GET_DEPTH,
    tag::TEST_DEPTH,
    tag::SET_DEPTH,
    tag::GET_PIXEL_ORDER,
    tag::TEST_PIXEL_ORDER,
    tag::SET_PIXEL_ORDER,
    tag::GET_ALPHA_MODE,
    tag::TEST_ALPHA_MODE,
    tag::SET_ALPHA_MODE,
    tag::GET_PITCH,
    tag::GET_VIRTUAL_OFFSET,
    tag::TEST_VIRTUAL_OFFSET,
    tag::SET_VIRTUAL_OFFSET,
    tag::GET_OVERSCAN,
    tag::TEST_OVERSCAN,
    tag::SET_OVERSCAN,
    tag::GET_PALETTE,
    tag::SET_PALETTE,
];

/// Whether any client may send the tag `id`.
pub fn is_allowed(id: u32) -> bool {
    ALLOWED.contains(&id)
}

/// Bytes of the page a request is copied into.
pub const BUFFER_BYTES: usize = 4096;

/// Words in the smallest buffer: header and end tag.
const MIN_WORDS: usize = 3;

/// Bit 31 of a tag's request/response word: set once the firmware answered.
const TAG_ANSWERED: u32 = 0x8000_0000;

/// Why a buffer is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// Not a well-formed request.
    Malformed,
    /// A tag no client may send.
    NotAllowed(u32),
}

/// Check the request `words`: a header whose size is the buffer's size, an
/// ordinary request, tags that fit in the buffer and are all allowed, and an
/// end tag. Words after the end tag are padding.
pub fn check_request(words: &[u32]) -> Result<(), Rejected> {
    let &[size, code, ..] = words else {
        return Err(Rejected::Malformed);
    };
    let bytes = words.len() * 4;
    if words.len() < MIN_WORDS
        || bytes > BUFFER_BYTES
        || usize::try_from(size) != Ok(bytes)
        || code != 0
    {
        return Err(Rejected::Malformed);
    }
    let mut index = 2;
    loop {
        let &id = words.get(index).ok_or(Rejected::Malformed)?;
        if id == 0 {
            return Ok(());
        }
        let &value_bytes = words.get(index + 1).ok_or(Rejected::Malformed)?;
        let &request = words.get(index + 2).ok_or(Rejected::Malformed)?;
        if request & TAG_ANSWERED != 0 {
            return Err(Rejected::Malformed);
        }
        if !is_allowed(id) {
            return Err(Rejected::NotAllowed(id));
        }
        let value_words = usize::try_from(value_bytes)
            .map_err(|_overflow| Rejected::Malformed)?
            .div_ceil(4);
        index = index
            .checked_add(3)
            .and_then(|next| next.checked_add(value_words))
            .filter(|&next| next <= words.len())
            .ok_or(Rejected::Malformed)?;
    }
}
