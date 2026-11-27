// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The framebuffer component: the Raspberry Pi's display behind `fb_v1` (see
//! `interfaces/fb_v1.if`) and, as a scanout output, `display_v1` (see
//! `drivers/display/interfaces/display_v1.if`).
//!
//! Kickstart loads it for the framebuffer node (`brcm,bcm2708-fb`) and wires
//! its import of `mailbox_v1` through the firmware node. Configuring the
//! display sends two property buffers through the data area the component
//! shares with the mailbox (see `fb_impl`); the answers are kept for the
//! getters, after checking that the framebuffer lies in the `VideoCore` memory
//! Kickstart handed it (`DriverInit::video`). `frames` then hands the Frames
//! that cover the framebuffer to a client, carved straight into a table the
//! client lends for the call (`CallWithKey`); mapping and drawing is up to
//! the client.
//!
//! At start it reads the display's EDID base block through the mailbox (the
//! firmware's `Get EDID block`): the modes are the sizes it names, preferred
//! first, and its image size the display's millimetres. It then configures
//! the first mode the firmware takes, so the display has a mode before any
//! client asks. Without an EDID (nothing attached, or an emulator that does
//! not know the tag) the one mode is 1024x768, of unknown size.
//! `fb_v1.configure` still sets any size, and `display_v1.set_mode` one of
//! the modes, until the memory is handed out.

use fb::fb_v1::{__runtime::KeyTableKey, Exception, FbV1, PixelOrder};

/// `firmware_error` code when the mailbox cannot be reached or answers
/// something unexpected.
const MAILBOX_FAILED: u32 = u32::MAX;

/// The display.
struct Display;

impl FbV1 for Display {
    #[cfg(target_os = "vesper")]
    fn enter(_badge: u64) {
        component::record_return_key();
    }

    fn configure(width: u32, height: u32, depth: u32) -> Result<u32, Exception> {
        configure(width, height, depth)
    }

    fn address() -> Result<u64, Exception> {
        configured().map(|display| display.address)
    }

    fn size() -> Result<u32, Exception> {
        configured().map(|display| display.bytes)
    }

    fn pixel_order() -> Result<PixelOrder, Exception> {
        configured().map(|display| display.pixel_order)
    }

    fn frames(into: &KeyTableKey, first_slot: u32) -> Result<u32, Exception> {
        frames(into, first_slot)
    }
}

fb::fb_v1_export!(Display);

/// What configuring the display produced.
#[derive(Clone, Copy)]
struct Configured {
    address: u64,
    bytes: u32,
    pixel_order: PixelOrder,
}

#[cfg(target_os = "vesper")]
use component::{configure, configured, frames};

#[cfg(not(target_os = "vesper"))]
fn frames(_into: &KeyTableKey, _first_slot: u32) -> Result<u32, Exception> {
    Err(Exception::NotConfigured)
}

#[cfg(not(target_os = "vesper"))]
fn configure(_width: u32, _height: u32, _depth: u32) -> Result<u32, Exception> {
    Err(Exception::FirmwareError {
        code: MAILBOX_FAILED,
    })
}

#[cfg(not(target_os = "vesper"))]
fn configured() -> Result<Configured, Exception> {
    Err(Exception::NotConfigured)
}

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::{Configured, Exception, MAILBOX_FAILED},
        core::{
            cell::UnsafeCell,
            sync::atomic::{AtomicU32, AtomicU64, Ordering},
        },
        display::display_v1::{DisplayV1, Exception as DisplayException, Format, Rotation},
        fb::fb_v1::PixelOrder,
        fb_impl::{
            ATTRIBUTE_WORDS, CONFIGURE_WORDS, EDID_WORDS, FramePlan, PAGE_BYTES,
            attributes_request, configure_request, covered_by,
            edid::{Edid, Modes, Timing},
            edid_request, page_range, physical_address, pixel_bytes, read_allocation,
            read_attributes, read_edid,
        },
        libdevice_driver::{
            DriverInit, INIT_VA, MAX_VIDEO_BLOCKS, NO_BUS_ADDRESS, Turnstile, driver_compatible,
            driver_init, skip_chunks,
        },
        libmeddle_runtime::IoRec,
        libobject::{
            EventCountKey, FrameAttribute, KeyTableKey, NotificationKey, ObjectType, RawKey,
            Rights, UntypedKey, export::init_return_key, thread::ThreadReturnKey,
        },
        libserial_trace::{Trace, trace},
        mailbox_client::mailbox_v1::{Bindings, Client, Error, Exception as MailboxException},
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2708-fb");

    /// The configured display: zero until `configure` succeeds, then its
    /// physical address (the size and pixel order follow it).
    static ADDRESS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);
    static PIXEL_ORDER: AtomicU64 = AtomicU64::new(0);
    /// Bytes per pixel of the configured mode, as its pitch shows them.
    static PIXEL_BYTES: AtomicU32 = AtomicU32::new(4);

    /// The configured mode: its size in pixels and its pitch in bytes.
    static WIDTH: AtomicU32 = AtomicU32::new(0);
    static HEIGHT: AtomicU32 = AtomicU32::new(0);
    static PITCH: AtomicU32 = AtomicU32::new(0);

    /// The display's image size in millimetres (0: unknown), from its EDID.
    static WIDTH_MM: AtomicU32 = AtomicU32::new(0);
    static HEIGHT_MM: AtomicU32 = AtomicU32::new(0);

    /// `display_v1`'s generation: advanced whenever the configured mode
    /// changes.
    static GENERATION: AtomicU32 = AtomicU32::new(1);

    /// The depth every `display_v1` mode is configured with: 32-bit pixels,
    /// alpha ignored.
    const DEPTH: u32 = 32;

    /// The mode of a display without an EDID.
    const FALLBACK_MODE: Timing = Timing {
        width: 1024,
        height: 768,
        refresh_mhz: 0,
    };

    /// The display's modes, set once by `main` before the component says it
    /// is ready, and only read after.
    struct SharedModes(UnsafeCell<Modes>);

    // SAFETY: `main` writes the cell before any call can enter (every call
    // waits for the ready advance) and nothing writes it after, so every later
    // access is a read.
    unsafe impl Sync for SharedModes {}

    static MODES: SharedModes = SharedModes(UnsafeCell::new(Modes::empty()));

    /// The display's modes.
    fn modes() -> &'static Modes {
        // SAFETY: see `SharedModes`: written before the ready advance only.
        unsafe { &*MODES.0.get() }
    }

    /// Admits one caller at a time: the mailbox stack it calls on is single-user.
    static TURNSTILE: Turnstile = Turnstile::new();

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// Record this component's `Thread.Return` key for its exports.
    pub fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    /// The firmware's response code behind a failed mailbox call.
    fn firmware_code(error: &Error) -> u32 {
        match error {
            Error::Exception(MailboxException::FirmwareError { code }) => *code,
            _ => MAILBOX_FAILED,
        }
    }

    fn firmware_error(code: u32) -> Exception {
        Exception::FirmwareError { code }
    }

    /// Send `request` through the shared data area and read the answer back.
    fn exchange<const WORDS: usize>(
        mailbox: &Client,
        request: &[u32; WORDS],
    ) -> Result<[u32; WORDS], Exception> {
        let area = mailbox
            .data_area()
            .ok_or_else(|| firmware_error(MAILBOX_FAILED))?;
        let bytes = u64::try_from(WORDS * 4).unwrap_or(0);
        let reference = area
            .reference(0, bytes)
            .ok_or_else(|| firmware_error(MAILBOX_FAILED))?;
        if !area.write_words(0, request) {
            return Err(firmware_error(MAILBOX_FAILED));
        }
        mailbox
            .property(reference)
            .map_err(|error| firmware_error(firmware_code(&error)))?;
        let mut answer = [0; WORDS];
        if !area.read_words(0, &mut answer) {
            return Err(firmware_error(MAILBOX_FAILED));
        }
        Ok(answer)
    }

    /// Configure the display and return its pitch in bytes.
    pub fn configure(width: u32, height: u32, depth: u32) -> Result<u32, Exception> {
        let init = init();
        let _turn = TURNSTILE.enter(init);
        configure_holding_turn(&mut Trace::new(init), init, width, height, depth)
    }

    /// [`configure`] for a caller holding a turn, or for `main` before the
    /// component is ready (when no call can enter). Each firmware request is
    /// traced before it is sent, so a request the firmware never answers shows
    /// as the last line.
    fn configure_holding_turn(
        trace: &mut Trace,
        init: &DriverInit,
        width: u32,
        height: u32,
        depth: u32,
    ) -> Result<u32, Exception> {
        let mailbox = Client::new(Bindings::from_imports(init.imports()));

        trace!(trace, "fb:   allocating {width}x{height}x{depth}");
        let answer: [u32; CONFIGURE_WORDS] =
            exchange(&mailbox, &configure_request(width, height, depth))?;
        let allocation = read_allocation(&answer).map_err(firmware_error)?;
        trace!(
            trace,
            "fb:   allocated {:#x} bytes at bus {:#x}; reading the pixel order and pitch",
            allocation.bytes,
            allocation.bus_address
        );
        let answer: [u32; ATTRIBUTE_WORDS] = exchange(&mailbox, &attributes_request())?;
        let attributes = read_attributes(&answer).map_err(firmware_error)?;
        // The pitch tells the pixel size: the Raspberry Pi 5's firmware gives
        // 16-bit pixels whatever depth is asked for, and reports the depth
        // asked for.
        let pixel_bytes = pixel_bytes(width, attributes.pitch);
        trace!(
            trace,
            "fb:   pixel order {}, pitch {}, firmware depth {}, pixels {}",
            attributes.pixel_order,
            attributes.pitch,
            attributes.depth,
            match pixel_bytes {
                Some(4) => "32-bit",
                Some(2) => "16-bit",
                _ => "of no size the pitch allows",
            }
        );
        let Some(pixel_bytes) = pixel_bytes else {
            return Err(firmware_error(attributes.pitch));
        };
        if attributes.depth != pixel_bytes * 8 {
            trace!(
                trace,
                "fb:   the firmware reports depth {} for {}-bit pixels: the pitch is trusted",
                attributes.depth,
                pixel_bytes * 8
            );
        }
        match attributes.pixel_order {
            0 | 1 => {}
            other => return Err(firmware_error(other)),
        }

        // The firmware answers with a bus address; the offset to the physical
        // address is the one between the driver's memory and its bus view.
        let bus_offset = if init.memory_bus == NO_BUS_ADDRESS {
            u64::MAX
        } else {
            init.memory_bus.saturating_sub(init.memory_paddr)
        };
        let physical = physical_address(allocation.bus_address, bus_offset);
        // The memory must be among the blocks of video memory this component
        // holds, for the Frames handed to a client to come from them.
        let mut blocks = [(0, 0); MAX_VIDEO_BLOCKS];
        for (slot, block) in blocks.iter_mut().zip(init.video()) {
            *slot = (block.base, block.end());
        }
        let video = init.video();
        trace!(
            trace,
            "fb:   physical {physical:#x} (bus offset {bus_offset:#x}); video memory {} blocks, {:#x}..{:#x}",
            video.len(),
            video.first().map_or(0, |block| block.base),
            video.last().map_or(0, libdevice_driver::DeviceBlock::end)
        );
        if !covered_by(&blocks, physical, u64::from(allocation.bytes)) {
            return Err(Exception::OutOfVideoMemory);
        }
        let changed = (
            WIDTH.load(Ordering::Acquire),
            HEIGHT.load(Ordering::Acquire),
            PITCH.load(Ordering::Acquire),
            PIXEL_ORDER.load(Ordering::Acquire),
            PIXEL_BYTES.load(Ordering::Acquire),
            ADDRESS.load(Ordering::Acquire),
        ) != (
            width,
            height,
            attributes.pitch,
            u64::from(attributes.pixel_order),
            pixel_bytes,
            physical,
        );
        BYTES.store(u64::from(allocation.bytes), Ordering::Release);
        PIXEL_ORDER.store(u64::from(attributes.pixel_order), Ordering::Release);
        PIXEL_BYTES.store(pixel_bytes, Ordering::Release);
        WIDTH.store(width, Ordering::Release);
        HEIGHT.store(height, Ordering::Release);
        PITCH.store(attributes.pitch, Ordering::Release);
        ADDRESS.store(physical, Ordering::Release);
        if changed {
            GENERATION.fetch_add(1, Ordering::AcqRel);
        }
        trace!(trace, "fb:   configured");
        Ok(attributes.pitch)
    }

    /// The configured display, or `not_configured`. It waits until `main` has
    /// configured the first mode: a client's call can come before the
    /// component's own Thread finished setting up.
    pub fn configured() -> Result<Configured, Exception> {
        EventCountKey::from_key(RawKey::from_wire(init().event_count))
            .await_ge(1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("fb: ready wait failed: {:?}", error.code()));
        let address = ADDRESS.load(Ordering::Acquire);
        if address == 0 {
            return Err(Exception::NotConfigured);
        }
        Ok(Configured {
            address,
            bytes: u32::try_from(BYTES.load(Ordering::Acquire)).unwrap_or(u32::MAX),
            pixel_order: if PIXEL_ORDER.load(Ordering::Acquire) == 0 {
                PixelOrder::Bgr
            } else {
                PixelOrder::Rgb
            },
        })
    }

    /// Rights of the Frames handed to a client: to map them, and to learn
    /// their extent.
    const FRAME_RIGHTS: Rights = Rights(Rights::READ | Rights::WRITE | Rights::GRANT);

    /// The loosest memory attribute a client may map the framebuffer with.
    /// A framebuffer is written, never read back for effect, so write-combining
    /// suits it and the strict device attributes need no ceiling of their own.
    const FRAME_ATTRIBUTE_CEILING: FrameAttribute = FrameAttribute::WriteCombining;

    /// Next free slot of the component's own table: the chunks of video memory
    /// skipped on the way to the framebuffer land there. The caller holding
    /// the turnstile owns it.
    static NEXT_OWN_SLOT: AtomicU64 = AtomicU64::new(0);

    /// Where the watermark of each video block's Untyped stands, as an offset
    /// from the block's base: Retype only allocates above it, so memory below
    /// it is gone for good. Guarded by the turnstile.
    static WATERMARKS: [AtomicU64; MAX_VIDEO_BLOCKS] =
        [const { AtomicU64::new(0) }; MAX_VIDEO_BLOCKS];

    /// The pages `[HANDED_BASE, HANDED_END)` of video memory were handed out
    /// (zero: none). Guarded by the turnstile.
    static HANDED_BASE: AtomicU64 = AtomicU64::new(0);
    static HANDED_END: AtomicU64 = AtomicU64::new(0);

    /// Hand the Frames that cover the framebuffer to the caller: carve them
    /// from the video memory straight into the table `into` (lent for this
    /// call) at consecutive slots from `first_slot`, in address order.
    ///
    /// Retype allocates at an Untyped's watermark, so the memory below the
    /// framebuffer in its block is first carved into Untypeds that stay in
    /// this component's own table. The Frames themselves never are: the
    /// caller owns the framebuffer's memory from then on.
    pub fn frames(into: &KeyTableKey, first_slot: u32) -> Result<u32, Exception> {
        let init = init();
        let _turn = TURNSTILE.enter(init);
        let display = configured()?;
        let (start, end) = page_range(display.address, u64::from(display.bytes))
            .ok_or(Exception::OutOfVideoMemory)?;
        let video = init.video();
        let mut blocks = [(0, 0); MAX_VIDEO_BLOCKS];
        for (slot, block) in blocks.iter_mut().zip(video) {
            *slot = (block.base, block.end());
        }
        if !covered_by(&blocks, start, end - start) {
            return Err(Exception::OutOfVideoMemory);
        }
        if start < HANDED_END.load(Ordering::Acquire) && HANDED_BASE.load(Ordering::Acquire) < end {
            return Err(Exception::AlreadyHandedOut);
        }

        let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let mut slot = first_slot;
        let mut installed = 0_u32;
        for run in FramePlan::new(&blocks, start, end) {
            let (index, block) = video
                .iter()
                .enumerate()
                .find(|(_, block)| block.base <= run.address && run.address < block.end())
                .ok_or(Exception::OutOfVideoMemory)?;
            let untyped = UntypedKey::from_key(RawKey::from_wire(block.key));
            let offset = run.address - block.base;
            let mark = WATERMARKS[index].load(Ordering::Acquire);
            if mark.next_multiple_of(1 << run.size_bits) > offset {
                return Err(Exception::AlreadyHandedOut);
            }
            // Carve the memory up to the run into Untypeds of our own.
            let mut cursor = mark;
            for bits in skip_chunks(mark, offset) {
                let own_slot = u32::try_from(NEXT_OWN_SLOT.fetch_add(1, Ordering::AcqRel))
                    .map_err(|_overflow| Exception::OutOfVideoMemory)?;
                untyped
                    .retype(
                        ObjectType::UNTYPED,
                        bits,
                        0,
                        1,
                        &own,
                        own_slot,
                        Rights::all(),
                    )
                    .map_err(|_error| Exception::OutOfVideoMemory)?;
                cursor += 1 << bits;
                WATERMARKS[index].store(cursor, Ordering::Release);
            }
            untyped
                .retype_with_ceiling(
                    ObjectType::FRAME,
                    run.size_bits,
                    FRAME_ATTRIBUTE_CEILING,
                    run.count,
                    into,
                    slot,
                    FRAME_RIGHTS,
                )
                .map_err(|_error| Exception::InstallFailed { installed })?;
            WATERMARKS[index].store(offset + run.bytes(), Ordering::Release);
            if installed == 0 {
                HANDED_BASE.store(start, Ordering::Release);
                HANDED_END.store(end, Ordering::Release);
            }
            installed += run.count;
            slot = slot.saturating_add(run.count);
        }
        Ok(installed)
    }

    /// `display_v1`'s `not_present`: the display was never configured.
    fn not_present() -> DisplayException {
        DisplayException::NotPresent {
            generation: GENERATION.load(Ordering::Acquire),
        }
    }

    /// The `display_v1` format of the configured mode: 16-bit pixels are
    /// 5-6-5, the low byte first; of 32-bit ones, the firmware's BGR (0) has
    /// blue in the lowest byte of the pixel word, RGB (1) red
    /// (`libgraphics::PixelOrder`).
    fn format() -> Format {
        if PIXEL_BYTES.load(Ordering::Acquire) == 2 {
            Format::Rgb565
        } else if PIXEL_ORDER.load(Ordering::Acquire) == 0 {
            Format::Xrgb8888
        } else {
            Format::Xbgr8888
        }
    }

    /// The configured mode, as `get_mode` answers it.
    fn current_mode() -> Result<(u32, u32, u32, Format), DisplayException> {
        configured().map_err(|_not_configured| not_present())?;
        Ok((
            WIDTH.load(Ordering::Acquire),
            HEIGHT.load(Ordering::Acquire),
            PITCH.load(Ordering::Acquire),
            format(),
        ))
    }

    /// The display, as one `display_v1` output: a scanout output, upright
    /// only, with the modes its EDID names.
    struct Output;

    impl DisplayV1 for Output {
        fn enter(_badge: u64) {
            record_return_key();
        }

        fn describe(_badge: u64) -> Result<(u32, bool, u32, u32, u32), DisplayException> {
            configured().map_err(|_not_configured| not_present())?;
            Ok((
                GENERATION.load(Ordering::Acquire),
                true,
                WIDTH_MM.load(Ordering::Acquire),
                HEIGHT_MM.load(Ordering::Acquire),
                // The firmware's framebuffer turns no way but upright.
                0b0001,
            ))
        }

        fn modes(_badge: u64) -> Result<u32, DisplayException> {
            configured().map_err(|_not_configured| not_present())?;
            Ok(u32::try_from(modes().as_slice().len()).unwrap_or(0))
        }

        fn mode(_badge: u64, index: u32) -> Result<(u32, u32, Format, u32), DisplayException> {
            configured().map_err(|_not_configured| not_present())?;
            let mode = usize::try_from(index)
                .ok()
                .and_then(|at| modes().as_slice().get(at))
                .ok_or(DisplayException::NoMode { index })?;
            Ok((mode.width, mode.height, format(), mode.refresh_mhz))
        }

        fn get_mode(_badge: u64) -> Result<(u32, u32, u32, Format), DisplayException> {
            current_mode()
        }

        fn set_mode(_badge: u64, index: u32) -> Result<(u32, u32, u32, Format), DisplayException> {
            let init = init();
            let _turn = TURNSTILE.enter(init);
            configured().map_err(|_not_configured| not_present())?;
            let mode = usize::try_from(index)
                .ok()
                .and_then(|at| modes().as_slice().get(at))
                .copied()
                .ok_or(DisplayException::NoMode { index })?;
            if (mode.width, mode.height)
                == (
                    WIDTH.load(Ordering::Acquire),
                    HEIGHT.load(Ordering::Acquire),
                )
            {
                return current_mode();
            }
            // A new mode is new memory, and what is handed out stays handed out.
            if HANDED_END.load(Ordering::Acquire) != 0 {
                return Err(DisplayException::AlreadyHandedOut);
            }
            configure_holding_turn(&mut Trace::new(init), init, mode.width, mode.height, DEPTH)
                .map_err(|_error| DisplayException::Failed)?;
            current_mode()
        }

        fn frames(
            _badge: u64,
            into: &libmeddle_runtime::KeyTableKey,
            first_slot: u32,
        ) -> Result<(u32, u32), DisplayException> {
            let installed = frames(into, first_slot).map_err(|error| match error {
                Exception::NotConfigured => not_present(),
                Exception::AlreadyHandedOut => DisplayException::AlreadyHandedOut,
                Exception::InstallFailed { installed } => {
                    DisplayException::InstallFailed { installed }
                }
                Exception::FirmwareError { .. } | Exception::OutOfVideoMemory => {
                    DisplayException::Failed
                }
            })?;
            // The Frames start at the page that holds the first pixel.
            let address = configured()
                .map_err(|_not_configured| not_present())?
                .address;
            let offset = u32::try_from(address % PAGE_BYTES).unwrap_or(0);
            Ok((installed, offset))
        }

        fn rotate(_badge: u64, rotation: Rotation) -> Result<(u32, u32), DisplayException> {
            let (width, height, ..) = current_mode()?;
            match rotation {
                Rotation::R0 => Ok((width, height)),
                Rotation::R90 | Rotation::R180 | Rotation::R270 => {
                    Err(DisplayException::Unsupported)
                }
            }
        }

        fn flush(
            _badge: u64,
            _x: u32,
            _y: u32,
            _width: u32,
            _height: u32,
            _screen: IoRec,
        ) -> Result<(), DisplayException> {
            // A scanout output shows its memory by itself.
            current_mode().map(|_mode| ())
        }
    }

    display::display_v1_export!(Output);

    /// The display's modes and image size from its EDID base block, or the
    /// one fallback mode of unknown size without a usable block.
    fn read_modes(trace: &mut Trace, init: &DriverInit) -> (Modes, Option<(u32, u32)>) {
        trace!(trace, "fb: reading the display's EDID");
        let mailbox = Client::new(Bindings::from_imports(init.imports()));
        let answer: Result<[u32; EDID_WORDS], _> = exchange(&mailbox, &edid_request(0));
        let block = match answer.map(|answer| read_edid(&answer)) {
            Ok(Ok(block)) => block,
            Ok(Err(no_edid)) => {
                trace!(trace, "fb: no EDID: {no_edid:?}");
                return (Modes::fallback(FALLBACK_MODE), None);
            }
            Err(error) => {
                trace!(trace, "fb: no EDID: the mailbox failed: {error:?}");
                return (Modes::fallback(FALLBACK_MODE), None);
            }
        };
        let edid = match Edid::new(&block) {
            Ok(edid) => edid,
            Err(invalid) => {
                trace!(
                    trace,
                    "fb: the EDID block is not valid ({invalid:?}); it starts {:02x?}",
                    &block[..8]
                );
                return (Modes::fallback(FALLBACK_MODE), None);
            }
        };
        trace!(
            trace,
            "fb: EDID {:?}; image size {:?} mm by its preferred timing, {:?} mm by its screen size bytes",
            edid.version(),
            edid.timing_size_mm(),
            edid.screen_size_mm()
        );
        let modes = edid.modes();
        if modes.as_slice().is_empty() {
            trace!(trace, "fb: the EDID names no mode");
            return (Modes::fallback(FALLBACK_MODE), edid.image_size_mm());
        }
        (modes, edid.image_size_mm())
    }

    /// Configure the first of the display's modes the firmware takes, and
    /// return its index.
    fn configure_first_mode(trace: &mut Trace, init: &DriverInit) -> Option<usize> {
        modes().as_slice().iter().position(|mode| {
            trace!(trace, "fb: configuring {}x{}", mode.width, mode.height);
            let configured = configure_holding_turn(trace, init, mode.width, mode.height, DEPTH);
            if let Err(error) = configured {
                trace!(
                    trace,
                    "fb: mode {}x{} refused: {error:?}", mode.width, mode.height
                );
            }
            configured.is_ok()
        })
    }

    /// Set the turnstile up, read the display's modes and configure the first
    /// one that works, then park: everything else happens in calls.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let mut trace = Trace::new(init);
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                u32::try_from(init.first_free_slot).unwrap_or(u32::MAX),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("fb: Retype failed: {:?}", error.code()));
        NEXT_OWN_SLOT.store(init.first_free_slot + 1, Ordering::Release);
        TURNSTILE.open(turns);

        let (display_modes, size_mm) = read_modes(&mut trace, init);
        // SAFETY: no call has entered yet: they wait for the ready advance
        // below, and nothing writes the modes after.
        unsafe {
            *MODES.0.get() = display_modes;
        }
        let (width_mm, height_mm) = size_mm.unwrap_or((0, 0));
        WIDTH_MM.store(width_mm, Ordering::Release);
        HEIGHT_MM.store(height_mm, Ordering::Release);
        match configure_first_mode(&mut trace, init) {
            Some(index) => trace!(
                trace,
                "fb: {} modes, mode {index} configured: {}x{}, pitch {}, {width_mm}x{height_mm} mm",
                modes().as_slice().len(),
                WIDTH.load(Ordering::Acquire),
                HEIGHT.load(Ordering::Acquire),
                PITCH.load(Ordering::Acquire)
            ),
            None => trace!(trace, "fb: no mode could be configured"),
        }

        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("fb: ready advance failed: {:?}", error.code()));
        trace!(trace, "fb: ready");
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
