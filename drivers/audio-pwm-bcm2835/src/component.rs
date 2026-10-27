//! The driver's one Thread (see the crate documentation in `main.rs`).

use {
    audio_pwm_bcm2835::{JACK_PINS, PWM_CLOCK_HZ},
    clock_client::clock_v1,
    dma_client::dma_v1,
    driver_protocol::report_key,
    gpio_client::gpio_v1,
    libdevice_driver::{
        DEVICE_WINDOW_VA, DriverInit, MAX_DEVICE_PAGES, NO_BUS_ADDRESS, driver_compatible,
        driver_init,
    },
    libobject::{
        EventCountKey, FrameAttribute, FrameKey, KeyTableKey, NotificationKey, ObjectType, RawKey,
        Rights, UntypedKey,
    },
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

/// Where the driver maps the memory the DMA controller reaches, in the
/// device window: above the room of the device pages and the serial
/// channel's control page and data area, which follow them
/// (`libserial_channel::CONTROL_VA`, five pages from 0x3802_0000).
const DMA_PAGES_VA: u64 = DEVICE_WINDOW_VA + 0x4_0000;

const _: () = assert!(DMA_PAGES_VA >= DEVICE_WINDOW_VA + (MAX_DEVICE_PAGES as u64 + 5) * 4096);

/// Most pages of [`DmaPages`]: the ring's four periods.
const MAX_DMA_PAGES: usize = audio_pwm_bcm2835::PERIODS;

/// Pages of the driver's own memory for the DMA controller: consecutive
/// physical pages, mapped (cacheable, as every RAM Frame) from
/// [`DMA_PAGES_VA`], with the bus address the controller reaches the first at.
/// The controller does not snoop the CPU caches: what it reads is cleaned
/// first, what it writes is invalidated before the CPU reads it.
struct DmaPages {
    /// Wire keys of the Frames, one per page
    frames: [u64; MAX_DMA_PAGES],
    count: usize,
    bus: u32,
}

impl DmaPages {
    /// Carve `count` pages from the driver's memory into its table from
    /// `first_slot`, and map them. Returns them and the next free slot.
    fn carve(init: &DriverInit, first_slot: u64, count: usize) -> (Self, u64) {
        assert!(count <= MAX_DMA_PAGES);
        assert_ne!(
            init.memory_bus, NO_BUS_ADDRESS,
            "audio: the PWM node's bus has no DMA addresses for the driver's memory"
        );
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let first = u32::try_from(first_slot).expect("a slot fits in u32");
        // One Retype of several Frames carves consecutive pages.
        UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::FRAME,
                12,
                0,
                u32::try_from(count).expect("a few pages"),
                &table,
                first,
                Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
            )
            .unwrap_or_else(|error| panic!("audio: DMA pages Retype failed: {:?}", error.code()));
        let mut pages = Self {
            frames: [0; MAX_DMA_PAGES],
            count,
            bus: 0,
        };
        let mut first_paddr = 0;
        for (index, wire) in pages.frames[..count].iter_mut().enumerate() {
            let slot = first + u32::try_from(index).expect("a few pages");
            let key = libdevice_driver::local_key(init.table, slot);
            *wire = key.to_wire();
            let frame = FrameKey::from_key(key);
            let (paddr, _size) = frame
                .get_extent()
                .unwrap_or_else(|error| panic!("audio: DMA page extent: {:?}", error.code()));
            if index == 0 {
                first_paddr = paddr;
            }
            assert_eq!(
                paddr,
                first_paddr + index as u64 * 4096,
                "audio: the DMA pages are consecutive"
            );
            frame
                .map(
                    RawKey::from_wire(init.address_space),
                    DMA_PAGES_VA + index as u64 * 4096,
                    Rights(Rights::READ | Rights::WRITE),
                    FrameAttribute::Default,
                )
                .unwrap_or_else(|error| panic!("audio: DMA page Map failed: {:?}", error.code()));
        }
        pages.bus = first_paddr
            .checked_sub(init.memory_paddr)
            .and_then(|offset| init.memory_bus.checked_add(offset))
            .and_then(|bus| u32::try_from(bus).ok())
            .expect("the DMA pages have a 32-bit bus address");
        (pages, first_slot + count as u64)
    }

    /// The words of page `index`.
    fn words(&self, index: usize) -> &'static mut [u32] {
        assert!(index < self.count);
        // SAFETY: the page is mapped read/write for good; the DMA controller
        // reads or writes it only while the caller does not touch it.
        unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::with_exposed_provenance_mut::<u32>(
                    usize::try_from(DMA_PAGES_VA).expect("a 64-bit address") + index * 4096,
                ),
                4096 / size_of::<u32>(),
            )
        }
    }

    /// The bus address of byte `offset` of the pages.
    fn bus_address(&self, offset: usize) -> u32 {
        self.bus + u32::try_from(offset).expect("a few pages")
    }

    fn frame(&self, index: usize) -> FrameKey {
        FrameKey::from_key(RawKey::from_wire(self.frames[index]))
    }

    /// Clean bytes `start..end` of page `index` to the point of coherency,
    /// for the DMA controller to read.
    fn clean(&self, index: usize, start: u64, end: u64) {
        self.frame(index)
            .clean_data(start, end)
            .unwrap_or_else(|error| panic!("audio: CleanData failed: {:?}", error.code()));
    }

    /// Clean and discard bytes `start..end` of page `index`: before the DMA
    /// controller writes them (no dirty line may be written back over its
    /// bytes) and again after (no stale line may be read).
    #[cfg_attr(not(feature = "qemu"), allow(dead_code))]
    fn discard(&self, index: usize, start: u64, end: u64) {
        self.frame(index)
            .clean_invalidate_data(start, end)
            .unwrap_or_else(|error| {
                panic!("audio: CleanInvalidateData failed: {:?}", error.code())
            });
    }
}

/// The `EventCount` the dma component advances once per finished control
/// block on this driver's channels.
fn dma_events(dma: &dma_v1::Client) -> EventCountKey {
    let channel = dma
        .channel()
        .expect("audio: dma_v1 shares an io-channel with the driver");
    assert_ne!(channel.event, 0, "audio: the io-channel has an EventCount");
    EventCountKey::from_key(RawKey::from_wire(channel.event))
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
    let dma = dma_v1::Client::new(dma_v1::Bindings::from_imports(init.imports()));

    #[cfg(feature = "qemu")]
    {
        emulated::check_refusals(&mut trace, &clock);
        emulated::check_dma(&mut trace, init, &dma);
    }
    #[cfg(not(feature = "qemu"))]
    {
        let (pwm, next_slot) = tone::play_test(&mut trace, init);
        tone::play_ring(&mut trace, init, pwm, next_slot, &dma);
    }

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
        super::{DmaPages, PWM_CLOCK, clock_v1, dma_v1},
        audio_pwm_bcm2835::{MEMORY_DREQ, PWM_CLOCK_HZ, PWM_DREQ},
        libdevice_driver::DriverInit,
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

    /// Bytes of one block of the copies.
    const BLOCK_BYTES: u32 = 512;
    /// Blocks of the first copy: half the page, the other half receives it.
    const BLOCKS: u32 = 4;
    /// Bytes the first copy moves.
    const COPY_BYTES: usize = (BLOCK_BYTES * BLOCKS) as usize;
    /// How long a completion may take: QEMU runs a chain inside the write
    /// that starts it, so only the interrupt's way to the client counts.
    const COMPLETION_TIMEOUT_NS: u64 = 1_000_000_000;

    /// Check that `result` is the exception `expected`.
    fn refused<T: core::fmt::Debug>(result: Result<T, dma_v1::Error>, expected: dma_v1::Exception) {
        match result {
            Err(dma_v1::Error::Exception(exception)) if exception == expected => {}
            other => panic!("audio: expected {expected:?}, got {other:?}"),
        }
    }

    /// The word at index `index` of the source pattern of copy `copy`.
    fn pattern(copy: u32, index: usize) -> u32 {
        0xd0a0_0000 | (copy << 16) | u32::try_from(index).unwrap_or(u32::MAX)
    }

    /// Run a copy of `blocks` blocks of `BLOCK_BYTES` on `channel` from the
    /// page's words `source..` to `destination..` (word offsets), and check
    /// that the client's `EventCount` advances by `blocks` and the bytes
    /// arrive.
    fn copy_and_check(
        dma: &dma_v1::Client,
        pages: &DmaPages,
        channel: u32,
        copy: u32,
        source: usize,
        destination: usize,
        blocks: u32,
    ) {
        let words = (BLOCK_BYTES * blocks) as usize / size_of::<u32>();
        let page = pages.words(0);
        for (index, word) in page[source..source + words].iter_mut().enumerate() {
            *word = pattern(copy, index);
        }
        page[destination..destination + words].fill(0);
        let byte = |word: usize| (word * size_of::<u32>()) as u64;
        pages.clean(0, byte(source), byte(source + words));
        pages.discard(0, byte(destination), byte(destination + words));
        let events = super::dma_events(dma);
        let before = events.read().unwrap_or_else(|error| {
            panic!("audio: reading the DMA EventCount: {:?}", error.code())
        });
        dma.copy(
            channel,
            pages.bus_address(source * size_of::<u32>()),
            pages.bus_address(destination * size_of::<u32>()),
            BLOCK_BYTES,
            blocks,
        )
        .unwrap_or_else(|error| panic!("audio: DMA copy on channel {channel} failed: {error:?}"));
        let after = events
            .await_ge(before + u64::from(blocks), COMPLETION_TIMEOUT_NS)
            .unwrap_or_else(|error| {
                panic!(
                    "audio: no DMA completion on channel {channel}: {:?}",
                    error.code()
                )
            });
        assert_eq!(
            after - before,
            u64::from(blocks),
            "one completion per block of channel {channel}"
        );
        pages.discard(0, byte(destination), byte(destination + words));
        let page = pages.words(0);
        for index in 0..words {
            assert_eq!(
                page[destination + index],
                pattern(copy, index),
                "word {index} of the copy on channel {channel}"
            );
        }
        assert_eq!(dma.position(channel), Ok(blocks), "past the last block");
        assert_eq!(dma.errors(channel), Ok(0), "no DMA errors");
    }

    /// The dma component through `dma_v1`: a chain of four blocks copies
    /// half a page into the other half on the first channel, with one
    /// completion per block; every channel of the mask is handed out once;
    /// a channel on the line channels 11 to 14 share copies too; and the
    /// requests the component must refuse are refused.
    pub fn check_dma(trace: &mut Trace, init: &DriverInit, dma: &dma_v1::Client) {
        let (pages, _next_slot) = DmaPages::carve(init, init.first_free_slot, 1);
        let first = dma
            .claim(MEMORY_DREQ)
            .unwrap_or_else(|error| panic!("audio: claiming a DMA channel failed: {error:?}"));
        let half = COPY_BYTES / size_of::<u32>();
        copy_and_check(dma, &pages, first, 1, 0, half, BLOCKS);
        trace!(
            trace,
            "audio: DMA copied {COPY_BYTES} bytes in {BLOCKS} linked blocks on channel {first}: {BLOCKS} completions, destination matches"
        );

        // Every other channel the mask grants, then none.
        let mut claimed = [first; 16];
        let mut count = 1;
        while let Ok(channel) = dma.claim(MEMORY_DREQ) {
            claimed[count] = channel;
            count += 1;
        }
        assert_eq!(
            dma.claim(MEMORY_DREQ),
            Err(dma_v1::Error::Exception(dma_v1::Exception::NoChannel))
        );
        let claimed = &claimed[..count];
        trace!(trace, "audio: DMA channels handed out: {claimed:?}");
        // The last one sits on the line channels 11 to 14 share.
        let shared = *claimed.last().expect("channels");
        assert!(shared >= 11, "the mask grants channels 11 to 14");
        copy_and_check(dma, &pages, shared, 2, 0, half, 2);
        trace!(
            trace,
            "audio: DMA copied {} bytes in 2 blocks on channel {shared}, on the shared interrupt line",
            BLOCK_BYTES * 2
        );

        // Refusals.
        let source = pages.bus_address(0);
        let destination = pages.bus_address(COPY_BYTES);
        refused(
            dma.copy(first, source, destination, BLOCK_BYTES - 2, 1),
            dma_v1::Exception::BadTransfer,
        );
        refused(
            dma.copy(first, source, destination, BLOCK_BYTES, 9),
            dma_v1::Exception::BadTransfer,
        );
        refused(
            dma.start_ring(first, source, BLOCK_BYTES, 4),
            dma_v1::Exception::UnknownRequest { dreq: MEMORY_DREQ },
        );
        refused(
            dma.position(15),
            dma_v1::Exception::NotOwner { dma_channel: 15 },
        );
        for &channel in claimed {
            dma.release(channel)
                .unwrap_or_else(|error| panic!("audio: releasing channel {channel}: {error:?}"));
        }
        refused(
            dma.release(first),
            dma_v1::Exception::NotOwner { dma_channel: first },
        );
        refused(
            dma.claim(PWM_DREQ + 1),
            dma_v1::Exception::UnknownRequest { dreq: PWM_DREQ + 1 },
        );
        trace!(
            trace,
            "audio: dma_v1 refused a ragged length, nine blocks, a ring without a peripheral, a channel not claimed and DREQ {}",
            PWM_DREQ + 1
        );
    }
}

/// The polled test tone, on the board.
#[cfg(not(feature = "qemu"))]
mod tone {
    use {
        super::{DmaPages, dma_v1},
        audio_pwm_bcm2835::{
            Control, DmaConfig, FIFO_WORDS, MmioRegisterBlock, PERIOD_BYTES, PERIOD_FRAMES,
            PERIODS, PWM_CLOCK_HZ, PWM_DREQ, PeriodContent, RANGE, RATE_TOLERANCE_PPM,
            RegisterBlock, Side, Status, Tone, clear_errors, clearing, deviation_ppm, dma_feed,
            fifo_stereo, frame, has_errors, measured_rate_millihertz, period_content,
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
    pub struct Pwm {
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
    /// test tone. Returns the block, stopped, and the first free slot of the
    /// driver's table.
    pub fn play_test(trace: &mut Trace, init: &DriverInit) -> (Pwm, u64) {
        let next_slot = map_device_pages(init)
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
        (pwm, next_slot)
    }

    /// Milliseconds of tone the DMA ring plays.
    const RING_TONE_MS: u64 = 5_000;
    /// How long a period may take before the ring is given up: four periods'
    /// worth at 44.1 kHz is 46 ms.
    const PERIOD_TIMEOUT_NS: u64 = 1_000_000_000;

    /// Fill period slot `slot` of the ring with period `index` of the test
    /// (`period_content`), continuing `tone`, and clean it for the DMA.
    fn fill_period(pages: &DmaPages, slot: usize, index: u64, tone_periods: u64, tone: &mut Tone) {
        let silence = word(0, RANGE);
        let words = pages.words(slot);
        let ramp = PERIOD_FRAMES as u64;
        for (frame_index, pair) in words.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let step = frame_index as u64 + 1;
            *pair = match period_content(index, tone_periods) {
                PeriodContent::RampUp => {
                    let level = u32::try_from(u64::from(silence) * step / ramp).unwrap_or(silence);
                    [level, level]
                }
                PeriodContent::Tone => frame(Side::Both, tone.next_sample(), RANGE),
                PeriodContent::RampDown => {
                    let level =
                        u32::try_from(u64::from(silence) * (ramp - step) / ramp).unwrap_or(0);
                    [level, level]
                }
                // Right first: channel 1 takes the even words.
                PeriodContent::Off => [0, 0],
            };
        }
        pages.clean(slot, 0, PERIOD_BYTES as u64);
    }

    /// Play the test tone on both sides from a DMA ring of `PERIODS` periods
    /// of `PERIOD_FRAMES` frames feeding the FIFO, paced by the PWM's DREQ,
    /// refilling each period once the DMA has left it. The period interrupts
    /// are timed against the generic timer (`CNTVCT_EL0`) to measure the real
    /// sample rate, which also checks the assumed 500 MHz of PLLD.
    pub fn play_ring(
        trace: &mut Trace,
        init: &DriverInit,
        mut pwm: Pwm,
        next_slot: u64,
        dma: &dma_v1::Client,
    ) {
        let (pages, _next_slot) = DmaPages::carve(init, next_slot, PERIODS);
        let channel = dma
            .claim(PWM_DREQ)
            .unwrap_or_else(|error| panic!("audio: claiming a DMA channel failed: {error:?}"));
        let rate_millihertz = sample_rate_millihertz(PWM_CLOCK_HZ, RANGE);
        let tone_periods = frames(RING_TONE_MS, rate_millihertz) / PERIOD_FRAMES as u64;
        // The ramp up, the tone, the ramp down: then the ring runs on, off.
        let last_period = tone_periods + 1;
        let mut tone = Tone::new(TONE_HZ, rate_millihertz, ATTENUATION);
        for slot in 0..PERIODS {
            fill_period(&pages, slot, slot as u64, tone_periods, &mut tone);
        }

        // Both channels stopped, the FIFO cleared, DMA requests on (§9.6);
        // the ring starts filling the FIFO, then the channels start on it.
        // No `RPTL`: a FIFO the DMA did not refill in time shows as a gap.
        pwm.registers.write_control(Control::ZERO);
        pwm.registers.write_range1(RANGE);
        pwm.registers.write_range2(RANGE);
        pwm.registers.write_control(clearing());
        pwm.registers.write_dma_config(dma_feed());
        pwm.registers.write_status(clear_errors());
        let events = super::dma_events(dma);
        let base = events.read().unwrap_or_else(|error| {
            panic!("audio: reading the DMA EventCount: {:?}", error.code())
        });
        dma.start_ring(
            channel,
            pages.bus_address(0),
            u32::try_from(PERIOD_BYTES).expect("a page"),
            u32::try_from(PERIODS).expect("four periods"),
        )
        .unwrap_or_else(|error| panic!("audio: starting the DMA ring failed: {error:?}"));
        pwm.registers.write_control(fifo_stereo());
        trace!(
            trace,
            "audio: DMA ring on channel {channel}: {PERIODS} periods of {PERIOD_FRAMES} frames, {TONE_HZ} Hz on both sides for {RING_TONE_MS} ms"
        );

        // Period `finished` is the next to finish; slot `finished % PERIODS`
        // then takes period `finished + PERIODS`.
        let mut finished: u64 = 0;
        let mut late = 0_u64;
        let mut first_period = None;
        let mut last_period_seen = (0, 0);
        while finished <= last_period + 1 {
            let target = base + finished + 1;
            let Ok(now) = events.await_ge(target, PERIOD_TIMEOUT_NS) else {
                trace!(
                    trace,
                    "audio: DMA ring stalled after {finished} periods (position {:?})",
                    dma.position(channel)
                );
                break;
            };
            let at = libuser::clock::now_ns();
            let newly = now - base - finished;
            if newly >= PERIODS as u64 {
                // The DMA played periods that were not refilled in time.
                late += newly - (PERIODS as u64 - 1);
            }
            for _ in 0..newly {
                let slot = usize::try_from(finished % PERIODS as u64).expect("a slot");
                fill_period(
                    &pages,
                    slot,
                    finished + PERIODS as u64,
                    tone_periods,
                    &mut tone,
                );
                finished += 1;
            }
            first_period.get_or_insert((finished, at));
            last_period_seen = (finished, at);
        }

        // Read while the ring still feeds the FIFO: once it stops, the
        // channels find it empty.
        let status = pwm.registers.read_status();
        dma.stop(channel)
            .unwrap_or_else(|error| panic!("audio: stopping the DMA ring failed: {error:?}"));
        let dma_errors = dma.errors(channel).unwrap_or(u32::MAX);
        pwm.registers.write_control(Control::ZERO);
        pwm.registers.write_dma_config(DmaConfig::ZERO);
        pwm.registers.write_status(clear_errors());
        dma.release(channel)
            .unwrap_or_else(|error| panic!("audio: releasing the DMA channel failed: {error:?}"));

        if let Some((first_count, first_at)) = first_period
            && last_period_seen.0 > first_count
        {
            let frames_played = (last_period_seen.0 - first_count) * PERIOD_FRAMES as u64;
            let measured = measured_rate_millihertz(frames_played, last_period_seen.1 - first_at);
            let deviation = deviation_ppm(measured, rate_millihertz);
            trace!(
                trace,
                "audio: DMA ring rate {}.{:03} Hz measured over {frames_played} frames, computed {}.{:03} Hz: {deviation} ppm off, {}",
                measured / 1000,
                measured % 1000,
                rate_millihertz / 1000,
                rate_millihertz % 1000,
                if deviation <= RATE_TOLERANCE_PPM {
                    "within 0.1 %"
                } else {
                    "NOT within 0.1 %"
                }
            );
        }
        trace!(
            trace,
            "audio: DMA ring done: {finished} periods, {late} refilled late, write error {}, read error {}, gap right {} left {}, bus error {}, DMA errors {dma_errors:#x}",
            status.write_error(),
            status.read_error(),
            status.gap1(),
            status.gap2(),
            status.bus_error()
        );
    }
}
