// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! EL0 driver component for the BCM2835 SDHOST controller, the Raspberry Pi's
//! SD card slot, serving `block_v1` (see `interfaces/block_v1.if`).
//!
//! Kickstart loads it for the `brcm,bcm2835-sdhost` node and wires its imports
//! (see `interfaces/imports.if`): `mailbox_v1` through the firmware node, since
//! the card clock divides the core clock, whose rate the firmware reports; and
//! `gpio_v1`, through which it sets the pulls of its node's pin group and
//! routes its pins (on the Raspberry Pi 3, GPIO 48 to 53 on ALT0) to the
//! controller.
//!
//! Its one Thread maps the registers, routes the pins, brings the card up and then serves the
//! request queue of every client's io-channel (`libblock-channel`): it sleeps
//! on a doorbell `EventCount` the `submitted` export advances, and on each
//! wakeup serves every waiting request of every client in order, moving each
//! block through the data register, and completes it back. A card that failed
//! to come up answers every request with `NoMedium` or `DeviceFailed`.

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

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::Exception,
        core::sync::atomic::{AtomicU32, AtomicU64, Ordering},
        gpio_client::gpio_v1,
        libblock_channel::{BLOCK_BYTES, BlockChannel, Header, Op, Refusal, Status, admit, shape},
        libdevice_driver::{
            DriverInit, ExportedArea, INIT_VA, NO_PULL, device_address, driver_compatible,
            driver_init, map_device_pages,
        },
        libio_channel::{DataArea, IoRec, NoEvent, Packet},
        libmeddle_runtime::PAGE_BYTES,
        libobject::{
            EventCountKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
            export::init_return_key, thread::ThreadReturnKey,
        },
        libuser::semihosting as semi,
        mailbox_client::mailbox_v1::{Bindings, Client, Clock},
        sdhost_bcm2835::{
            RegisterBlock, Sdhost,
            card::{Bus, Card, CardError, READ_SINGLE_BLOCK, WRITE_BLOCK},
        },
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-sdhost");

    /// The core clock Linux assumes when the firmware does not say.
    const DEFAULT_CORE_HZ: u32 = 250_000_000;

    /// `FAILURE` of a slot without a card.
    const NO_CARD: u32 = u32::MAX;

    /// Blocks of the card, once it is up.
    static BLOCKS: AtomicU64 = AtomicU64::new(0);
    /// Why the card is not up: zero when it is, [`NO_CARD`], or the step it
    /// failed at.
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
            .unwrap_or_else(|error| panic!("sdhost: ready wait failed: {:?}", error.code()));
    }

    /// `block_v1.blocks`.
    pub fn blocks() -> Result<u64, Exception> {
        wait_ready();
        match FAILURE.load(Ordering::Acquire) {
            0 => Ok(BLOCKS.load(Ordering::Acquire)),
            NO_CARD => Err(Exception::NoMedium),
            step => Err(Exception::DeviceFailed { step }),
        }
    }

    /// `block_v1.submitted`: ring the doorbell.
    pub fn submitted() {
        wait_ready();
        EventCountKey::from_key(RawKey::from_wire(DOORBELL.load(Ordering::Acquire)))
            .advance(1)
            .unwrap_or_else(|error| panic!("sdhost: doorbell advance failed: {:?}", error.code()));
    }

    fn now_ms() -> u64 {
        libuser::clock::now_ns() / 1_000_000
    }

    /// The core clock's rate, from the firmware.
    fn core_hz(init: &DriverInit) -> u32 {
        let mailbox = Client::new(Bindings::from_imports(init.imports()));
        mailbox.get_clock_rate(Clock::Core).unwrap_or_else(|_| {
            semi::println!("sdhost: no core clock rate from the firmware, assuming 250 MHz");
            DEFAULT_CORE_HZ
        })
    }

    /// Give the pins of the node's pin group their pulls, if the group sets
    /// them, and route them to the controller. The firmware
    /// already does on a Raspberry Pi 3; QEMU keeps the card on the Arasan
    /// controller until they switch.
    fn route_pins(init: &DriverInit) {
        let gpio = gpio_v1::Client::new(gpio_v1::Bindings::from_imports(init.imports()));
        for setting in init.pins() {
            // The pull settles before the pin connects to the controller.
            let pulled = setting.pull == NO_PULL
                || gpio_v1::PinPull::from_code(setting.pull)
                    .is_some_and(|pull| gpio.set_pull(setting.pin, pull).is_ok());
            let routed = pulled
                && gpio_v1::PinFunction::from_select_code(setting.function)
                    .is_some_and(|function| gpio.set_function(setting.pin, function).is_ok());
            if !routed {
                semi::println!(
                    "sdhost: routing GPIO {} failed, the card may be unreachable",
                    setting.pin
                );
            }
        }
    }

    /// Move the `blocks` of `buffers` from `block` on, one command per block.
    fn transfer(
        host: &mut Sdhost<'_>,
        card: &Card,
        op: Op,
        mut block: u64,
        buffers: &[IoRec],
    ) -> Status {
        for buffer in buffers {
            for index in 0..buffer.len / BLOCK_BYTES {
                let Some(argument) = card.argument(block) else {
                    return Status::OutOfRange;
                };
                let mut address = (buffer.base + index * BLOCK_BYTES) as *mut u32;
                let moved = match op {
                    Op::Read => host.command(READ_SINGLE_BLOCK, argument).and_then(|_| {
                        host.read_block(|word| {
                            // SAFETY: `shape` checked the buffer lies in the
                            // mapped data area, word aligned, in whole blocks;
                            // the client gave it to the driver until it comes
                            // back.
                            unsafe {
                                address.write_volatile(word);
                                address = address.add(1);
                            }
                        })
                    }),
                    Op::Write => host
                        .command(WRITE_BLOCK, argument)
                        .and_then(|_| {
                            host.write_block(|| {
                                // SAFETY: as for a read.
                                unsafe {
                                    let word = address.read_volatile();
                                    address = address.add(1);
                                    word
                                }
                            })
                        })
                        .and_then(|()| card.wait_ready(host)),
                };
                if moved.is_err() {
                    return Status::DeviceFailed;
                }
                block += 1;
            }
        }
        Status::Done
    }

    /// Serve one request of the channel whose data area is `area`.
    fn serve(
        host: &mut Sdhost<'_>,
        card: &Result<Card, CardError>,
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
        let status = match card {
            Err(CardError::NoCard) => Status::NoMedium,
            Err(CardError::Failed { .. }) => Status::DeviceFailed,
            Ok(card) => match admit(&header, blocks, card.blocks) {
                Ok(op) => transfer(host, card, op, header.block, &packet.recs()[1..]),
                Err(status) => status,
            },
        };
        // SAFETY: as above.
        unsafe {
            Header::write_status(header_address, status);
        }
    }

    /// Serve every request waiting on `exported`'s channel.
    fn drain(host: &mut Sdhost<'_>, card: &Result<Card, CardError>, exported: &ExportedArea) {
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
            serve(host, card, &data, &packet);
            // A client with more requests out than the FIFO holds loses the
            // extra ones.
            let _lost = completions.put(&packet);
        }
        completions.signal().unwrap_or_else(|error| {
            panic!("sdhost: completion advance failed: {:?}", error.code())
        });
    }

    /// Map the registers, bring the card up, say the exports can be served,
    /// then serve requests whenever the doorbell rings.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("sdhost: mapping registers failed: {:?}", error.code()));
        let registers_paddr = init.regions()[0].start;
        let registers = device_address(init, registers_paddr).expect("sdhost: registers mapped");
        // SAFETY: `map_device_pages` mapped the node's register page read/write
        // as device memory at `registers` for the driver's lifetime, and only
        // this Thread touches it.
        let registers = unsafe { RegisterBlock::new_mmio(registers as *mut RegisterBlock) };
        route_pins(init);
        let core_hz = core_hz(init);
        let mut host = Sdhost::new(registers, core_hz, now_ms);
        host.reset();
        let card = Card::bring_up(&mut host);
        match &card {
            Ok(card) => {
                BLOCKS.store(card.blocks, Ordering::Release);
                semi::println!(
                    "sdhost: {:#x} up, core clock {} Hz: card {:#06x}, {} blocks, {} capacity",
                    registers_paddr,
                    core_hz,
                    card.rca,
                    card.blocks,
                    if card.high_capacity {
                        "high"
                    } else {
                        "standard"
                    }
                );
            }
            Err(CardError::NoCard) => {
                FAILURE.store(NO_CARD, Ordering::Release);
                semi::println!("sdhost: {:#x} up, no card", registers_paddr);
            }
            Err(CardError::Failed { step, .. }) => {
                FAILURE.store(*step as u32, Ordering::Release);
                semi::println!("sdhost: card bring-up failed: {:?}", card);
            }
        }

        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let doorbell = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &table,
                u32::try_from(slot).unwrap_or(u32::MAX),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("sdhost: doorbell Retype failed: {:?}", error.code()));
        DOORBELL.store(doorbell.to_wire(), Ordering::Release);
        // Say the exports can be served.
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("sdhost: ready advance failed: {:?}", error.code()));

        let doorbell = EventCountKey::from_key(doorbell);
        let mut seen = 0;
        loop {
            seen = doorbell
                .await_ge(seen + 1, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("sdhost: doorbell wait failed: {:?}", error.code()));
            for exported in init.areas().iter().filter(|area| area.channel_base != 0) {
                drain(&mut host, &card, exported);
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
