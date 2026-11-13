// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! EL0 driver component for a standard SD host controller (SDHCI), serving
//! `block_v1` (see `interfaces/block_v1.if`).
//!
//! Kickstart loads it for a `brcm,bcm2711-emmc2` node (the Raspberry Pi 4's
//! card slot), a `brcm,bcm2712-sdhci` node (the Raspberry Pi 5's) and, in a
//! `qemu` build, a `brcm,bcm2835-sdhci` node: QEMU puts its card on that
//! controller, which on the boards serves the Wi-Fi chip. It imports
//! `mailbox_v1` through the firmware node (see `interfaces/imports.if`) for
//! the EMMC2 clock, when neither the controller's capabilities nor a fixed
//! clock in the device tree give the base clock.
//!
//! Its one Thread, which receives the controller's interrupt, maps the
//! registers, brings the card up and then serves the request queue of every
//! client's io-channel (`libblock-channel`): it sleeps on a doorbell
//! `EventCount` the `submitted` export advances, and on each wakeup serves
//! every waiting request of every client in order, and completes it back.
//! Each command and data stage ends in an interrupt the Thread waits for, at
//! most `sdhci::EVENT_MS` before the command fails.
//! Blocks move by ADMA2 through bounce pages of the driver's own memory where
//! the controller has ADMA2 and the memory has a 32-bit bus address, and
//! through the buffer data port otherwise; a multiple-block request moves in
//! one command per [`BOUNCE_PAGES`] pages. A card that failed to come up
//! answers every request with `NoMedium` or `DeviceFailed`.

use block::block_v1::{BlockV1, Exception};

/// The SD card slot.
struct SdCard;

impl BlockV1 for SdCard {
    #[cfg(target_os = "vesper")]
    fn enter(_badge: u64) {
        component::record_return_key();
    }

    fn blocks() -> Result<u64, Exception> {
        blocks()
    }

    fn submitted() {
        submitted();
    }
}

block::block_v1_export!(SdCard);

#[cfg(target_os = "vesper")]
use component::{blocks, submitted};

#[cfg(not(target_os = "vesper"))]
fn blocks() -> Result<u64, Exception> {
    Err(Exception::NoMedium)
}

#[cfg(not(target_os = "vesper"))]
fn submitted() {}

/// Pages of the driver's memory a DMA transfer bounces through: one command
/// moves at most this many pages.
pub const BOUNCE_PAGES: usize = 16;

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::{BOUNCE_PAGES, Exception},
        core::sync::atomic::{AtomicU32, AtomicU64, Ordering},
        libblock_channel::{BLOCK_BYTES, BlockChannel, Header, Op, Refusal, Status, admit, shape},
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, ExportedArea, INIT_VA, MAX_DEVICE_PAGES, NO_BUS_ADDRESS,
            device_address, driver_compatible, driver_init, map_device_pages,
        },
        libio_channel::{DataArea, IoRec, NoEvent, Packet},
        libmeddle_runtime::PAGE_BYTES,
        libobject::{
            CapError, EventCountKey, FrameAttribute, FrameKey, IrqHandlerKey, KeyTableKey,
            NotificationKey, ObjectType, RawKey, Rights, UntypedKey, export::init_return_key,
            thread::ThreadReturnKey,
        },
        libsd_card::{
            Card, CardError, Command, READ_MULTIPLE_BLOCK, READ_SINGLE_BLOCK, WRITE_BLOCK,
            WRITE_MULTIPLE_BLOCK,
        },
        libuser::semihosting as semi,
        mailbox_client::mailbox_v1::{Bindings, Client, Clock},
        sdhci::{Bcm2712Config, Mover, RegisterBlock, Sdhci, adma2_descriptor, configure_bcm2712},
    };

    libuser::entry!(main);

    #[cfg(feature = "qemu")]
    driver_compatible!(
        "brcm,bcm2711-emmc2",
        "brcm,bcm2712-sdhci",
        "brcm,bcm2835-sdhci"
    );
    #[cfg(not(feature = "qemu"))]
    driver_compatible!("brcm,bcm2711-emmc2", "brcm,bcm2712-sdhci");

    /// `DriverInit::compatible_index` of `brcm,bcm2712-sdhci`, whose node's
    /// second register range is its configuration block.
    const BCM2712: u64 = 1;

    /// Where the DMA pages map: the descriptor table, then the bounce pages.
    const DMA_VA: u64 = DEVICE_WINDOW_VA + MAX_DEVICE_PAGES as u64 * PAGE_BYTES;

    /// Bytes of a page.
    const PAGE: u32 = 4096;
    /// Blocks one command moves at most: the bounce pages' worth.
    const MOST_BLOCKS: u64 = BOUNCE_PAGES as u64 * PAGE as u64 / BLOCK_BYTES;

    /// `FAILURE` of a slot without a card.
    const NO_CARD: u32 = u32::MAX;
    /// `FAILURE` of a controller that did not come up: no base clock, or its
    /// reset did not finish.
    const NO_CONTROLLER: u32 = u32::MAX - 1;

    /// Blocks of the card, once it is up.
    static BLOCKS: AtomicU64 = AtomicU64::new(0);
    /// Why the card is not up: zero when it is, [`NO_CARD`],
    /// [`NO_CONTROLLER`] or the step it failed at.
    static FAILURE: AtomicU32 = AtomicU32::new(0);
    /// The doorbell `submitted` rings, as its wire key.
    static DOORBELL: AtomicU64 = AtomicU64::new(0);

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

    /// Wait until the Thread brought the card up (or failed to).
    fn wait_ready() {
        EventCountKey::from_key(RawKey::from_wire(init().event_count))
            .await_ge(1, EventCountKey::WAIT_INFINITE)
            .unwrap_or_else(|error| panic!("sdhci: ready wait failed: {:?}", error.code()));
    }

    /// `block_v1.blocks`.
    pub fn blocks() -> Result<u64, Exception> {
        wait_ready();
        match FAILURE.load(Ordering::Acquire) {
            0 => Ok(BLOCKS.load(Ordering::Acquire)),
            NO_CARD => Err(Exception::NoMedium),
            NO_CONTROLLER => Err(Exception::DeviceFailed { step: 0 }),
            step => Err(Exception::DeviceFailed { step }),
        }
    }

    /// `block_v1.submitted`: ring the doorbell.
    pub fn submitted() {
        wait_ready();
        EventCountKey::from_key(RawKey::from_wire(DOORBELL.load(Ordering::Acquire)))
            .advance(1)
            .unwrap_or_else(|error| panic!("sdhci: doorbell advance failed: {:?}", error.code()));
    }

    fn now_ms() -> u64 {
        libuser::clock::now_ns() / 1_000_000
    }

    /// The base clock when the capabilities do not give it: the node's fixed
    /// clock, or the firmware's EMMC2 clock.
    fn fallback_hz(init: &DriverInit) -> Option<u32> {
        if init.clock_hz != 0 {
            return u32::try_from(init.clock_hz).ok();
        }
        let mailbox = Client::new(Bindings::from_imports(init.imports()));
        mailbox.get_clock_rate(Clock::Emmc2).ok()
    }

    /// Return once the controller's interrupt line was raised, or after the
    /// milliseconds given without it: acknowledge its handlers, which unmasks
    /// the line, and wait for the Notification with that timeout. A node
    /// without interrupts is polled.
    fn wait_interrupt(init: &'static DriverInit) -> impl FnMut(u64) {
        let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
        move |milliseconds| {
            let handlers = init.interrupts();
            if handlers.is_empty() {
                core::hint::spin_loop();
                return;
            }
            for interrupt in handlers {
                IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
                    .ack()
                    .unwrap_or_else(|error| {
                        panic!("sdhci: interrupt Ack failed: {:?}", error.code())
                    });
            }
            // A timed-out wait consumed nothing: the caller reads the status
            // and gives up once its own deadline passes.
            let timeout_ns = milliseconds.saturating_mul(1_000_000).max(1);
            match interrupts.await_bits(timeout_ns) {
                Ok(_) | Err(CapError::TimedOut { .. }) => {}
                Err(error) => panic!("sdhci: interrupt wait failed: {:?}", error.code()),
            }
        }
    }

    /// The pages ADMA2 moves blocks through: a descriptor table and the bounce
    /// pages, frames of the driver's own memory mapped cacheable and kept
    /// coherent with `Frame CleanData` and `InvalidateData`.
    struct Dma {
        /// The frames, as wire keys
        descriptors: u64,
        descriptors_bus: u32,
        bounce: [u64; BOUNCE_PAGES],
        bounce_bus: [u32; BOUNCE_PAGES],
    }

    /// Retype a page of the driver's memory into `slot` and map it at `va`;
    /// the frame's wire key and the bus address of its first byte, `None`
    /// when the controller cannot reach it with a 32-bit address.
    fn dma_page(init: &DriverInit, slot: u32, va: u64) -> Option<(u64, u32)> {
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let frame = FrameKey::from_key(
            UntypedKey::from_key(RawKey::from_wire(init.memory))
                .retype(
                    ObjectType::FRAME,
                    12,
                    0,
                    1,
                    &table,
                    slot,
                    Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
                )
                .unwrap_or_else(|error| {
                    panic!("sdhci: DMA frame Retype failed: {:?}", error.code())
                }),
        );
        let (paddr, _size) = frame
            .get_extent()
            .unwrap_or_else(|error| panic!("sdhci: DMA frame extent: {:?}", error.code()));
        frame
            .map(
                RawKey::from_wire(init.address_space),
                va,
                Rights(Rights::READ | Rights::WRITE),
                FrameAttribute::Default,
            )
            .unwrap_or_else(|error| panic!("sdhci: DMA frame Map failed: {:?}", error.code()));
        let bus = init
            .memory_bus
            .checked_add(paddr.checked_sub(init.memory_paddr)?)
            .and_then(|bus| u32::try_from(bus).ok())?;
        Some((frame.to_wire(), bus))
    }

    impl Dma {
        /// The DMA pages, retyped into the slots from `first_slot` on; `None`
        /// when the controller's bus cannot reach the driver's memory with
        /// 32-bit addresses.
        fn new(init: &DriverInit, first_slot: u32) -> Option<Self> {
            if init.memory_bus == NO_BUS_ADDRESS {
                return None;
            }
            let (descriptors, descriptors_bus) = dma_page(init, first_slot, DMA_VA)?;
            let mut bounce = [descriptors; BOUNCE_PAGES];
            let mut bounce_bus = [0; BOUNCE_PAGES];
            for (index, (frame, bus)) in bounce.iter_mut().zip(&mut bounce_bus).enumerate() {
                let offset = u32::try_from(index + 1).ok()?;
                (*frame, *bus) = dma_page(
                    init,
                    first_slot + offset,
                    DMA_VA + u64::from(offset) * PAGE_BYTES,
                )?;
            }
            Some(Self {
                descriptors,
                descriptors_bus,
                bounce,
                bounce_bus,
            })
        }

        /// Where bounce page `index` is mapped.
        const fn bounce_va(index: usize) -> u64 {
            DMA_VA + (index as u64 + 1) * PAGE_BYTES
        }

        /// Clean (`write_back`) or invalidate bytes `0..bytes` of `frame`.
        fn maintain(frame: u64, write_back: bool, bytes: u32) {
            let frame = FrameKey::from_key(RawKey::from_wire(frame));
            let result = if write_back {
                frame.clean_data(0, u64::from(bytes))
            } else {
                frame.invalidate_data(0, u64::from(bytes))
            };
            result.unwrap_or_else(|error| {
                panic!("sdhci: cache maintenance failed: {:?}", error.code())
            });
        }

        /// Write the descriptor table for `bytes` from the first bounce page
        /// on and make it visible to the controller.
        fn describe(&self, bytes: u32) {
            let pages = bytes.div_ceil(PAGE) as usize;
            for (index, bus) in self.bounce_bus.iter().take(pages).enumerate() {
                let start = u32::try_from(index).unwrap_or(0) * PAGE;
                let descriptor =
                    adma2_descriptor(*bus, (bytes - start).min(PAGE), index + 1 == pages);
                // SAFETY: the descriptor table page is mapped read/write at
                // `DMA_VA` for the driver's lifetime; `pages` is at most
                // `BOUNCE_PAGES`, whose descriptors fit the page.
                unsafe {
                    (DMA_VA as *mut u64).add(index).write_volatile(descriptor);
                }
            }
            Self::maintain(
                self.descriptors,
                true,
                u32::try_from(pages * 8).unwrap_or(PAGE),
            );
        }

        /// Copy `bytes` between the bounce pages and `buffer`, page by page:
        /// into the pages when `to_bounce`, out of them otherwise.
        fn copy(buffer: u64, bytes: u32, to_bounce: bool) {
            for index in 0..bytes.div_ceil(PAGE) as usize {
                let start = u32::try_from(index).unwrap_or(0) * PAGE;
                let length = (bytes - start).min(PAGE) as usize;
                let bounce = Self::bounce_va(index) as *mut u8;
                let client = (buffer + u64::from(start)) as *mut u8;
                let (from, to) = if to_bounce {
                    (client, bounce)
                } else {
                    (bounce, client)
                };
                // SAFETY: `shape` checked the client's buffer lies in its
                // mapped data area; the bounce page is mapped for the
                // driver's lifetime; they never overlap.
                unsafe {
                    core::ptr::copy_nonoverlapping(from, to, length);
                }
            }
        }

        /// Make bounce bytes `0..bytes` visible to the controller
        /// (`write_back`), or drop what the cache holds of them.
        fn maintain_bounce(&self, write_back: bool, bytes: u32) {
            for (index, frame) in self
                .bounce
                .iter()
                .take(bytes.div_ceil(PAGE) as usize)
                .enumerate()
            {
                let start = u32::try_from(index).unwrap_or(0) * PAGE;
                Self::maintain(*frame, write_back, (bytes - start).min(PAGE));
            }
        }
    }

    /// The command moving `blocks` blocks for `op`.
    const fn command(op: Op, blocks: u16) -> Command {
        match (op, blocks) {
            (Op::Read, 1) => READ_SINGLE_BLOCK,
            (Op::Read, _) => READ_MULTIPLE_BLOCK,
            (Op::Write, 1) => WRITE_BLOCK,
            (Op::Write, _) => WRITE_MULTIPLE_BLOCK,
        }
    }

    /// Move the blocks of `buffers` from `block` on, at most
    /// [`BOUNCE_PAGES`] pages per command.
    fn transfer(
        host: &mut Sdhci<'_, impl FnMut(u64)>,
        dma: Option<&Dma>,
        card: &Card,
        op: Op,
        mut block: u64,
        buffers: &[IoRec],
    ) -> Status {
        for buffer in buffers {
            let total = buffer.len / BLOCK_BYTES;
            let mut done = 0;
            while done < total {
                let blocks = (total - done).min(MOST_BLOCKS);
                let Some(argument) = card.argument(block) else {
                    return Status::OutOfRange;
                };
                let address = buffer.base + done * BLOCK_BYTES;
                // At most the bounce pages' worth: both fit.
                let bytes = u32::try_from(blocks * BLOCK_BYTES).unwrap_or(u32::MAX);
                let count = u16::try_from(blocks).unwrap_or(u16::MAX);
                let moved = match dma {
                    Some(dma) => {
                        dma.describe(bytes);
                        if op == Op::Write {
                            Dma::copy(address, bytes, true);
                            dma.maintain_bounce(true, bytes);
                        }
                        let moved = host.transfer(
                            command(op, count),
                            argument,
                            count,
                            Mover::Dma {
                                descriptors: dma.descriptors_bus,
                            },
                        );
                        if op == Op::Read && moved.is_ok() {
                            dma.maintain_bounce(false, bytes);
                            Dma::copy(address, bytes, false);
                        }
                        moved
                    }
                    None => {
                        let mut word_address = address as *mut u32;
                        let mut port = |word: Option<u32>| {
                            // SAFETY: `shape` checked the buffer lies in the
                            // mapped data area, word aligned, in whole blocks;
                            // the client gave it to the driver until it comes
                            // back.
                            unsafe {
                                let value = match word {
                                    Some(word) => {
                                        word_address.write_volatile(word);
                                        word
                                    }
                                    None => word_address.read_volatile(),
                                };
                                word_address = word_address.add(1);
                                value
                            }
                        };
                        host.transfer(command(op, count), argument, count, Mover::Port(&mut port))
                    }
                };
                let settled = moved.and_then(|()| {
                    if op == Op::Write {
                        card.wait_ready(host)
                    } else {
                        Ok(())
                    }
                });
                if settled.is_err() {
                    return Status::DeviceFailed;
                }
                block += blocks;
                done += blocks;
            }
        }
        Status::Done
    }

    /// The card, or why there is none.
    type CardState = Result<Card, CardError>;

    /// Serve one request of the channel whose data area is `area`.
    fn serve(
        host: Option<&mut Sdhci<'_, impl FnMut(u64)>>,
        dma: Option<&Dma>,
        card: &CardState,
        area: &DataArea,
        packet: &Packet,
    ) {
        let (header_address, blocks) = match shape(packet, area) {
            Ok(shaped) => shaped,
            Err(Refusal::Untouched) => return,
            Err(Refusal::Status(status)) => {
                // SAFETY: `shape` checked the header record lies in the mapped
                // data area, 8-byte aligned and a header long.
                unsafe {
                    Header::write_status(packet.recs()[0].base, status);
                }
                return;
            }
        };
        // SAFETY: as above.
        let header = unsafe { Header::read(header_address) };
        let status = match (host, card) {
            (None, _) | (_, Err(CardError::Failed { .. })) => Status::DeviceFailed,
            (_, Err(CardError::NoCard)) => Status::NoMedium,
            (Some(host), Ok(card)) => match admit(&header, blocks, card.blocks) {
                Ok(op) => transfer(host, dma, card, op, header.block, &packet.recs()[1..]),
                Err(status) => status,
            },
        };
        // SAFETY: as above.
        unsafe {
            Header::write_status(header_address, status);
        }
    }

    /// Serve every request waiting on `exported`'s channel.
    fn drain(
        mut host: Option<&mut Sdhci<'_, impl FnMut(u64)>>,
        dma: Option<&Dma>,
        card: &CardState,
        exported: &ExportedArea,
    ) {
        // The channel's data pages follow its control page.
        let data = DataArea {
            base: exported.channel_base + PAGE_BYTES,
            len: block::block_v1::CHANNEL_PAGES * PAGE_BYTES,
        };
        // SAFETY: Kickstart mapped the channel's control page and data pages
        // read/write for the driver's lifetime.
        let channel = unsafe { BlockChannel::at(exported.channel_base) };
        let (mut requests, mut completions) = channel.requests.server(
            NoEvent,
            EventCountKey::from_key(RawKey::from_wire(exported.channel_event)),
        );
        loop {
            let packet = match requests.get() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                // The client garbled its FIFO; what it published is gone.
                Err(_corrupt) => continue,
            };
            serve(host.as_deref_mut(), dma, card, &data, &packet);
            // A client with more requests out than the FIFO holds loses the
            // extra ones.
            let _lost = completions.put(&packet);
        }
        completions
            .signal()
            .unwrap_or_else(|error| panic!("sdhci: completion advance failed: {:?}", error.code()));
    }

    /// The registers of the node's register range `index`, mapped by
    /// `map_device_pages`.
    fn range(init: &DriverInit, index: usize) -> Option<(u64, u64)> {
        let start = init.regions().get(index)?.start;
        Some((start, device_address(init, start)?))
    }

    /// Bring the controller and the card up; `None` when the controller's
    /// reset does not finish.
    fn bring_up(host: &mut Sdhci<'_, impl FnMut(u64)>, config: Option<u64>) -> Option<CardState> {
        host.reset().ok()?;
        if let Some(config) = config {
            // SAFETY: `map_device_pages` mapped the node's configuration range
            // read/write as device memory at `config` for the driver's
            // lifetime, and only this Thread touches it.
            let mut registers = unsafe { Bcm2712Config::new_mmio(config as *mut Bcm2712Config) };
            configure_bcm2712(&mut registers);
        }
        Some(Card::bring_up(host))
    }

    /// Map the registers, bring the card up, say the exports can be served,
    /// then serve requests whenever the doorbell rings.
    #[cfg_attr(
        not(feature = "qemu"),
        expect(
            unused_variables,
            reason = "the bring-up report prints over semihosting only"
        )
    )]
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("sdhci: mapping registers failed: {:?}", error.code()));
        let slot = u32::try_from(slot).unwrap_or(u32::MAX);
        let (registers_paddr, registers) = range(init, 0).expect("sdhci: registers mapped");
        let config = (init.compatible_index == BCM2712).then(|| {
            range(init, 1)
                .expect("sdhci: configuration registers mapped")
                .1
        });
        // SAFETY: `map_device_pages` mapped the node's register range
        // read/write as device memory at `registers` for the driver's
        // lifetime, and only this Thread touches it.
        let registers = unsafe { RegisterBlock::new_mmio(registers as *mut RegisterBlock) };
        let mut host = Sdhci::new(
            registers,
            || fallback_hz(init),
            wait_interrupt(init),
            now_ms,
        );

        let mut dma = None;
        let card = match host.as_mut() {
            None => {
                FAILURE.store(NO_CONTROLLER, Ordering::Release);
                semi::println!("sdhci: {:#x}: no base clock", registers_paddr);
                Err(CardError::NoCard)
            }
            Some(host) => {
                dma = if host.has_adma2() {
                    Dma::new(init, slot + 1)
                } else {
                    None
                };
                let card = bring_up(host, config);
                let moves = if dma.is_some() {
                    "ADMA2"
                } else {
                    "the data port"
                };
                match &card {
                    None => {
                        FAILURE.store(NO_CONTROLLER, Ordering::Release);
                        semi::println!("sdhci: {:#x}: the reset did not finish", registers_paddr);
                    }
                    Some(Ok(card)) => {
                        BLOCKS.store(card.blocks, Ordering::Release);
                        semi::println!(
                            "sdhci: {:#x} up, version {}, base clock {} Hz, blocks by {}: \
                             card {:#06x}, {} blocks, {} capacity",
                            registers_paddr,
                            host.version(),
                            host.base_hz(),
                            moves,
                            card.rca,
                            card.blocks,
                            if card.high_capacity {
                                "high"
                            } else {
                                "standard"
                            }
                        );
                    }
                    Some(Err(CardError::NoCard)) => {
                        FAILURE.store(NO_CARD, Ordering::Release);
                        semi::println!("sdhci: {:#x} up, no card", registers_paddr);
                    }
                    Some(Err(failure @ CardError::Failed { step, .. })) => {
                        FAILURE.store(*step as u32, Ordering::Release);
                        semi::println!("sdhci: card bring-up failed: {:?}", failure);
                    }
                }
                // A controller that did not come up serves no requests.
                card.unwrap_or(Err(CardError::NoCard))
            }
        };
        if FAILURE.load(Ordering::Acquire) == NO_CONTROLLER {
            host = None;
        }

        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let doorbell = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                slot,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("sdhci: doorbell Retype failed: {:?}", error.code()));
        DOORBELL.store(doorbell.to_wire(), Ordering::Release);
        // Say the exports can be served.
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("sdhci: ready advance failed: {:?}", error.code()));

        let doorbell = EventCountKey::from_key(doorbell);
        let mut seen = 0;
        loop {
            seen = doorbell
                .await_ge(seen + 1, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("sdhci: doorbell wait failed: {:?}", error.code()));
            for exported in init.areas().iter().filter(|area| area.channel_base != 0) {
                drain(host.as_mut(), dma.as_ref(), &card, exported);
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
