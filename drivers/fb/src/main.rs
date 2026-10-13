// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]

//! The framebuffer component: the Raspberry Pi's display behind `fb_v1` (see
//! `interfaces/fb_v1.if`).
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

use fb::fb_v1::{__runtime::KeyTableKey, Exception, FbV1, PixelOrder};

/// `firmware_error` code when the mailbox cannot be reached or answers
/// something unexpected.
const MAILBOX_FAILED: u32 = u32::MAX;

/// The display.
struct Display;

impl FbV1 for Display {
    #[cfg(target_os = "vesper")]
    fn enter() {
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
        core::sync::atomic::{AtomicU64, Ordering},
        fb::fb_v1::PixelOrder,
        fb_impl::{
            ATTRIBUTE_WORDS, CONFIGURE_WORDS, FramePlan, attributes_request, configure_request,
            covered_by, page_range, physical_address, read_allocation, read_attributes,
            skip_chunks,
        },
        libdevice_driver::{
            INIT_VA, MAX_VIDEO_BLOCKS, NO_BUS_ADDRESS, Turnstile, driver_compatible, driver_init,
        },
        libmeddle_runtime::ExclusiveStacks,
        libobject::{
            EventCountKey, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        mailbox_client::mailbox_v1::{Bindings, Client, Error, Exception as MailboxException},
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2708-fb");

    /// The configured display: zero until `configure` succeeds, then its
    /// physical address (the size and pixel order follow it).
    static ADDRESS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);
    static PIXEL_ORDER: AtomicU64 = AtomicU64::new(0);

    /// Admits one caller at a time: the mailbox stack it calls on is single-user.
    static TURNSTILE: Turnstile = Turnstile::new();

    fn init() -> &'static libdevice_driver::DriverInit {
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
        let mailbox = Client::new(
            Bindings::from_imports(init.imports()),
            // SAFETY: the turnstile admits one caller at a time, so this
            // Thread alone uses the stack reserved for the mailbox calls.
            unsafe { ExclusiveStacks::new() },
        );

        let answer: [u32; CONFIGURE_WORDS] =
            exchange(&mailbox, &configure_request(width, height, depth))?;
        let allocation = read_allocation(&answer).map_err(firmware_error)?;
        let answer: [u32; ATTRIBUTE_WORDS] = exchange(&mailbox, &attributes_request())?;
        let attributes = read_attributes(&answer).map_err(firmware_error)?;
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
        if !covered_by(&blocks, physical, u64::from(allocation.bytes)) {
            return Err(Exception::OutOfVideoMemory);
        }
        BYTES.store(u64::from(allocation.bytes), Ordering::Release);
        PIXEL_ORDER.store(u64::from(attributes.pixel_order), Ordering::Release);
        ADDRESS.store(physical, Ordering::Release);
        Ok(attributes.pitch)
    }

    /// The configured display, or `not_configured`.
    pub fn configured() -> Result<Configured, Exception> {
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
                .retype(
                    ObjectType::FRAME,
                    run.size_bits,
                    0,
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

    /// Set the turnstile up, then park: everything else happens in calls.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
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
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("fb: ready advance failed: {:?}", error.code()));
        let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let _bits = park.wait(NotificationKey::WAIT_INFINITE);
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
