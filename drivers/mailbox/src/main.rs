// Host builds only check the interface implementation; the component itself
// runs on Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! The mailbox component: serves the `VideoCore` property mailbox
//! (`brcm,bcm2835-mbox`) to other components through `mailbox_v1` (see
//! `interfaces/mailbox_v1.if`).
//!
//! Kickstart loads it as a driver: on the Raspberry Pi 3 its register page is
//! the BCM interrupt controller's, handed to it as a device Frame. Its Thread
//! sets up a bounce page for the property buffers and two `EventCount`s, then
//! serves the mailbox interrupt: each reply from the firmware advances
//! `replies`.
//!
//! A function call runs on the caller's Thread. It takes a ticket, waits on
//! `turns` until every earlier request is done, writes its tag buffer into
//! the bounce page, cleans it to memory, posts the buffer's bus address on
//! the property channel, waits on `replies` for its answer, invalidates the
//! page, reads the answer, and advances `turns` for the next caller.
//!
//! The typed functions build their buffers themselves. `property` takes a
//! reference into the data area the caller shares with the component: the
//! component finds the caller's area from the stack the call runs on (see
//! `libdevice_driver::calling_area`), copies the buffer into the bounce page,
//! checks the copy (`mailbox_impl::check_request`: only allowed tags), and
//! copies the firmware's answer back to the area.

use {
    mailbox::mailbox_v1::{__runtime::IoRec, Clock, Device, Exception, MailboxV1},
    mailbox_impl::{ACT_LED_PIN, tag},
};

/// `firmware_error` code of a request that cannot run (non-Vesper builds).
#[cfg(not(target_os = "vesper"))]
const NOT_SERVED: u32 = u32::MAX;

/// The firmware's id of a clock: its position in `mailbox_v1.clock`, from 1.
fn clock_id(clock: Clock) -> u32 {
    let mut words = [0];
    mailbox::mailbox_v1::__runtime::Words::store(&clock, &mut words);
    u32::try_from(words[0] + 1).unwrap_or(u32::MAX)
}

/// The firmware's id of a device: its position in `mailbox_v1.device`.
fn device_id(device: Device) -> u32 {
    let mut words = [0];
    mailbox::mailbox_v1::__runtime::Words::store(&device, &mut words);
    u32::try_from(words[0]).unwrap_or(u32::MAX)
}

/// Power state bits: the device is on; it does not exist (in answers) or the
/// firmware should wait for it to settle (in requests).
const POWER_ON: u32 = 1 << 0;
const POWER_MISSING_OR_WAIT: u32 = 1 << 1;

/// A power state answer: whether the device is on, unless it does not exist.
fn power_state(state: u32) -> Result<bool, Exception> {
    if state & POWER_MISSING_OR_WAIT == 0 {
        Ok(state & POWER_ON != 0)
    } else {
        Err(Exception::FirmwareError { code: state })
    }
}

/// The firmware mailbox.
struct VideoCoreMailbox;

impl MailboxV1 for VideoCoreMailbox {
    #[cfg(target_os = "vesper")]
    fn enter(_badge: u64) {
        component::record_return_key();
    }

    fn get_board_revision() -> Result<u32, Exception> {
        call(tag::GET_BOARD_REVISION, &[], 1).map(|values| values[0])
    }

    fn get_board_serial() -> Result<u64, Exception> {
        call(tag::GET_BOARD_SERIAL, &[], 2)
            .map(|values| u64::from(values[0]) | u64::from(values[1]) << 32)
    }

    fn get_clock_rate(which: Clock) -> Result<u32, Exception> {
        call(tag::GET_CLOCK_RATE, &[clock_id(which)], 2).map(|values| values[1])
    }

    fn get_power_state(which: Device) -> Result<bool, Exception> {
        call(tag::GET_POWER_STATE, &[device_id(which)], 2).and_then(|values| power_state(values[1]))
    }

    fn set_power_state(which: Device, on: bool) -> Result<bool, Exception> {
        let state = POWER_MISSING_OR_WAIT | if on { POWER_ON } else { 0 };
        call(tag::SET_POWER_STATE, &[device_id(which), state], 2)
            .and_then(|values| power_state(values[1]))
    }

    fn set_led(on: bool) -> Result<(), Exception> {
        call(tag::SET_GPIO_STATE, &[ACT_LED_PIN, u32::from(on)], 2).map(|_values| ())
    }

    fn property(tags: IoRec) -> Result<(), Exception> {
        property(tags)
    }
}

mailbox::mailbox_v1_export!(VideoCoreMailbox);

/// Most value words of one typed tag.
const MAX_VALUE_WORDS: usize = 4;

/// One typed request with tag `tag` and value words `request`, answered with
/// `answer_words` value words.
#[cfg(target_os = "vesper")]
fn call(
    tag: u32,
    request: &[u32],
    answer_words: usize,
) -> Result<[u32; MAX_VALUE_WORDS], Exception> {
    component::call(tag, request, answer_words)
}

#[cfg(target_os = "vesper")]
fn property(tags: IoRec) -> Result<(), Exception> {
    component::property(tags)
}

#[cfg(not(target_os = "vesper"))]
fn call(
    _tag: u32,
    _request: &[u32],
    _answer_words: usize,
) -> Result<[u32; MAX_VALUE_WORDS], Exception> {
    Err(Exception::FirmwareError { code: NOT_SERVED })
}

#[cfg(not(target_os = "vesper"))]
fn property(_tags: IoRec) -> Result<(), Exception> {
    Err(Exception::FirmwareError { code: NOT_SERVED })
}

#[cfg(target_os = "vesper")]
mod component {
    use {
        super::{Exception, IoRec, MAX_VALUE_WORDS},
        arbitrary_int::{u4, u28},
        bitbybit::bitfield,
        core::sync::atomic::{AtomicU64, Ordering},
        derive_mmio::Mmio,
        libdevice_driver::{
            DEVICE_WINDOW_VA, INIT_VA, NO_BUS_ADDRESS, calling_area, device_address,
            driver_compatible, driver_init, map_device_pages,
        },
        libmmio::assert_register_offsets,
        libobject::{
            EventCountKey, FrameKey, IrqHandlerKey, KeyTableKey, NotificationKey, ObjectType,
            RawKey, Rights, UntypedKey, export::init_return_key, thread::ThreadReturnKey,
        },
        libuser::semihosting as semi,
        mailbox::mailbox_v1::__runtime::DataArea,
        mailbox_impl::{BUFFER_BYTES, check_request},
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-mbox");

    /// Mailbox status.
    #[bitfield(u32, default = 0, debug)]
    struct Status {
        /// The mailbox holds no word
        #[bit(30, r)]
        empty: bool,
        /// The mailbox cannot take another word
        #[bit(31, r)]
        full: bool,
    }

    /// Mailbox configuration.
    #[bitfield(u32, default = 0, debug)]
    struct Config {
        /// Interrupt while the mailbox holds data
        #[bit(0, rw)]
        data_irq: bool,
    }

    /// A mailbox word: a channel and the top 28 bits of a 16-byte aligned
    /// bus address.
    #[bitfield(u32, default = 0, debug)]
    struct MailboxWord {
        #[bits(0..=3, rw)]
        channel: u4,
        #[bits(4..=31, rw)]
        address_bits: u28,
    }

    /// The node's registers: mailbox 0 carries the firmware's answers to the
    /// ARM, mailbox 1 the ARM's requests to the firmware.
    #[derive(Mmio)]
    #[mmio(no_owned)]
    #[repr(C)]
    struct RegisterBlock {
        #[mmio(Read)]
        read0: MailboxWord,
        _reserved0: [u32; 5],
        #[mmio(PureRead)]
        status0: Status,
        config0: Config,
        #[mmio(Write)]
        write1: MailboxWord,
        _reserved1: [u32; 5],
        #[mmio(PureRead)]
        status1: Status,
    }

    assert_register_offsets!(RegisterBlock, 0x3c, {
        read0: 0x00,
        status0: 0x18,
        config0: 0x1c,
        write1: 0x20,
        status1: 0x38,
    });

    /// The registers mapped at `address`.
    ///
    /// # Safety
    ///
    /// `address` maps the node's registers as device memory for the rest of
    /// the component's life. The caller Thread holding the turn uses only
    /// mailbox 1, and the interrupt Thread only mailbox 0.
    unsafe fn registers(address: u64) -> MmioRegisterBlock<'static> {
        // SAFETY: the caller's contract.
        unsafe { RegisterBlock::new_mmio(address as *mut RegisterBlock) }
    }

    /// The property channel.
    const PROPERTY_CHANNEL: u4 = u4::new(8);

    /// Buffer code of a request, and of a buffer the firmware answered.
    const REQUEST: u32 = 0;
    const ANSWERED: u32 = 0x8000_0000;
    /// Set in a tag's code word once the firmware answered the tag.
    const TAG_ANSWERED: u32 = 0x8000_0000;

    /// What the interrupt Thread set up for the calls (zero until ready).
    static REGISTERS_VA: AtomicU64 = AtomicU64::new(0);
    static BOUNCE_VA: AtomicU64 = AtomicU64::new(0);
    static BOUNCE_BUS: AtomicU64 = AtomicU64::new(0);
    static BOUNCE_FRAME: AtomicU64 = AtomicU64::new(0);
    /// Requests finished, so ticket `t` may run once `turns >= t - 1`.
    static TURNS: AtomicU64 = AtomicU64::new(0);
    /// Answers the firmware gave: exchange `n` is answered once `replies >= n`.
    static REPLIES: AtomicU64 = AtomicU64::new(0);
    /// Requests sent to the firmware, in order; only the holder of the turn
    /// sends, so the count is the sequence number of its exchange.
    static EXCHANGES: AtomicU64 = AtomicU64::new(0);
    /// Last ticket taken.
    static TICKETS: AtomicU64 = AtomicU64::new(0);

    fn init() -> &'static libdevice_driver::DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    fn key(word: &AtomicU64) -> RawKey {
        RawKey::from_wire(word.load(Ordering::Acquire))
    }

    /// Record this component's `Thread.Return` key for its exports.
    pub fn record_return_key() {
        init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
            init().return_key,
        )));
    }

    /// The one request in flight: while a `Turn` lives, its holder alone uses
    /// the bounce page and the mailbox. Dropping it lets the next ticket run.
    struct Turn;

    impl Turn {
        /// Wait until the mailbox is ready and every earlier request is done.
        fn take() -> Self {
            EventCountKey::from_key(RawKey::from_wire(init().event_count))
                .await_ge(1, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("mailbox: ready wait failed: {:?}", error.code()));
            let ticket = TICKETS.fetch_add(1, Ordering::AcqRel) + 1;
            EventCountKey::from_key(key(&TURNS))
                .await_ge(ticket - 1, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("mailbox: turn wait failed: {:?}", error.code()));
            Self
        }

        /// The bounce page, as words.
        fn buffer() -> *mut u32 {
            BOUNCE_VA.load(Ordering::Acquire) as *mut u32
        }

        /// Send the `bytes` of the bounce page to the firmware and wait for
        /// its answer, which replaces them.
        #[allow(
            clippy::unused_self,
            reason = "the receiver proves the caller holds the turn"
        )]
        fn exchange(&self, bytes: usize) {
            let length = u64::try_from(bytes).unwrap_or(u64::MAX);
            let frame = FrameKey::from_key(key(&BOUNCE_FRAME));
            // SAFETY: the interrupt Thread mapped the registers before the
            // first turn; this Thread holds the turn and uses mailbox 1 only.
            let mut registers = unsafe { registers(REGISTERS_VA.load(Ordering::Acquire)) };
            frame
                .clean_data(0, length)
                .unwrap_or_else(|error| panic!("mailbox: CleanData failed: {:?}", error.code()));
            while registers.read_status1().full() {
                core::hint::spin_loop();
            }
            // The bounce page is page aligned and below 4 GiB on the bus.
            let bus = u32::try_from(BOUNCE_BUS.load(Ordering::Acquire)).unwrap_or(0);
            let exchange = EXCHANGES.fetch_add(1, Ordering::AcqRel) + 1;
            registers.write_write1(
                MailboxWord::ZERO
                    .with_channel(PROPERTY_CHANNEL)
                    .with_address_bits(u28::new(bus >> 4)),
            );
            EventCountKey::from_key(key(&REPLIES))
                .await_ge(exchange, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("mailbox: reply wait failed: {:?}", error.code()));
            frame.invalidate_data(0, length).unwrap_or_else(|error| {
                panic!("mailbox: InvalidateData failed: {:?}", error.code())
            });
        }
    }

    impl Drop for Turn {
        fn drop(&mut self) {
            EventCountKey::from_key(key(&TURNS))
                .advance(1)
                .unwrap_or_else(|error| panic!("mailbox: turn release failed: {:?}", error.code()));
        }
    }

    /// One typed request (see [`super::call`]): the answer's value words, or
    /// the firmware's error code.
    pub fn call(
        tag: u32,
        request: &[u32],
        answer_words: usize,
    ) -> Result<[u32; MAX_VALUE_WORDS], Exception> {
        let value_words = request.len().max(answer_words).min(MAX_VALUE_WORDS);
        // Size, code, tag, value bytes, tag code, values, end tag.
        let words = 6 + value_words;
        let mut message = [0_u32; 6 + MAX_VALUE_WORDS];
        message[0] = u32::try_from(words * 4).unwrap_or(u32::MAX);
        message[1] = REQUEST;
        message[2] = tag;
        message[3] = u32::try_from(value_words * 4).unwrap_or(0);
        message[4] = 0;
        message[5..5 + request.len()].copy_from_slice(request);

        let turn = Turn::take();
        let buffer = Turn::buffer();
        for (index, &word) in message[..words].iter().enumerate() {
            // SAFETY: the bounce page is mapped read/write and only the
            // holder of the turn touches it.
            unsafe {
                buffer.add(index).write_volatile(word);
            }
        }
        turn.exchange(words * 4);
        for (index, word) in message[..words].iter_mut().enumerate() {
            // SAFETY: as above; the firmware has answered.
            *word = unsafe { buffer.add(index).read_volatile() };
        }
        drop(turn);

        if message[1] != ANSWERED {
            return Err(Exception::FirmwareError { code: message[1] });
        }
        if message[4] & TAG_ANSWERED == 0 {
            return Err(Exception::FirmwareError { code: message[4] });
        }
        let mut values = [0; MAX_VALUE_WORDS];
        values[..value_words].copy_from_slice(&message[5..5 + value_words]);
        Ok(values)
    }

    /// Words copied at a time between the data area and the bounce page.
    const COPY_WORDS: usize = 16;

    /// `property`: the caller's tag buffer `tags`, in its data area, goes to
    /// the firmware if it is a well-formed request of allowed tags; the answer
    /// replaces it in the area.
    pub fn property(tags: IoRec) -> Result<(), Exception> {
        let area = calling_area(init()).ok_or(Exception::NotPermitted)?;
        let data = DataArea {
            base: area.base,
            len: area.len,
        };
        let bytes = usize::try_from(tags.len).map_err(|_overflow| Exception::NotPermitted)?;
        if !data.contains(&tags)
            || bytes > BUFFER_BYTES
            || !bytes.is_multiple_of(4)
            || !tags.base.is_multiple_of(4)
        {
            return Err(Exception::NotPermitted);
        }
        let offset = tags.base - data.base;
        let words = bytes / 4;

        let turn = Turn::take();
        let buffer = Turn::buffer();
        // Copy the request into the bounce page: it is checked and sent from
        // there, out of the caller's reach.
        let mut chunk = [0_u32; COPY_WORDS];
        for start in (0..words).step_by(COPY_WORDS) {
            let count = COPY_WORDS.min(words - start);
            if !data.read_words(offset + start as u64 * 4, &mut chunk[..count]) {
                return Err(Exception::NotPermitted);
            }
            for (index, &word) in chunk[..count].iter().enumerate() {
                // SAFETY: the bounce page is mapped read/write, `words` fits
                // it, and only the holder of the turn touches it.
                unsafe {
                    buffer.add(start + index).write_volatile(word);
                }
            }
        }
        // SAFETY: as above; nothing else writes the page until it is sent.
        let request = unsafe { core::slice::from_raw_parts(buffer, words) };
        check_request(request).map_err(|_rejected| Exception::NotPermitted)?;
        turn.exchange(bytes);

        // Copy the answer back, then judge it.
        for start in (0..words).step_by(COPY_WORDS) {
            let count = COPY_WORDS.min(words - start);
            for (index, word) in chunk[..count].iter_mut().enumerate() {
                // SAFETY: as above; the firmware has answered.
                *word = unsafe { buffer.add(start + index).read_volatile() };
            }
            if !data.write_words(offset + start as u64 * 4, &chunk[..count]) {
                return Err(Exception::NotPermitted);
            }
        }
        // SAFETY: as above.
        let code = unsafe { buffer.add(1).read_volatile() };
        if code == ANSWERED {
            Ok(())
        } else {
            Err(Exception::FirmwareError { code })
        }
    }

    /// Set up the bounce page and the `EventCount`s, then serve the mailbox
    /// interrupt.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let mut slot = map_device_pages(init).unwrap_or_else(|error| {
            panic!("mailbox: mapping registers failed: {:?}", error.code())
        });
        let mut next_slot = || {
            let taken = u32::try_from(slot).unwrap_or(u32::MAX);
            slot += 1;
            taken
        };
        let registers_va =
            device_address(init, init.regions()[0].start).expect("mailbox: registers mapped");
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let memory = UntypedKey::from_key(RawKey::from_wire(init.memory));
        let mut carve = |kind: ObjectType, size_bits: u8| {
            memory
                .retype(kind, size_bits, 0, 1, &table, next_slot(), Rights::all())
                .unwrap_or_else(|error| panic!("mailbox: Retype failed: {:?}", error.code()))
        };
        let frame = carve(ObjectType::FRAME, 12);
        let turns = carve(ObjectType::EVENT_COUNT, 0);
        let replies = carve(ObjectType::EVENT_COUNT, 0);

        // The bounce page, right after the register pages.
        let bounce_va = DEVICE_WINDOW_VA + init.pages().len() as u64 * 0x1000;
        FrameKey::from_key(frame)
            .map(
                RawKey::from_wire(init.address_space),
                bounce_va,
                Rights(Rights::READ | Rights::WRITE),
                libobject::FrameAttribute::Default,
            )
            .unwrap_or_else(|error| panic!("mailbox: bounce page Map failed: {:?}", error.code()));
        let (paddr, _) = FrameKey::from_key(frame)
            .get_extent()
            .unwrap_or_else(|error| panic!("mailbox: bounce page address: {:?}", error.code()));
        assert_ne!(
            init.memory_bus, NO_BUS_ADDRESS,
            "the firmware cannot reach this memory"
        );
        let bus = init.memory_bus + (paddr - init.memory_paddr);

        REGISTERS_VA.store(registers_va, Ordering::Release);
        BOUNCE_VA.store(bounce_va, Ordering::Release);
        BOUNCE_BUS.store(bus, Ordering::Release);
        BOUNCE_FRAME.store(frame.to_wire(), Ordering::Release);
        TURNS.store(turns.to_wire(), Ordering::Release);
        REPLIES.store(replies.to_wire(), Ordering::Release);

        // SAFETY: `map_device_pages` mapped the registers for good; this
        // Thread uses mailbox 0 only.
        let mut registers = unsafe { registers(registers_va) };
        registers.write_config0(Config::ZERO.with_data_irq(true));
        let handlers = init.interrupts();
        for interrupt in handlers {
            IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
                .ack()
                .unwrap_or_else(|error| {
                    panic!("mailbox: interrupt Ack failed: {:?}", error.code())
                });
        }
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("mailbox: ready advance failed: {:?}", error.code()));
        semi::println!(
            "mailbox: registers at {:#x}, bounce page at bus address {bus:#x}, {} data areas",
            init.regions()[0].start,
            init.areas().len()
        );

        let interrupts = NotificationKey::from_key(RawKey::from_wire(init.notification));
        let replies = EventCountKey::from_key(replies);
        loop {
            interrupts
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| {
                    panic!("mailbox: interrupt wait failed: {:?}", error.code())
                });
            while !registers.read_status0().empty() {
                let word = registers.read_read0();
                if word.channel() == PROPERTY_CHANNEL {
                    replies.advance(1).unwrap_or_else(|error| {
                        panic!("mailbox: reply advance failed: {:?}", error.code())
                    });
                }
            }
            for interrupt in handlers {
                IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
                    .ack()
                    .unwrap_or_else(|error| {
                        panic!("mailbox: interrupt Ack failed: {:?}", error.code())
                    });
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
