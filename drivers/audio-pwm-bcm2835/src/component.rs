//! The driver's refill Thread and its `audio_v1` exports (see the crate
//! documentation in `main.rs`).

use {
    audio::audio_v1::{AudioV1, CHANNEL_PAGES, Exception},
    audio_pwm_bcm2835::{
        JACK_PINS, PERIOD_BYTES, PERIOD_FRAMES, PERIODS, PWM_CLOCK_HZ, RANGE,
        sample_rate_millihertz,
        stream::{
            CHANNEL_SLOTS, FRAME_BYTES, Fill, Queue, Ring, Stream, channel_at, ramp_down, ramp_up,
        },
    },
    clock_client::clock_v1,
    core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    dma_client::dma_v1,
    gpio_client::gpio_v1,
    libdevice_driver::{
        DEVICE_WINDOW_VA, DriverInit, INIT_VA, MAX_DEVICE_PAGES, NO_BUS_ADDRESS, driver_compatible,
        driver_init,
    },
    libio_channel::{Consumer, DataArea, NoEvent, Packet, Producer},
    libmeddle_runtime::PAGE_BYTES,
    libobject::{
        EventCountKey, FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey, Rights,
        UntypedKey, export::init_return_key, thread::ThreadReturnKey,
    },
    libserial_trace::{Trace, trace},
};

libuser::entry!(main);

driver_compatible!("brcm,bcm2835-pwm");

/// The PWM clock's number in the clock manager's device tree binding, the
/// second cell of the node's `clocks = <&clocks 30>`: Kickstart links the node
/// to the clock manager but does not pass the cell.
const PWM_CLOCK: u32 = 30;

// ── What the exports and the refill Thread share ───────────────────────────

/// The client with the output open, by its position among the data areas,
/// plus one; zero while the output is closed. `open` sets it, the refill
/// Thread clears it once the output is off.
static OWNER: AtomicU64 = AtomicU64::new(0);
/// `close` asked the refill Thread to ramp down and stop.
static CLOSING: AtomicBool = AtomicBool::new(false);
/// Underruns of the stream open now, or of the last one.
static UNDERRUNS: AtomicU32 = AtomicU32::new(0);
/// Wire key of the `EventCount` the exports advance to wake the refill Thread
/// while the output is closed.
static DOORBELL: AtomicU64 = AtomicU64::new(0);
/// Wire key of the `EventCount` the refill Thread advances each time the
/// output is off after `close`.
static CLOSED: AtomicU64 = AtomicU64::new(0);

fn init() -> &'static DriverInit {
    // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it is
    // read in this component's own `AddressSpace` only.
    unsafe { driver_init(INIT_VA) }
}

/// Wait until the refill Thread set the output up.
fn wait_ready() {
    EventCountKey::from_key(RawKey::from_wire(init().event_count))
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("audio: ready wait failed: {:?}", error.code()));
}

fn ring_doorbell() {
    EventCountKey::from_key(RawKey::from_wire(DOORBELL.load(Ordering::Acquire)))
        .advance(1)
        .unwrap_or_else(|error| panic!("audio: doorbell advance failed: {:?}", error.code()));
}

/// The client whose key carries `badge`, by its position among the data
/// areas.
fn client_of(badge: u64) -> Option<usize> {
    init().area_for_badge(badge).map(|(client, _area)| client)
}

/// The headphone jack, behind `audio_v1`. The calls run on their callers'
/// Threads and only hand requests to the refill Thread through the statics
/// above: the refill Thread alone touches the PWM block, the ring and
/// `dma_v1`.
struct Jack;

impl AudioV1 for Jack {
    fn enter(_badge: u64) {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    fn rate(_badge: u64) -> u32 {
        u32::try_from(sample_rate_millihertz(PWM_CLOCK_HZ, RANGE)).unwrap_or(u32::MAX)
    }

    fn open(badge: u64) -> Result<(), Exception> {
        wait_ready();
        let client = client_of(badge).ok_or(Exception::Busy)?;
        OWNER
            .compare_exchange(0, client as u64 + 1, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_owner| Exception::Busy)?;
        UNDERRUNS.store(0, Ordering::Release);
        ring_doorbell();
        Ok(())
    }

    fn close(badge: u64) -> Result<(), Exception> {
        wait_ready();
        let client = client_of(badge).ok_or(Exception::NotOpen)?;
        if OWNER.load(Ordering::Acquire) != client as u64 + 1 {
            return Err(Exception::NotOpen);
        }
        let closed = EventCountKey::from_key(RawKey::from_wire(CLOSED.load(Ordering::Acquire)));
        // The refill Thread advances it only after a close was asked for.
        let before = closed
            .read()
            .unwrap_or_else(|error| panic!("audio: reading closes failed: {:?}", error.code()));
        CLOSING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_closing| Exception::NotOpen)?;
        ring_doorbell();
        closed
            .await_ge(before + 1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("audio: close wait failed: {:?}", error.code()));
        Ok(())
    }

    fn underruns(_badge: u64) -> u32 {
        UNDERRUNS.load(Ordering::Acquire)
    }
}

audio::audio_v1_export!(Jack);

// ── Set-up ─────────────────────────────────────────────────────────────────

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

/// Where the ring's periods are mapped.
const RING_VA: u64 = DMA_PAGES_VA;

/// Most pages of [`DmaPages`]: the ring's four periods.
const MAX_DMA_PAGES: usize = PERIODS;

/// Pages of the driver's own memory for the DMA controller: consecutive
/// physical pages, mapped (cacheable, as every RAM Frame) from `va`, with the
/// bus address the controller reaches the first at. The controller does not
/// snoop the CPU caches: what it reads is cleaned first, what it writes is
/// invalidated before the CPU reads it.
struct DmaPages {
    /// Wire keys of the Frames, one per page
    frames: [u64; MAX_DMA_PAGES],
    count: usize,
    va: u64,
    bus: u32,
}

impl DmaPages {
    /// Carve `count` pages from the driver's memory into its table from
    /// `first_slot`, and map them from `va`. Returns them and the next free
    /// slot.
    fn carve(init: &DriverInit, first_slot: u64, count: usize, va: u64) -> (Self, u64) {
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
            va,
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
                    va + index as u64 * 4096,
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
                    usize::try_from(self.va).expect("a 64-bit address") + index * 4096,
                ),
                4096 / size_of::<u32>(),
            )
        }
    }

    /// The FIFO frames of page `index`, a period of the ring.
    fn frames(&self, index: usize) -> &'static mut [[u32; 2]] {
        self.words(index).as_chunks_mut::<2>().0
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

// ── The stream ─────────────────────────────────────────────────────────────

/// The driver's ends of the open client's io-channel.
struct ClientQueue<'channel> {
    submitted: Consumer<'channel, CHANNEL_SLOTS, NoEvent>,
    completed: Producer<'channel, CHANNEL_SLOTS, EventCountKey>,
    /// Packets handed back since the output opened
    returned: u64,
}

impl Queue for ClientQueue<'_> {
    fn next(&mut self) -> Option<Packet> {
        loop {
            match self.submitted.get() {
                Ok(packet) => return packet,
                // The client garbled its FIFO; what it published is gone.
                Err(_corrupt) => {}
            }
        }
    }

    fn complete(&mut self, packet: &Packet) {
        // The completion FIFO holds every packet the submission FIFO did.
        let _lost = self.completed.put(packet);
        self.returned += 1;
    }

    fn read_frame(&mut self, address: u64) -> u32 {
        // SAFETY: `Stream` reads only 4-byte aligned frames of records that
        // lie in the channel's data pages (`admits`), which Kickstart mapped
        // read/write for the driver's lifetime; the client does not write a
        // buffer while the driver holds its packet.
        unsafe {
            core::ptr::read_volatile(core::ptr::with_exposed_provenance::<u32>(
                usize::try_from(address).expect("a 64-bit address"),
            ))
        }
    }
}

impl ClientQueue<'_> {
    /// Advance the client's `EventCount` if packets went back.
    fn signal(&mut self) {
        self.completed
            .signal()
            .unwrap_or_else(|error| panic!("audio: completion advance failed: {:?}", error.code()));
    }
}

/// Most periods `close` plays out from the client's queue before it ramps
/// down: the frames the channel's data pages hold (eight periods). A client
/// still sending after that gets the rest back unplayed.
const MAX_DRAIN_PERIODS: u64 = CHANNEL_PAGES * PAGE_BYTES / (PERIOD_FRAMES as u64 * FRAME_BYTES);

/// What a period of the ring was filled with, kept per slot for the capture
/// check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Content {
    /// From no output up to silence
    RampUp,
    /// `frames` client frames from stream position `first`, then silence
    Stream { first: u64, frames: usize },
    /// From the FIFO words `from` down to no output
    RampDown { from: [u32; 2] },
    /// No output: every word 0
    Off,
}

/// One open output: the client's stream, the ring and what fills it.
struct Playback<'channel> {
    queue: ClientQueue<'channel>,
    data: DataArea,
    stream: Stream,
    ring: Ring,
    /// Periods filled from the client's queue since `close` was asked for
    drained: u64,
    /// The period the ramp down was written to, once the stream drained
    ramp_down: Option<u64>,
    contents: [Content; PERIODS],
}

impl Playback<'_> {
    /// Write period `period` into its slot of `ring_pages` and clean it.
    fn write_period(&mut self, ring_pages: &DmaPages, period: u64) {
        let slot = self.ring.slot(period);
        let frames = ring_pages.frames(slot);
        let content = if period == 0 {
            ramp_up(frames, RANGE);
            Content::RampUp
        } else if self.ramp_down.is_some() {
            frames.fill([0, 0]);
            Content::Off
        } else if CLOSING.load(Ordering::Acquire) {
            // Play out what the client queued, then ramp down.
            let fill = if self.drained < MAX_DRAIN_PERIODS {
                self.drained += 1;
                self.stream.drain(frames, &self.data, &mut self.queue)
            } else {
                Fill::default()
            };
            if fill.client_frames == 0 {
                let from = self.stream.last();
                ramp_down(frames, from);
                self.ramp_down = Some(period);
                // A client still sending after the drain's limit: what it
                // queued comes back unplayed.
                self.stream.finish(&mut self.queue);
                Content::RampDown { from }
            } else {
                Content::Stream {
                    first: self.stream.frames() - fill.client_frames as u64,
                    frames: fill.client_frames,
                }
            }
        } else {
            let fill = self.stream.fill(frames, &self.data, &mut self.queue);
            Content::Stream {
                first: self.stream.frames() - fill.client_frames as u64,
                frames: fill.client_frames,
            }
        };
        self.contents[slot] = content;
        ring_pages.clean(slot, 0, PERIOD_BYTES as u64);
    }
}

/// Play the stream of the client that opened the output until it closes
/// it: fill the ring (a ramp up, then the client's frames), start the
/// output, and refill each period the DMA leaves, as `dma_v1`'s `EventCount`
/// reports them; then ramp down, stop, and hand every packet back.
fn play(trace: &mut Trace, ring_pages: &DmaPages, output: &mut Output<'_>) {
    let init = init();
    let client = usize::try_from(OWNER.load(Ordering::Acquire) - 1).expect("a client");
    let area = init.areas()[client];
    // SAFETY: Kickstart mapped the channel's control page and data pages
    // read/write for the driver's lifetime.
    let channel = unsafe { channel_at(area.channel_base) };
    let (submitted, completed) = channel.server(
        NoEvent,
        EventCountKey::from_key(RawKey::from_wire(area.channel_event)),
    );
    let mut playback = Playback {
        queue: ClientQueue {
            submitted,
            completed,
            returned: 0,
        },
        data: DataArea {
            base: area.channel_base + PAGE_BYTES,
            len: CHANNEL_PAGES * PAGE_BYTES,
        },
        stream: Stream::new(RANGE),
        ring: Ring::new(PERIODS as u64),
        drained: 0,
        ramp_down: None,
        contents: [Content::Off; PERIODS],
    };
    for period in 0..PERIODS as u64 {
        playback.write_period(ring_pages, period);
    }
    playback.queue.signal();
    trace!(
        trace,
        "audio: open for client {client}: a ramp up, then its packets"
    );
    output.start(ring_pages);

    let mut late = 0;
    loop {
        let Some(finished) = output.wait(playback.ring.finished()) else {
            trace!(
                trace,
                "audio: the DMA stalled after {} periods",
                playback.ring.finished()
            );
            break;
        };
        for period in playback.ring.finished()..finished {
            output.played(
                trace,
                ring_pages,
                period,
                playback.contents[playback.ring.slot(period)],
            );
        }
        let refill = playback.ring.advance(finished);
        late += refill.late;
        for period in refill.first..refill.first + refill.count {
            playback.write_period(ring_pages, period);
        }
        playback.queue.signal();
        UNDERRUNS.store(playback.stream.underruns(), Ordering::Release);
        if playback
            .ramp_down
            .is_some_and(|period| playback.ring.has_played(period))
        {
            break;
        }
        output.next(ring_pages, playback.ring.finished());
    }

    output.stop(trace);
    // Packets the client put while the output was closing.
    playback.stream.finish(&mut playback.queue);
    playback.queue.signal();
    trace!(
        trace,
        "audio: closed after {} periods: {} client frames, {} underruns, {late} late, {} packets back",
        playback.ring.finished(),
        playback.stream.frames(),
        playback.stream.underruns(),
        playback.queue.returned
    );
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
    let (mut output, next_slot) = {
        emulated::check_refusals(&mut trace, &clock);
        let next_slot = emulated::check_dma(&mut trace, init, &dma, init.first_free_slot);
        emulated::Capture::new(init, &dma, next_slot)
    };
    #[cfg(not(feature = "qemu"))]
    let (mut output, next_slot) = board::Pwm::new(init, &dma);

    // The doorbell and the count of closes.
    let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
    let events_slot = u32::try_from(next_slot).expect("a slot fits in u32");
    let doorbell = UntypedKey::from_key(RawKey::from_wire(init.memory))
        .retype(
            ObjectType::EVENT_COUNT,
            0,
            0,
            2,
            &table,
            events_slot,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("audio: EventCount Retype failed: {:?}", error.code()));
    DOORBELL.store(doorbell.to_wire(), Ordering::Release);
    CLOSED.store(
        libdevice_driver::local_key(init.table, events_slot + 1).to_wire(),
        Ordering::Release,
    );
    let (ring_pages, _next_slot) = DmaPages::carve(init, next_slot + 2, PERIODS, RING_VA);
    trace!(
        trace,
        "audio: audio_v1 ready: a ring of {PERIODS} periods of {PERIOD_FRAMES} frames at bus {:#010x}, range {RANGE}",
        ring_pages.bus_address(0)
    );
    // Say the exports can be served.
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .advance(1)
        .unwrap_or_else(|error| panic!("audio: ready advance failed: {:?}", error.code()));

    // The refill Thread: this one. It sleeps on the doorbell while the
    // output is closed, and on dma_v1's EventCount while it plays.
    let doorbell = EventCountKey::from_key(doorbell);
    let mut rung = 0;
    loop {
        if OWNER.load(Ordering::Acquire) == 0 {
            rung = doorbell
                .await_ge(rung + 1, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("audio: doorbell wait failed: {:?}", error.code()));
            continue;
        }
        play(&mut trace, &ring_pages, &mut output);
        CLOSING.store(false, Ordering::Release);
        OWNER.store(0, Ordering::Release);
        EventCountKey::from_key(RawKey::from_wire(CLOSED.load(Ordering::Acquire)))
            .advance(1)
            .unwrap_or_else(|error| panic!("audio: closed advance failed: {:?}", error.code()));
    }
}

/// How long a period may take before the DMA is given up on: four periods'
/// worth at 44.1 kHz is 46 ms.
const PERIOD_TIMEOUT_NS: u64 = 1_000_000_000;

#[cfg(feature = "qemu")]
use emulated::Capture as Output;

#[cfg(not(feature = "qemu"))]
use board::Pwm as Output;

/// The output on the board: the PWM block fed by a DMA ring paced by its
/// DREQ.
#[cfg(not(feature = "qemu"))]
mod board {
    use {
        super::{DmaPages, PERIOD_TIMEOUT_NS, dma_events, dma_v1},
        audio_pwm_bcm2835::{
            Control, DmaConfig, MmioRegisterBlock, PERIOD_BYTES, PERIOD_FRAMES, PERIODS,
            PWM_CLOCK_HZ, PWM_DREQ, RANGE, RATE_TOLERANCE_PPM, RegisterBlock, clear_errors,
            clearing, deviation_ppm, dma_feed, fifo_stereo, measured_rate_millihertz,
            sample_rate_millihertz,
        },
        libdevice_driver::{DriverInit, device_address, map_device_pages},
        libobject::EventCountKey,
        libserial_trace::{Trace, trace},
    };

    /// The PWM block and the DMA channel feeding it.
    pub struct Pwm<'dma> {
        registers: MmioRegisterBlock<'static>,
        dma: &'dma dma_v1::Client,
        channel: u32,
        events: EventCountKey,
        /// `events` when the ring started
        base: u64,
        /// The first and the latest period completion seen, and when: the
        /// measured rate
        first: Option<(u64, u64)>,
        latest: (u64, u64),
    }

    impl<'dma> Pwm<'dma> {
        /// Map the block, stop it, and claim a DMA channel for its DREQ.
        /// Returns it and the first free slot of the driver's table.
        pub fn new(init: &DriverInit, dma: &'dma dma_v1::Client) -> (Self, u64) {
            let next_slot = map_device_pages(init).unwrap_or_else(|error| {
                panic!("audio: mapping registers failed: {:?}", error.code())
            });
            let address =
                device_address(init, init.regions()[0].start).expect("audio: registers mapped");
            // SAFETY: mapped for good just above; the refill Thread is the
            // only one touching the block.
            let mut registers = unsafe { RegisterBlock::new_mmio(address as *mut RegisterBlock) };
            registers.write_control(Control::ZERO);
            registers.write_dma_config(DmaConfig::ZERO);
            let channel = dma
                .claim(PWM_DREQ)
                .unwrap_or_else(|error| panic!("audio: claiming a DMA channel failed: {error:?}"));
            (
                Self {
                    registers,
                    dma,
                    channel,
                    events: dma_events(dma),
                    base: 0,
                    first: None,
                    latest: (0, 0),
                },
                next_slot,
            )
        }

        /// Start the channels on the ring, every slot filled and cleaned:
        /// both channels stopped, the same range on both (§9.6, `FIF1`), the
        /// FIFO cleared, DMA requests on; the ring starts filling the FIFO,
        /// then the channels start on it. No `RPTL`: a FIFO the DMA did not
        /// refill in time shows as a gap.
        pub fn start(&mut self, ring: &DmaPages) {
            self.registers.write_control(Control::ZERO);
            self.registers.write_range1(RANGE);
            self.registers.write_range2(RANGE);
            self.registers.write_control(clearing());
            self.registers.write_dma_config(dma_feed());
            self.registers.write_status(clear_errors());
            self.base = self.events.read().unwrap_or_else(|error| {
                panic!("audio: reading the DMA EventCount: {:?}", error.code())
            });
            self.first = None;
            self.dma
                .start_ring(
                    self.channel,
                    ring.bus_address(0),
                    u32::try_from(PERIOD_BYTES).expect("a page"),
                    u32::try_from(PERIODS).expect("four periods"),
                )
                .unwrap_or_else(|error| panic!("audio: starting the DMA ring failed: {error:?}"));
            self.registers.write_control(fifo_stereo());
        }

        /// Wait until the DMA finished more than `finished` periods; how
        /// many it has finished in all, or `None` if it stalled.
        pub fn wait(&mut self, finished: u64) -> Option<u64> {
            let now = self
                .events
                .await_ge(self.base + finished + 1, PERIOD_TIMEOUT_NS)
                .ok()?;
            let at = libuser::clock::now_ns();
            let total = now - self.base;
            self.first.get_or_insert((total, at));
            self.latest = (total, at);
            Some(total)
        }

        /// The DMA finished `period`: nothing to check on the board.
        #[expect(clippy::unused_self, reason = "the same calls as the capture's")]
        pub fn played(
            &self,
            _trace: &mut Trace,
            _ring: &DmaPages,
            _period: u64,
            _content: super::Content,
        ) {
        }

        /// The ring runs on by itself.
        #[expect(clippy::unused_self, reason = "the same calls as the capture's")]
        pub fn next(&self, _ring: &DmaPages, _period: u64) {}

        /// Stop the ring and the channels, and trace the measured rate, the
        /// `STA` flags and the DMA errors.
        pub fn stop(&mut self, trace: &mut Trace) {
            // Read while the ring still feeds the FIFO: once it stops, the
            // channels find it empty.
            let status = self.registers.read_status();
            self.dma
                .stop(self.channel)
                .unwrap_or_else(|error| panic!("audio: stopping the DMA ring failed: {error:?}"));
            let dma_errors = self.dma.errors(self.channel).unwrap_or(u32::MAX);
            self.registers.write_control(Control::ZERO);
            self.registers.write_dma_config(DmaConfig::ZERO);
            self.registers.write_status(clear_errors());
            let rate_millihertz = sample_rate_millihertz(PWM_CLOCK_HZ, RANGE);
            if let Some((first_count, first_at)) = self.first
                && self.latest.0 > first_count
            {
                let frames = (self.latest.0 - first_count) * PERIOD_FRAMES as u64;
                let measured = measured_rate_millihertz(frames, self.latest.1 - first_at);
                let deviation = deviation_ppm(measured, rate_millihertz);
                trace!(
                    trace,
                    "audio: rate {}.{:03} Hz measured over {frames} frames, computed {}.{:03} Hz: {deviation} ppm off, {}",
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
                "audio: stopped: write error {}, read error {}, gap right {} left {}, bus error {}, DMA errors {dma_errors:#x}",
                status.write_error(),
                status.read_error(),
                status.gap1(),
                status.gap2(),
                status.bus_error()
            );
        }
    }
}

/// What QEMU can check: the clock manager's and the DMA controller's models,
/// without the PWM block, and the stream through a capture buffer.
#[cfg(feature = "qemu")]
mod emulated {
    use {
        super::{
            Content, DMA_PAGES_VA, DmaPages, PERIOD_TIMEOUT_NS, PWM_CLOCK, clock_v1, dma_events,
            dma_v1,
        },
        audio_pwm_bcm2835::{
            MEMORY_DREQ, PERIOD_BYTES, PERIOD_FRAMES, PERIODS, PWM_CLOCK_HZ, PWM_DREQ, RANGE,
            sample_rate_millihertz,
            stream::{
                expected_stream_period, is_audible, ramp_down, ramp_up, silent_frame,
                starts_with_test_pattern,
            },
        },
        libdevice_driver::DriverInit,
        libobject::EventCountKey,
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
        trace!(
            trace,
            "audio: QEMU has no PWM block: the stream goes to a capture buffer"
        );
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
    /// Where the page of the copy checks is mapped: after the ring's and the
    /// capture buffer's pages.
    const CHECK_VA: u64 = DMA_PAGES_VA + 2 * PERIODS as u64 * 4096;
    /// Where the capture buffer is mapped: after the ring.
    const CAPTURE_VA: u64 = DMA_PAGES_VA + PERIODS as u64 * 4096;

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
        let events = dma_events(dma);
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
    /// requests the component must refuse are refused. Every channel is
    /// released after. Returns the next free slot after `first_slot`.
    pub fn check_dma(
        trace: &mut Trace,
        init: &DriverInit,
        dma: &dma_v1::Client,
        first_slot: u64,
    ) -> u64 {
        let (pages, next_slot) = DmaPages::carve(init, first_slot, 1, CHECK_VA);
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
        next_slot
    }

    /// What the capture saw of one stream.
    #[derive(Debug, Default, Clone, Copy)]
    struct Tally {
        ramp_up: u64,
        stream: u64,
        silent: u64,
        /// Stream periods with a client frame that is not silence
        audible: u64,
        client_frames: u64,
        ramp_down: u64,
        off: u64,
    }

    /// What the stream periods of a stream are checked against.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum StreamCheck {
        /// The capture test's pattern, converted: the stream started with
        /// it (`starts_with_test_pattern`)
        Pattern,
        /// What the stream filled the period with: any other client's
        /// sound
        AsFilled,
    }

    /// The output under QEMU: QEMU runs a DMA chain inside the write that
    /// starts it, so a ring would never end. Instead each period of the ring
    /// is played by a one-block `dma_v1.copy` from its slot into the same
    /// slot of a capture buffer, paced by the period's length; each copy's
    /// completion is the `EventCount` advance a period of the ring makes on
    /// the board. The captured words are checked against what the period
    /// was filled with: the ramps and silence as computed here, and a
    /// period of the stream, when the stream started with the capture
    /// test's pattern (`test_pattern`), against the pattern converted (the
    /// samples arrive converted, in order, with silence where the client
    /// sent none), and otherwise against the ring slot the copy came from,
    /// counting the periods that are not silent.
    pub struct Capture<'dma> {
        dma: &'dma dma_v1::Client,
        channel: u32,
        events: EventCountKey,
        pages: DmaPages,
        /// `events` when the stream started
        base: u64,
        /// When period 0's copy started
        started_ns: u64,
        tally: Tally,
        /// Decided at the stream's first period with client frames
        check: Option<StreamCheck>,
    }

    /// Nanoseconds of one period at the output rate.
    fn period_ns() -> u64 {
        PERIOD_FRAMES as u64 * 1_000_000_000_000 / sample_rate_millihertz(PWM_CLOCK_HZ, RANGE)
    }

    impl<'dma> Capture<'dma> {
        /// Carve the capture buffer and claim a memory DMA channel. Returns
        /// it and the next free slot after `first_slot`.
        pub fn new(init: &DriverInit, dma: &'dma dma_v1::Client, first_slot: u64) -> (Self, u64) {
            let (pages, next_slot) = DmaPages::carve(init, first_slot, PERIODS, CAPTURE_VA);
            let channel = dma
                .claim(MEMORY_DREQ)
                .unwrap_or_else(|error| panic!("audio: claiming a DMA channel failed: {error:?}"));
            (
                Self {
                    dma,
                    channel,
                    events: dma_events(dma),
                    pages,
                    base: 0,
                    started_ns: 0,
                    tally: Tally::default(),
                    check: None,
                },
                next_slot,
            )
        }

        /// Copy period `period`'s slot of `ring` into the capture buffer.
        fn copy(&self, ring: &DmaPages, period: u64) {
            let slot = usize::try_from(period % PERIODS as u64).expect("a slot");
            self.pages.discard(slot, 0, PERIOD_BYTES as u64);
            self.dma
                .copy(
                    self.channel,
                    ring.bus_address(slot * PERIOD_BYTES),
                    self.pages.bus_address(slot * PERIOD_BYTES),
                    u32::try_from(PERIOD_BYTES).expect("a page"),
                    1,
                )
                .unwrap_or_else(|error| panic!("audio: capture copy failed: {error:?}"));
        }

        pub fn start(&mut self, ring: &DmaPages) {
            self.base = self.events.read().unwrap_or_else(|error| {
                panic!("audio: reading the DMA EventCount: {:?}", error.code())
            });
            self.tally = Tally::default();
            self.check = None;
            self.started_ns = libuser::clock::now_ns();
            self.copy(ring, 0);
        }

        /// Wait for the copy of period `finished`; `finished` + 1 periods
        /// done, or `None` if it never completed.
        pub fn wait(&mut self, finished: u64) -> Option<u64> {
            let now = self
                .events
                .await_ge(self.base + finished + 1, PERIOD_TIMEOUT_NS)
                .ok()?;
            assert_eq!(now - self.base, finished + 1, "one completion per copy");
            Some(finished + 1)
        }

        /// Check the capture of `period` against `content`, what its slot
        /// of `ring` was filled with.
        pub fn played(
            &mut self,
            trace: &mut Trace,
            ring: &DmaPages,
            period: u64,
            content: Content,
        ) {
            let slot = usize::try_from(period % PERIODS as u64).expect("a slot");
            self.pages.discard(slot, 0, PERIOD_BYTES as u64);
            let mut expected = [[0_u32; 2]; PERIOD_FRAMES];
            match content {
                Content::RampUp => {
                    ramp_up(&mut expected, RANGE);
                    self.tally.ramp_up += 1;
                }
                Content::Stream { first, frames } => {
                    let filled = ring.frames(slot);
                    if self.check.is_none() && frames > 0 {
                        self.check = Some(
                            if first == 0 && starts_with_test_pattern(filled, frames, RANGE) {
                                StreamCheck::Pattern
                            } else {
                                StreamCheck::AsFilled
                            },
                        );
                    }
                    if self.check == Some(StreamCheck::Pattern) {
                        expected_stream_period(&mut expected, first, frames, RANGE);
                    } else {
                        // The client's frames as the stream converted them,
                        // then silence.
                        let client_frames = frames.min(PERIOD_FRAMES);
                        expected[..client_frames].copy_from_slice(&filled[..client_frames]);
                        expected[client_frames..].fill(silent_frame(RANGE));
                    }
                    self.tally.stream += 1;
                    self.tally.client_frames += frames as u64;
                    if frames < PERIOD_FRAMES {
                        self.tally.silent += 1;
                    }
                    if is_audible(&expected[..frames.min(PERIOD_FRAMES)], RANGE) {
                        self.tally.audible += 1;
                        if self.check == Some(StreamCheck::AsFilled)
                            && self.tally.audible.is_power_of_two()
                        {
                            trace!(
                                trace,
                                "audio: capture: {} audible stream periods of {}",
                                self.tally.audible,
                                self.tally.stream
                            );
                        }
                    }
                }
                Content::RampDown { from } => {
                    ramp_down(&mut expected, from);
                    self.tally.ramp_down += 1;
                }
                Content::Off => self.tally.off += 1,
            }
            let captured = self.pages.frames(slot);
            if let Some(index) =
                (0..PERIOD_FRAMES).find(|&index| captured[index] != expected[index])
            {
                panic!(
                    "audio: captured period {period} ({content:?}) differs at frame {index}: {:?}, expected {:?}",
                    captured[index], expected[index]
                );
            }
        }

        /// Copy period `period` once its time has come.
        pub fn next(&mut self, ring: &DmaPages, period: u64) {
            let due = self.started_ns + period * period_ns();
            let now = libuser::clock::now_ns();
            if due > now {
                // Sleep: await a count the DMA EventCount does not reach.
                let _timed_out = self.events.await_ge(u64::MAX, due - now);
            }
            self.copy(ring, period);
        }

        pub fn stop(&mut self, trace: &mut Trace) {
            let tally = self.tally;
            let dma_errors = self.dma.errors(self.channel).unwrap_or(u32::MAX);
            if self.check == Some(StreamCheck::Pattern) {
                trace!(
                    trace,
                    "audio: capture checked: {} ramp up, {} stream periods with {} client frames of the pattern in order, {} of them silent at the end, {} ramp down, {} off; DMA errors {dma_errors:#x}",
                    tally.ramp_up,
                    tally.stream,
                    tally.client_frames,
                    tally.silent,
                    tally.ramp_down,
                    tally.off,
                );
            } else {
                trace!(
                    trace,
                    "audio: capture checked: {} ramp up, {} stream periods with {} client frames as filled, {} audible, {} of them silent at the end, {} ramp down, {} off; DMA errors {dma_errors:#x}",
                    tally.ramp_up,
                    tally.stream,
                    tally.client_frames,
                    tally.audible,
                    tally.silent,
                    tally.ramp_down,
                    tally.off,
                );
            }
        }
    }
}
