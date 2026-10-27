//! The driver's one Thread (see the crate documentation in `main.rs`).

use {
    audio_pwm_bcm2835::{JACK_PINS, PWM_CLOCK_HZ},
    clock_client::clock_v1,
    driver_protocol::report_key,
    gpio_client::gpio_v1,
    libdevice_driver::{DriverInit, driver_compatible, driver_init},
    libobject::{NotificationKey, RawKey},
    libserial_trace::{Trace, trace},
};

libuser::entry!(main);

driver_compatible!("brcm,bcm2835-pwm");

/// The PWM clock's number in the clock manager's device tree binding, the
/// second cell of the node's `clocks = <&clocks 30>`: Kickstart links the node
/// to the clock manager but does not pass the cell.
const PWM_CLOCK: u32 = 30;

/// Route the pins of the node's pin group.
fn route_pins(trace: &mut Trace, init: &DriverInit) {
    let gpio = gpio_v1::Client::new(gpio_v1::Bindings::from_imports(init.imports()));
    if init.pins().is_empty() {
        trace!(
            trace,
            "audio: the node has no pin group; GPIO {} and {} stay as they are",
            JACK_PINS[0],
            JACK_PINS[1]
        );
    }
    for setting in init.pins() {
        let function = gpio_v1::PinFunction::from_select_code(setting.function);
        let routed =
            function.is_some_and(|function| gpio.set_function(setting.pin, function).is_ok());
        trace!(
            trace,
            "audio: GPIO {} to {:?}: {}",
            setting.pin,
            function,
            if routed { "routed" } else { "FAILED" }
        );
    }
}

/// Set the PWM clock to [`PWM_CLOCK_HZ`], start it and read its rate back.
fn start_clock(trace: &mut Trace, clock: &clock_v1::Client) -> u32 {
    let actual = clock
        .set_rate(PWM_CLOCK, PWM_CLOCK_HZ)
        .unwrap_or_else(|error| panic!("audio: setting the PWM clock failed: {error:?}"));
    clock
        .enable(PWM_CLOCK)
        .unwrap_or_else(|error| panic!("audio: starting the PWM clock failed: {error:?}"));
    let rate = clock
        .rate(PWM_CLOCK)
        .unwrap_or_else(|error| panic!("audio: reading the PWM clock failed: {error:?}"));
    trace!(
        trace,
        "audio: PWM clock set to {actual} Hz, reads back {rate} Hz"
    );
    rate
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    let mut trace = Trace::new(init);
    trace!(trace, "audio: starting");
    route_pins(&mut trace, init);
    let clock = clock_v1::Client::new(clock_v1::Bindings::from_imports(init.imports()));
    let rate = start_clock(&mut trace, &clock);
    assert_eq!(rate, PWM_CLOCK_HZ, "audio: the PWM clock's rate");

    #[cfg(feature = "qemu")]
    emulated::check_refusals(&mut trace, &clock);
    #[cfg(not(feature = "qemu"))]
    tone::play_test(&mut trace, init);

    // The report key exists only in the test images; elsewhere nobody waits.
    if NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .is_ok()
    {
        trace!(trace, "audio: reported");
    }
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// What QEMU can check: the clock manager's model, without the PWM block.
#[cfg(feature = "qemu")]
mod emulated {
    use {
        super::{PWM_CLOCK, clock_v1},
        audio_pwm_bcm2835::PWM_CLOCK_HZ,
        libserial_trace::{Trace, trace},
    };

    /// A clock the component does not serve and a rate no integer divisor
    /// reaches are refused, and leave the PWM clock as it was.
    pub fn check_refusals(trace: &mut Trace, clock: &clock_v1::Client) {
        let pcm_clock = PWM_CLOCK + 1;
        assert_eq!(
            clock.set_rate(pcm_clock, PWM_CLOCK_HZ),
            Err(clock_v1::Error::Exception(
                clock_v1::Exception::UnknownClock { clock: pcm_clock }
            ))
        );
        let uart_hz = 48_000_000;
        assert_eq!(
            clock.set_rate(PWM_CLOCK, uart_hz),
            Err(clock_v1::Error::Exception(
                clock_v1::Exception::UnreachableRate { hz: uart_hz }
            ))
        );
        assert_eq!(clock.rate(PWM_CLOCK), Ok(PWM_CLOCK_HZ));
        trace!(
            trace,
            "audio: clock_v1 refused clock {pcm_clock} and {uart_hz} Hz; the PWM clock still runs at {PWM_CLOCK_HZ} Hz"
        );
        trace!(trace, "audio: QEMU has no PWM block, no tone is played");
    }
}

/// The polled test tone, on the board.
#[cfg(not(feature = "qemu"))]
mod tone {
    use {
        audio_pwm_bcm2835::{
            Control, DmaConfig, FIFO_WORDS, MmioRegisterBlock, PWM_CLOCK_HZ, RANGE, RegisterBlock,
            Side, Status, Tone, clear_errors, clearing, fifo_stereo, frame, has_errors,
            sample_rate_millihertz, word,
        },
        libdevice_driver::{DriverInit, device_address, map_device_pages},
        libserial_trace::{Trace, trace},
    };

    /// The test tone's pitch.
    const TONE_HZ: u32 = 440;
    /// Each sample shifted right by this: a quarter of full scale, gentle on
    /// headphones (full scale is about 1 V peak to peak at the jack).
    const ATTENUATION: u32 = 2;
    /// Milliseconds of each side's tone.
    const TONE_MS: u64 = 3_000;
    /// Milliseconds of silence after each side.
    const PAUSE_MS: u64 = 700;
    /// Milliseconds of the ramp between no output and silence, at the start
    /// and the end: starting the channels steps the output from low to the mid
    /// level, which a ramp turns from a click into a slope.
    const RAMP_MS: u64 = 20;
    /// Times the three sides are played.
    const ROUNDS: u32 = 3;

    /// Frames of `milliseconds` at `rate_millihertz`.
    fn frames(milliseconds: u64, rate_millihertz: u64) -> u64 {
        milliseconds * rate_millihertz / 1_000_000
    }

    /// The PWM block's registers and what went wrong while feeding it.
    struct Pwm {
        registers: MmioRegisterBlock<'static>,
        /// Error and gap flags seen since the last report
        seen: Status,
        /// Words since the last report that found the FIFO empty although the
        /// one before them had been written in time
        underruns: u64,
    }

    impl Pwm {
        /// Write one FIFO word once the FIFO has room; `count` says whether an
        /// empty FIFO is an underrun.
        fn push(&mut self, value: u32, count: bool) {
            loop {
                let status = self.registers.read_status();
                if !status.fifo_full() {
                    if count && status.fifo_empty() {
                        self.underruns += 1;
                    }
                    break;
                }
                core::hint::spin_loop();
            }
            self.registers.write_fifo(value);
        }

        /// Write `count` frames from `next`. The first FIFO's worth refills it
        /// after a pause in feeding (a trace line), so it does not count as an
        /// underrun.
        fn play(&mut self, count: u64, mut next: impl FnMut() -> [u32; 2]) {
            let refill = (FIFO_WORDS / 2) as u64;
            for index in 0..count {
                let [right, left] = next();
                self.push(right, index >= refill);
                self.push(left, index >= refill);
            }
            let status = self.registers.read_status();
            if has_errors(status) {
                self.seen = Status::new_with_raw_value(self.seen.raw_value() | status.raw_value());
                self.registers.write_status(clear_errors());
            }
        }

        /// Trace and forget what went wrong since the last report.
        fn report(&mut self, trace: &mut Trace, what: &str) {
            let seen = self.seen;
            if has_errors(seen) || self.underruns != 0 {
                trace!(
                    trace,
                    "audio: {what}: {} underruns, write error {}, read error {}, gap right {} left {}, bus error {}",
                    self.underruns,
                    seen.write_error(),
                    seen.read_error(),
                    seen.gap1(),
                    seen.gap2(),
                    seen.bus_error()
                );
            } else {
                trace!(trace, "audio: {what}: no underruns, no errors");
            }
            self.seen = Status::ZERO;
            self.underruns = 0;
        }
    }

    /// Map the block, set it up for both channels on the FIFO, and play the
    /// test tone.
    pub fn play_test(trace: &mut Trace, init: &DriverInit) {
        map_device_pages(init)
            .unwrap_or_else(|error| panic!("audio: mapping registers failed: {:?}", error.code()));
        let address =
            device_address(init, init.regions()[0].start).expect("audio: registers mapped");
        // SAFETY: mapped for good just above; this Thread is the only one
        // touching the block.
        let registers = unsafe { RegisterBlock::new_mmio(address as *mut RegisterBlock) };
        let mut pwm = Pwm {
            registers,
            seen: Status::ZERO,
            underruns: 0,
        };

        // The test feeds the FIFO by hand and stops feeding while it traces a
        // line, so both channels repeat the last word when the FIFO runs empty
        // (`RPTL`): a pause holds the last level instead of dropping the output
        // to low, which would click. An underrun inside a segment is counted
        // from `EMPT1` instead of the gap flags, which `RPTL` suppresses.
        let control = fifo_stereo()
            .with_repeat_last1(true)
            .with_repeat_last2(true);
        // Both channels stopped, DMA off, the same range on both (§9.6, FIF1),
        // the FIFO cleared and filled with low words (it is writable while the
        // channels are off, §9.6 CTL), then both channels started on it.
        pwm.registers.write_control(Control::ZERO);
        pwm.registers.write_dma_config(DmaConfig::ZERO);
        pwm.registers.write_range1(RANGE);
        pwm.registers.write_range2(RANGE);
        pwm.registers.write_control(clearing());
        pwm.registers
            .write_control(control.with_enable1(false).with_enable2(false));
        for _ in 0..FIFO_WORDS {
            pwm.registers.write_fifo(0);
        }
        pwm.registers.write_status(clear_errors());
        pwm.registers.write_control(control);
        let rate_millihertz = sample_rate_millihertz(PWM_CLOCK_HZ, RANGE);
        trace!(
            trace,
            "audio: PWM on, range {RANGE}, {}.{:03} Hz, {:?}",
            rate_millihertz / 1000,
            rate_millihertz % 1000,
            pwm.registers.read_control()
        );

        let silence = word(0, RANGE);
        let ramp = frames(RAMP_MS, rate_millihertz).max(1);
        let mut step = 0;
        pwm.play(ramp, || {
            step += 1;
            let level = u32::try_from(u64::from(silence) * step / ramp).unwrap_or(silence);
            [level, level]
        });
        pwm.report(trace, "ramp up");

        for round in 1..=ROUNDS {
            for (side, name) in [
                (Side::Left, "left only (PWM channel 2, GPIO41)"),
                (Side::Right, "right only (PWM channel 1, GPIO40)"),
                (Side::Both, "both sides"),
            ] {
                trace!(
                    trace,
                    "audio: round {round}/{ROUNDS}: {TONE_HZ} Hz on the {name} for {TONE_MS} ms"
                );
                let mut tone = Tone::new(TONE_HZ, rate_millihertz, ATTENUATION);
                pwm.play(frames(TONE_MS, rate_millihertz), || {
                    frame(side, tone.next_sample(), RANGE)
                });
                pwm.play(frames(PAUSE_MS, rate_millihertz), || [silence, silence]);
                pwm.report(trace, name);
            }
        }

        let mut step = ramp;
        pwm.play(ramp, || {
            step -= 1;
            let level = u32::try_from(u64::from(silence) * step / ramp).unwrap_or(0);
            [level, level]
        });
        pwm.report(trace, "ramp down");
        // Let the FIFO drain before the channels stop.
        while !pwm.registers.read_status().fifo_empty() {
            core::hint::spin_loop();
        }
        pwm.registers.write_control(Control::ZERO);
        pwm.registers.write_status(clear_errors());
        trace!(trace, "audio: test tone done");
    }
}
