//! MIDI channel state: program and bank, volume, expression, pan, pitch bend
//! and its range (registered parameter 0), and the sustain pedal.

use crate::tables::concave_centibels;

/// Registered parameter number of the pitch bend range.
const RPN_PITCH_BEND_RANGE: u16 = 0;
/// "No parameter selected".
const RPN_NULL: u16 = 0x3fff;

/// One MIDI channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Channel {
    /// Bank select (controller 0), applied at the next program change.
    pub bank_select: u8,
    /// Current program.
    pub program: u8,
    /// Index of the preset in the bank, if the program resolved to one.
    pub preset: Option<usize>,
    /// Channel volume (controller 7).
    pub volume: u8,
    /// Expression (controller 11).
    pub expression: u8,
    /// Pan (controller 10), 64 centre.
    pub pan: u8,
    /// Pitch bend, -8192 to 8191.
    pub bend: i16,
    /// Pitch bend range in cents.
    pub bend_range: i32,
    /// Sustain pedal down (controller 64 at 64 or more).
    pub sustain: bool,
    /// Selected registered parameter (controllers 101 and 100), `0x3fff`
    /// for none.
    pub registered_parameter: u16,
}

impl Channel {
    /// A channel as after power-on: program 0, volume 100, expression 127,
    /// centre pan, no bend, a range of 2 semitones.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bank_select: 0,
            program: 0,
            preset: None,
            volume: 100,
            expression: 127,
            pan: 64,
            bend: 0,
            bend_range: 200,
            sustain: false,
            registered_parameter: RPN_NULL,
        }
    }

    /// Reset all controllers (controller 121, General MIDI's RP-015):
    /// expression, bend, the pedal and the parameter selection; volume, pan
    /// and the program stay.
    pub const fn reset_controllers(&mut self) {
        self.expression = 127;
        self.bend = 0;
        self.sustain = false;
        self.registered_parameter = RPN_NULL;
    }

    /// The pitch bend in cents.
    #[must_use]
    pub fn bend_cents(&self) -> i32 {
        i32::from(self.bend) * self.bend_range / 8192
    }

    /// Attenuation of the channel's volume and expression in centibels.
    #[must_use]
    pub fn attenuation(&self) -> u32 {
        concave_centibels(self.volume) + concave_centibels(self.expression)
    }

    /// The channel's pan as a SoundFont pan offset (-500 to 492).
    #[must_use]
    pub fn pan_offset(&self) -> i32 {
        (i32::from(self.pan) - 64) * 500 / 64
    }

    /// Controllers 101/100 select a registered parameter.
    pub fn select_parameter(&mut self, controller: u8, value: u8) {
        let selected = self.registered_parameter;
        self.registered_parameter = if controller == 101 {
            (selected & 0x7f) | (u16::from(value) << 7)
        } else {
            (selected & !0x7f) | u16::from(value)
        };
    }

    /// Controllers 98/99 select a non-registered parameter: none is
    /// supported, so data entry is ignored until a registered one is chosen.
    pub const fn select_non_registered(&mut self) {
        self.registered_parameter = RPN_NULL;
    }

    /// Data entry coarse (controller 6) or fine (38) for the selected
    /// parameter.
    pub fn data_entry(&mut self, fine: bool, value: u8) {
        if self.registered_parameter != RPN_PITCH_BEND_RANGE {
            return;
        }
        let semitones = self.bend_range / 100;
        let cents = self.bend_range % 100;
        self.bend_range = if fine {
            semitones * 100 + i32::from(value.min(99))
        } else {
            i32::from(value) * 100 + cents
        };
    }
}

impl Default for Channel {
    fn default() -> Self {
        Self::new()
    }
}
