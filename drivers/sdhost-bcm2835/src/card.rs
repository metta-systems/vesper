//! The SD card protocol: bringing a card from power-up to the transfer state
//! and reading its size, over any controller that sends commands ([`Bus`]).
//!
//! Section numbers are those of the SD Physical Layer Simplified
//! Specification, version 9.10.

/// What a command's response is (§4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    /// No response
    None,
    /// 48 bits: the card status (R1), the OCR (R3), the published RCA (R6) or
    /// the interface condition (R7)
    Short,
    /// 48 bits without a valid CRC: the OCR (R3)
    ShortNoCrc,
    /// 136 bits: the CID or CSD register (R2)
    Long,
}

/// The data stage of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Data {
    None,
    /// One block from the card
    Read,
    /// One block to the card
    Write,
}

/// A command the card protocol sends (§4.7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub index: u8,
    pub response: Response,
    pub data: Data,
}

impl Command {
    const fn new(index: u8, response: Response) -> Self {
        Self {
            index,
            response,
            data: Data::None,
        }
    }
}

/// CMD0: every card to the idle state.
pub const GO_IDLE_STATE: Command = Command::new(0, Response::None);
/// CMD2: the card's CID.
pub const ALL_SEND_CID: Command = Command::new(2, Response::Long);
/// CMD3: the card publishes its relative address.
pub const SEND_RELATIVE_ADDR: Command = Command::new(3, Response::Short);
/// ACMD6: the data bus width; argument 2 for four bits.
pub const SET_BUS_WIDTH: Command = Command::new(6, Response::Short);
/// CMD7: select the card at the address in bits 31-16 (R1b; the busy is
/// waited out with [`SEND_STATUS`]).
pub const SELECT_CARD: Command = Command::new(7, Response::Short);
/// CMD8: the supply voltage and a check pattern, which a version 2 card echoes.
pub const SEND_IF_COND: Command = Command::new(8, Response::Short);
/// CMD9: the card's CSD.
pub const SEND_CSD: Command = Command::new(9, Response::Long);
/// CMD13: the card status.
pub const SEND_STATUS: Command = Command::new(13, Response::Short);
/// CMD16: the block length of a standard capacity card.
pub const SET_BLOCKLEN: Command = Command::new(16, Response::Short);
/// CMD17: read one block.
pub const READ_SINGLE_BLOCK: Command = Command {
    data: Data::Read,
    ..Command::new(17, Response::Short)
};
/// CMD24: write one block.
pub const WRITE_BLOCK: Command = Command {
    data: Data::Write,
    ..Command::new(24, Response::Short)
};
/// ACMD41: the host's capacity support; the card answers its OCR.
pub const SD_SEND_OP_COND: Command = Command::new(41, Response::ShortNoCrc);
/// CMD55: the next command is an application command.
pub const APP_CMD: Command = Command::new(55, Response::Short);

/// CMD8 argument: 2.7-3.6 V and the check pattern 0xAA (§4.3.13).
pub const IF_COND: u32 = 0x1aa;
/// ACMD41 argument bit: the host supports high capacity cards.
pub const HCS: u32 = 1 << 30;
/// ACMD41 argument: the 2.7-3.6 V window of the OCR (§5.1).
pub const VOLTAGE_WINDOW: u32 = 0x00ff_8000;
/// OCR bit: the card finished its power-up.
pub const OCR_POWERED_UP: u32 = 1 << 31;
/// OCR bit: a high or extended capacity card, addressed in blocks.
pub const OCR_CCS: u32 = 1 << 30;

/// Card status error bits of an R1 response (§4.10.1): `OUT_OF_RANGE` through
/// `ERROR`, but `CARD_IS_LOCKED`, which is a state.
pub const STATUS_ERRORS: u32 = 0xfdf8_0000;
/// Card status bit: the card takes data.
pub const READY_FOR_DATA: u32 = 1 << 8;
/// The transfer state, in bits 12-9 of the card status.
pub const STATE_TRANSFER: u32 = 4;

/// Bytes of a block.
pub const BLOCK_BYTES: u32 = 512;

/// The identification clock (§4.2.1: at most 400 kHz).
pub const IDENTIFICATION_HZ: u32 = 400_000;
/// The default speed clock (§4.3.10: at most 25 MHz).
pub const TRANSFER_HZ: u32 = 25_000_000;

/// How long a card may take to finish its power-up (§4.2.3: one second).
pub const POWER_UP_MS: u32 = 1000;
/// The pause between two power-up polls.
pub const POLL_MS: u32 = 10;

/// Why a command failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusError {
    /// No response came
    Timeout,
    /// The response or the data was damaged
    Crc,
    /// The controller gave up otherwise
    Failed,
}

/// A controller the card protocol sends commands through.
pub trait Bus {
    /// Send `command` with `argument` and return its response: a short one in
    /// word 0, a long one with its bits 31-0 in word 0. A command with a data
    /// stage leaves the block to move with the controller's own calls.
    ///
    /// # Errors
    ///
    /// What made the command fail.
    fn command(&mut self, command: Command, argument: u32) -> Result<[u32; 4], BusError>;

    /// Run the card clock at `hz` or the nearest rate below it.
    fn set_clock(&mut self, hz: u32);

    /// Use the four data lines.
    fn set_wide_bus(&mut self);

    /// Wait `milliseconds`.
    fn delay_ms(&mut self, milliseconds: u32);
}

/// A step of a card's bring-up: what `block_v1`'s `device_failed` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    InterfaceCondition = 1,
    PowerUp = 2,
    Identification = 3,
    RelativeAddress = 4,
    CardSpecific = 5,
    Selection = 6,
    BusWidth = 7,
    BlockLength = 8,
}

/// Why a card could not be brought up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardError {
    /// No card answered
    NoCard,
    /// The card failed at `step`, its command failing with `error`
    Failed { step: Step, error: BusError },
}

/// The error of a command that failed at `step`.
const fn failed(step: Step) -> impl FnOnce(BusError) -> CardError {
    move |error| CardError::Failed { step, error }
}

/// A card in the transfer state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    /// The relative address the card published
    pub rca: u16,
    /// Addressed in blocks; a standard capacity card is addressed in bytes
    pub high_capacity: bool,
    /// Blocks of [`BLOCK_BYTES`] the card holds
    pub blocks: u64,
}

/// Bits `high..=low` of a long response (bit 0 is word 0's bit 0).
fn long_bits(words: &[u32; 4], high: u32, low: u32) -> u64 {
    (low..=high).rev().fold(0, |value, bit| {
        let word = words[(bit / 32) as usize];
        (value << 1) | u64::from((word >> (bit % 32)) & 1)
    })
}

/// The blocks a card holds, from its CSD (§5.3); `None` for an unknown CSD
/// structure.
#[must_use]
pub fn csd_blocks(csd: &[u32; 4]) -> Option<u64> {
    match long_bits(csd, 127, 126) {
        // Version 1.0: C_SIZE, C_SIZE_MULT and READ_BL_LEN (§5.3.2).
        0 => {
            let size = long_bits(csd, 73, 62) + 1;
            let multiplier = long_bits(csd, 49, 47) + 2;
            let block_length = long_bits(csd, 83, 80);
            Some((size << multiplier << block_length) / u64::from(BLOCK_BYTES))
        }
        // Version 2.0: C_SIZE in 512 KiB units (§5.3.3); version 3.0 widens it
        // to bits 75-48 (§5.3.4), whose upper bits are zero in version 2.0.
        1 | 2 => Some((long_bits(csd, 75, 48) + 1) * 1024),
        _ => None,
    }
}

impl Card {
    /// Bring the card on `bus` from power-up to the transfer state (§4.2,
    /// figure 4-2) and learn its size.
    ///
    /// # Errors
    ///
    /// [`CardError::NoCard`] when no card answers the first application
    /// command, [`CardError::Failed`] with the step a card failed at.
    pub fn bring_up(bus: &mut impl Bus) -> Result<Self, CardError> {
        bus.set_clock(IDENTIFICATION_HZ);
        // No response to wait for: only the controller can fail it.
        if bus.command(GO_IDLE_STATE, 0).is_err() {
            return Err(CardError::NoCard);
        }

        // A version 2 card echoes the pattern; an older one stays silent.
        let version2 = match bus.command(SEND_IF_COND, IF_COND) {
            Ok(response) if response[0] & 0xfff == IF_COND => true,
            Err(BusError::Timeout) => false,
            Ok(_) => return Err(failed(Step::InterfaceCondition)(BusError::Failed)),
            Err(error) => return Err(failed(Step::InterfaceCondition)(error)),
        };

        let argument = VOLTAGE_WINDOW | if version2 { HCS } else { 0 };
        let mut ocr = 0;
        for poll in 0..POWER_UP_MS / POLL_MS {
            match bus.command(APP_CMD, 0) {
                Ok(_) => {}
                Err(BusError::Timeout) if poll == 0 => return Err(CardError::NoCard),
                Err(error) => return Err(failed(Step::PowerUp)(error)),
            }
            ocr = bus
                .command(SD_SEND_OP_COND, argument)
                .map_err(failed(Step::PowerUp))?[0];
            if ocr & OCR_POWERED_UP != 0 {
                break;
            }
            bus.delay_ms(POLL_MS);
        }
        if ocr & OCR_POWERED_UP == 0 {
            return Err(failed(Step::PowerUp)(BusError::Timeout));
        }

        bus.command(ALL_SEND_CID, 0)
            .map_err(failed(Step::Identification))?;
        let published = bus
            .command(SEND_RELATIVE_ADDR, 0)
            .map_err(failed(Step::RelativeAddress))?;
        let rca = (published[0] >> 16) as u16;
        let address = u32::from(rca) << 16;
        let csd = bus
            .command(SEND_CSD, address)
            .map_err(failed(Step::CardSpecific))?;
        let blocks =
            csd_blocks(&csd).ok_or_else(|| failed(Step::CardSpecific)(BusError::Failed))?;
        let card = Self {
            rca,
            high_capacity: version2 && ocr & OCR_CCS != 0,
            blocks,
        };

        bus.command(SELECT_CARD, address)
            .map_err(failed(Step::Selection))?;
        card.wait_ready(bus).map_err(failed(Step::Selection))?;
        bus.command(APP_CMD, address)
            .and_then(|_status| bus.command(SET_BUS_WIDTH, 2))
            .map_err(failed(Step::BusWidth))?;
        bus.set_wide_bus();
        if !card.high_capacity {
            bus.command(SET_BLOCKLEN, BLOCK_BYTES)
                .map_err(failed(Step::BlockLength))?;
        }
        bus.set_clock(TRANSFER_HZ);
        Ok(card)
    }

    /// The argument of a read or write of `block`: the block itself for a high
    /// capacity card, its byte address for a standard one; `None` past the
    /// address range.
    #[must_use]
    pub fn argument(&self, block: u64) -> Option<u32> {
        let address = if self.high_capacity {
            block
        } else {
            block.checked_mul(u64::from(BLOCK_BYTES))?
        };
        u32::try_from(address).ok()
    }

    /// Wait until the card is in the transfer state and takes data, as after a
    /// selection or a write (§4.10.1), polling once a millisecond.
    ///
    /// # Errors
    ///
    /// A failed status command, a status with an error bit, or the card staying
    /// busy past [`POWER_UP_MS`] (`BusError::Timeout`).
    pub fn wait_ready(&self, bus: &mut impl Bus) -> Result<(), BusError> {
        for _poll in 0..POWER_UP_MS {
            let status = bus.command(SEND_STATUS, u32::from(self.rca) << 16)?[0];
            if status & STATUS_ERRORS != 0 {
                return Err(BusError::Failed);
            }
            if status & READY_FOR_DATA != 0 && (status >> 9) & 0xf == STATE_TRANSFER {
                return Ok(());
            }
            bus.delay_ms(1);
        }
        Err(BusError::Timeout)
    }
}
