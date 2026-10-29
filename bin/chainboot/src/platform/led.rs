/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The activity LED. On most models it is on a GPIO pin and is switched
//! directly. The Raspberry Pi 3B has it on a GPIO expander only the firmware can
//! reach, and there it is switched by a mailbox message.
//!
//! The firmware answers a request a little later. While a kernel arrives over the
//! UART the core cannot spare the time to wait for that: the receive FIFO holds
//! sixteen bytes, 1.4 ms at 115200 baud and 80 us at 2 Mbaud. So a request can be
//! started and left unanswered, and the next one reaps the answer first; a flip
//! asked for before then is skipped.

use {
    super::{
        GPIO_BASE, gpio,
        mailbox::{self, Message, tag},
    },
    core::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::Duration,
    },
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libtime::time::time_manager,
};

/// The firmware's number of the LED's pin on the Raspberry Pi 3B: the GPIO
/// expander, which the firmware numbers from 128, pin 2 is 130.
const EXPANDER_PIN: u32 = 130;

/// The GPIO that drives the LED directly, or zero when the firmware has to.
static DIRECT_PIN: AtomicUsize = AtomicUsize::new(0);

/// The GPIO that drives the LED on the model with this revision code, zero for
/// the expander. A new-style code (bit 23) has the model in bits 11:4: 0x0d and
/// 0x0e are the 3B+ and 3A+ (GPIO 29); 0x11, 0x13 and 0x14 are the 4B, 400 and
/// CM4 (GPIO 42). Any other model, and a board that does not say, gets what the
/// board the program is built for has.
fn direct_pin(revision: Option<u32>) -> usize {
    const NEW_STYLE: u32 = 1 << 23;
    let built_for = if cfg!(board_rpi4) { 42 } else { 0 };
    let Some(revision) = revision.filter(|&code| code & NEW_STYLE != 0) else {
        return built_for;
    };
    match (revision >> 4) & 0xff {
        0x0d | 0x0e => 29,
        0x11 | 0x13 | 0x14 => 42,
        _ => built_for,
    }
}

/// Choose how the LED is driven for the board with this revision code, and set
/// up the pin when it is direct.
pub fn init(revision: Option<u32>) {
    let pin = direct_pin(revision);
    if pin != 0 {
        // SAFETY: the GPIO registers are at `GPIO_BASE`, the LED's pin is the
        // program's, and chainboot runs on one core.
        unsafe {
            gpio::make_output(GPIO_BASE, pin);
        }
    }
    DIRECT_PIN.store(pin, Ordering::Relaxed);
}

/// The request in flight. The firmware owns it from the send to the answer.
static REQUEST: IRQSafeNullLock<Message> = IRQSafeNullLock::new(Message::new(0, &[]));

/// Whether the firmware has not answered [`REQUEST`] yet.
static OUTSTANDING: AtomicBool = AtomicBool::new(false);

/// Collect the answer of the request in flight, if it came. Returns whether the
/// firmware is done with it.
fn reap() -> bool {
    if OUTSTANDING.load(Ordering::Relaxed) && mailbox::answered() {
        OUTSTANDING.store(false, Ordering::Relaxed);
    }
    !OUTSTANDING.load(Ordering::Relaxed)
}

/// Start switching the LED, and return without waiting for the firmware. Does
/// nothing when the request before is not answered yet. Returns whether the
/// request was sent.
fn start(on: bool) -> bool {
    let pin = DIRECT_PIN.load(Ordering::Relaxed);
    if pin != 0 {
        // SAFETY: as in `init`.
        unsafe {
            gpio::write_pin(GPIO_BASE, pin, on);
        }
        return true;
    }
    if !reap() {
        return false;
    }
    REQUEST.lock(|request| {
        *request = Message::new(tag::SET_GPIO_STATE, &[EXPANDER_PIN, u32::from(on)]);
        let sent = mailbox::send(request).is_ok();
        OUTSTANDING.store(sent, Ordering::Relaxed);
        sent
    })
}

/// Switch the LED and wait until the firmware has done it.
///
/// # Errors
///
/// If the firmware does not answer in time.
pub fn set(on: bool) -> Result<(), &'static str> {
    if !mailbox::wait_until(reap) {
        return Err("the LED request before is not answered");
    }
    if !start(on) {
        return Err("the LED request was not sent");
    }
    if mailbox::wait_until(reap) {
        Ok(())
    } else {
        Err("the LED request is not answered")
    }
}

/// Flash the LED `count` times, then leave it off and wait a moment, so that the
/// flashes of one stage can be told from the next. For bring-up, where the
/// console is not there yet: the LED is all there is to say what happened.
pub fn flash(count: u32) {
    const HALF: Duration = Duration::from_millis(150);
    const PAUSE: Duration = Duration::from_millis(700);

    for _ in 0..count {
        let _on = set(true);
        time_manager().spin_for(HALF);
        let _off = set(false);
        time_manager().spin_for(HALF);
    }
    time_manager().spin_for(PAUSE);
}

/// An LED that blinks by itself, never waiting for the firmware. It starts on.
pub struct Blinker {
    on: bool,
    /// When it last changed.
    changed: Duration,
}

impl Blinker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            on: start(true),
            changed: time_manager().uptime(),
        }
    }

    /// Flip the LED. A flip the firmware cannot take yet is skipped.
    pub fn toggle(&mut self) {
        if start(!self.on) {
            self.on = !self.on;
        }
        self.changed = time_manager().uptime();
    }

    /// Flip the LED when `period` has passed since it last changed.
    pub fn tick(&mut self, period: Duration) {
        let elapsed = time_manager()
            .uptime()
            .checked_sub(self.changed)
            .unwrap_or(Duration::ZERO);
        if elapsed >= period {
            self.toggle();
        }
    }
}

impl Default for Blinker {
    fn default() -> Self {
        Self::new()
    }
}
