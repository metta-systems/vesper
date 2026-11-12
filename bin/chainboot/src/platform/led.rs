/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The activity LED, as the device tree's `/leds` node describes it. On most
//! models it is on a GPIO pin and is switched directly. The Raspberry Pi 3B has
//! it on a GPIO expander only the firmware can reach, and there it is switched
//! by a mailbox message. A board whose LED is on neither (the Raspberry Pi 5's
//! is on a GPIO block chainboot does not drive) has no LED for chainboot.
//!
//! The firmware answers a request a little later. While a kernel arrives over the
//! UART the core cannot spare the time to wait for that: the receive FIFO holds
//! sixteen bytes, 1.4 ms at 115200 baud and 80 us at 2 Mbaud. So a request can be
//! started and left unanswered, and the next one reaps the answer first; a flip
//! asked for before then is skipped.

use {
    super::{
        gpio,
        mailbox::{self, Message, tag},
    },
    core::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    },
    libdevicetree::ActivityLed,
    liblocking::{IRQSafeNullLock, interface::Mutex},
    libtime::time::time_manager,
};

/// How the LED is switched, once [`init`] has been told.
static LED: IRQSafeNullLock<Option<ActivityLed>> = IRQSafeNullLock::new(None);

fn led() -> Option<ActivityLed> {
    LED.lock(|led| *led)
}

/// Drive the LED `led` describes, and set up its pin when it is on a GPIO block.
pub fn init(led: Option<ActivityLed>) {
    if let Some(ActivityLed::Gpio { gpio, pin, .. }) = led {
        // SAFETY: the device tree gives the GPIO registers, the LED's pin is
        // the program's, and chainboot runs on one core.
        unsafe {
            gpio::make_output(gpio, pin as usize);
        }
    }
    LED.lock(|attached| *attached = led);
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
/// nothing when the request before is not answered yet, or there is no LED.
/// Returns whether the request was sent.
fn start(on: bool) -> bool {
    match led() {
        None => false,
        Some(ActivityLed::Gpio {
            gpio,
            pin,
            active_low,
        }) => {
            // SAFETY: as in `init`.
            unsafe {
                gpio::write_pin(gpio, pin as usize, on != active_low);
            }
            true
        }
        Some(ActivityLed::Firmware { pin, active_low }) => {
            if !reap() {
                return false;
            }
            REQUEST.lock(|request| {
                *request = Message::new(tag::SET_GPIO_STATE, &[pin, u32::from(on != active_low)]);
                let sent = mailbox::send(request).is_ok();
                OUTSTANDING.store(sent, Ordering::Relaxed);
                sent
            })
        }
    }
}

/// Switch the LED and wait until the firmware has done it.
///
/// # Errors
///
/// If there is no LED, or the firmware does not answer in time.
pub fn set(on: bool) -> Result<(), &'static str> {
    if led().is_none() {
        return Err("there is no LED chainboot can switch");
    }
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

    if led().is_none() {
        return;
    }
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
