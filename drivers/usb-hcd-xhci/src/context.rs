//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Device contexts (§6.2): the Slot Context and Endpoint Context words
//! software writes into an Input Context for Address Device, Evaluate Context
//! and Configure Endpoint, and where a device sits on the bus: its root port,
//! route string and the transaction translator a low- or full-speed device is
//! reached through (§4.3.3, §8.9). Each context is 32 bytes, or 64 when
//! `HCCPARAMS1.CSZ` is set; only its first words are used here.

use {arbitrary_int::prelude::*, bitbybit::bitfield, vesper_usb_protocol::enumeration::Speed};

/// Slot Context word 0 (§6.2.2).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Slot0 {
    /// Route String: the hub port at each tier below the root port.
    #[bits(0..=19, rw)]
    route: u20,
    /// The device's speed, a Protocol Speed ID (1 full, 2 low, 3 high).
    #[bits(20..=23, rw)]
    speed: u4,
    /// Multi-TT: the hub runs one transaction translator per port.
    #[bit(25, rw)]
    multi_tt: bool,
    /// The device is a hub.
    #[bit(26, rw)]
    hub: bool,
    /// Context Entries: the index of the last valid Endpoint Context.
    #[bits(27..=31, rw)]
    context_entries: u5,
}

/// Slot Context word 1.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Slot1 {
    /// Root Hub Port Number: the port ID the device or its hubs hang on.
    #[bits(16..=23, rw)]
    root_port: u8,
    /// Number of Ports: a hub's downstream ports.
    #[bits(24..=31, rw)]
    ports: u8,
}

/// Slot Context word 2.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Slot2 {
    /// TT Hub Slot ID: the high-speed hub whose translator serves a low- or
    /// full-speed device.
    #[bits(0..=7, rw)]
    translator_slot: u8,
    /// TT Port Number: that hub's port.
    #[bits(8..=15, rw)]
    translator_port: u8,
    /// TT Think Time of a hub: 8, 16, 24 or 32 full-speed bit times as 0 to 3.
    #[bits(16..=17, rw)]
    think_time: u2,
}

/// Slot Context word 3, written by the controller.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Slot3 {
    /// USB Device Address: what Address Device gave the device.
    #[bits(0..=7, r)]
    address: u8,
    /// Slot State: 0 disabled or enabled, 1 default, 2 addressed, 3 configured.
    #[bits(27..=31, r)]
    state: u5,
}

/// Endpoint Context word 1 (§6.2.3).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Endpoint1 {
    /// Error Count: transaction errors retried before the endpoint halts.
    #[bits(1..=2, rw)]
    error_count: u2,
    /// Endpoint Type: 4 is a control endpoint.
    #[bits(3..=5, rw)]
    kind: u3,
    /// Max Packet Size.
    #[bits(16..=31, rw)]
    max_packet: u16,
}

/// The Endpoint Type of a bidirectional control endpoint.
const CONTROL_ENDPOINT: u8 = 4;
/// Retries of a failed transaction before the endpoint halts: the most
/// allowed, as Linux sets it.
const ERROR_RETRIES: u8 = 3;
/// The Average TRB Length of a control endpoint (§4.14.1.1).
const CONTROL_AVERAGE_TRB: u32 = 8;
/// The Dequeue Cycle State bit of an Endpoint Context's dequeue pointer.
const DEQUEUE_CYCLE_STATE: u64 = 1 << 0;
/// Tiers below the root port a route string can name (§8.9).
const MAX_TIERS: u8 = 5;

/// Input Control Context add flags (§6.2.5.1): the Slot Context and endpoint
/// 0's context.
pub const ADD_SLOT: u32 = 1 << 0;
pub const ADD_ENDPOINT0: u32 = 1 << 1;

/// A hub port whose transaction translator serves a low- or full-speed device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Translator {
    /// The slot of the high-speed hub with the translator.
    pub hub_slot: u8,
    pub port: u8,
}

/// Where a device sits on the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// The port ID of the root port above it.
    pub root_port: u8,
    /// The route string: the hub port at each tier, tier 1 in bits 3:0.
    pub route: u32,
    /// The tiers of hubs between the root port and the device.
    pub tiers: u8,
    pub translator: Option<Translator>,
}

impl Position {
    /// A device on root port `port_id`.
    #[must_use]
    pub const fn root(port_id: u8) -> Self {
        Self {
            root_port: port_id,
            route: 0,
            tiers: 0,
            translator: None,
        }
    }

    /// Where a `speed` device on port `port` of a hub sits, the hub being at
    /// `self`, in slot `hub_slot` and running at `hub_speed`. The route string
    /// takes the port at the hub's tier, 15 for ports past 15 (§8.9). The
    /// translator is the nearest high-speed hub's port (USB 2.0 §11.14).
    /// `None` past the fifth tier.
    #[must_use]
    pub const fn behind(
        &self,
        hub_slot: u8,
        hub_speed: Speed,
        port: u8,
        speed: Speed,
    ) -> Option<Self> {
        if self.tiers >= MAX_TIERS || port == 0 {
            return None;
        }
        let nibble = if port > 15 { 15 } else { port as u32 };
        let translator = match (hub_speed, speed) {
            (Speed::High, Speed::High) => None,
            (Speed::High, Speed::Low | Speed::Full) => Some(Translator { hub_slot, port }),
            (Speed::Low | Speed::Full, _) => self.translator,
        };
        Some(Self {
            root_port: self.root_port,
            route: self.route | nibble << (4 * self.tiers as u32),
            tiers: self.tiers + 1,
            translator,
        })
    }
}

/// The Protocol Speed ID of `speed` for the default USB 2 speeds (§7.2.2.1.1).
#[must_use]
pub const fn speed_id(speed: Speed) -> u8 {
    match speed {
        Speed::Full => 1,
        Speed::Low => 2,
        Speed::High => 3,
    }
}

/// What a hub's Slot Context records (`usb_hcd_v1.mark_hub`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hub {
    pub ports: u8,
    /// Think time in full-speed bit times: 8, 16, 24 or 32.
    pub think_time: u8,
    pub multi_tt: bool,
}

/// The first four words of the Slot Context of a `speed` device at
/// `position`, with endpoint 0 its one valid endpoint, and `hub` when it is
/// one.
#[must_use]
pub fn slot_context(position: Position, speed: Speed, hub: Option<Hub>) -> [u32; 4] {
    let word0 = Slot0::ZERO
        .with_route(u20::new(position.route & 0xf_ffff))
        .with_speed(u4::new(speed_id(speed)))
        .with_context_entries(u5::new(1))
        .with_hub(hub.is_some())
        .with_multi_tt(hub.is_some_and(|hub| hub.multi_tt));
    let word1 = Slot1::ZERO
        .with_root_port(position.root_port)
        .with_ports(hub.map_or(0, |hub| hub.ports));
    let think_time = hub.map_or(0, |hub| (hub.think_time / 8).clamp(1, 4) - 1);
    let word2 = Slot2::ZERO
        .with_translator_slot(
            position
                .translator
                .map_or(0, |translator| translator.hub_slot),
        )
        .with_translator_port(position.translator.map_or(0, |translator| translator.port))
        .with_think_time(u2::new(think_time));
    [word0.raw_value(), word1.raw_value(), word2.raw_value(), 0]
}

/// The first five words of endpoint 0's Endpoint Context: a control endpoint
/// of `max_packet` bytes whose transfer ring starts at bus address `ring` with
/// cycle state 1.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the dequeue pointer's low word is the truncation"
)]
pub fn endpoint0_context(max_packet: u16, ring: u64) -> [u32; 5] {
    let word1 = Endpoint1::ZERO
        .with_error_count(u2::new(ERROR_RETRIES))
        .with_kind(u3::new(CONTROL_ENDPOINT))
        .with_max_packet(max_packet);
    let dequeue = ring | DEQUEUE_CYCLE_STATE;
    [
        0,
        word1.raw_value(),
        dequeue as u32,
        (dequeue >> 32) as u32,
        CONTROL_AVERAGE_TRB,
    ]
}
