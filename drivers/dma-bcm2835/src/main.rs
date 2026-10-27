// The component runs on Vesper only; see the host stub below.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! The dma component: the one owner of the BCM2835 DMA controller, behind
//! `dma_v1` (see `interfaces/dma_v1.if`; decision D1 of
//! `doc/agentic/audio-design.md`).
//!
//! Kickstart loads it for the `brcm,bcm2835-dma` node. All fifteen channels
//! share one register page, so the component that maps it can program every
//! channel: no other component maps it. It hands out the channels the
//! node's `brcm,dma-channel-mask` leaves to the ARM (Kickstart passes the mask
//! in `DriverInit::dma_channel_mask`), lowest first, and only those whose
//! interrupt it holds.
//!
//! Control blocks live in one page of the component's own memory, eight slots
//! per channel at `CHAIN_BYTES` × the channel's number; the component writes
//! a chain there and cleans it to the point of coherency (`Frame.CleanData`)
//! before it starts the channel, which reads the blocks through the bus
//! address `DriverInit::memory_bus` gives the page. Clients' buffers are
//! named by bus address and maintained by the clients.
//!
//! The first Thread services the interrupts: it reads `INT_STATUS`, and for
//! each claimed channel that raised one it clears `CS.INT`, works out from
//! `CONBLK_AD` how many blocks finished since the last time, and advances the
//! owning client's io-channel `EventCount` by that many. QEMU runs a whole
//! chain inside the write that starts it, so there the one interrupt it
//! leaves counts every block of the chain. Channels 11 to 14 share one line;
//! `INT_STATUS` tells them apart. Lines of channels the component has not
//! handed out stay masked: the firmware's channels raise them too.
//!
//! Calls and the interrupt Thread take turns (a `Turnstile`).

// The interface crates link the syscall layer, which builds for AArch64 only,
// so a host build has nothing but this stub: the library and its host tests
// (`tests/dma.rs`) carry the logic.
#[cfg(not(target_os = "vesper"))]
fn main() {}

#[cfg(target_os = "vesper")]
mod component {
    use {
        core::cell::UnsafeCell,
        dma::dma_v1::{DmaV1, Exception},
        dma_bcm2835::{
            CHAIN_BYTES, CHANNEL_STRIDE, CHANNELS, Chain, ChainError, ChannelRegisters, Channels,
            ControlBlock, MAX_CHAIN_BLOCKS, MmioChannelRegisters, MmioRegisterBlock, Position,
            RegisterBlock, acknowledging, clearing_debug_errors, copy_chain, error_bits,
            finished_blocks, interrupt_index, is_known_request, pausing, peripheral_address,
            position, resetting, ring_chain, slot_address, starting,
        },
        libdevice_driver::{
            DEVICE_WINDOW_VA, DriverInit, INIT_VA, MAX_DEVICE_PAGES, NO_BUS_ADDRESS, Turn,
            Turnstile, device_address, driver_compatible, driver_init, map_device_pages,
        },
        libobject::{
            EventCountKey, FrameAttribute, FrameKey, IrqHandlerKey, KeyTableKey, NotificationKey,
            ObjectType, RawKey, Rights, UntypedKey, export::init_return_key,
            thread::ThreadReturnKey,
        },
        libuser::semihosting as semi,
    };

    libuser::entry!(main);

    driver_compatible!("brcm,bcm2835-dma");

    /// Where the page of control block slots is mapped, in the device
    /// window: above the room of the device pages and of a serial channel's
    /// pages (`libserial_channel::CONTROL_VA`, five pages from 0x3802_0000),
    /// should the component ever trace on one.
    const SLOTS_VA: u64 = DEVICE_WINDOW_VA + 0x4_0000;

    const _: () = assert!(SLOTS_VA >= DEVICE_WINDOW_VA + (MAX_DEVICE_PAGES as u64 + 5) * 4096);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a virtual address of this AArch64 component fits usize"
    )]
    const SLOTS_VA_ADDRESS: usize = SLOTS_VA as usize;

    /// How long a paused channel may take to have its last writes answered.
    /// One period of a 4 KiB block at the PWM's pace is about 12 ms; a pause
    /// stops it within one write, so this is a wide margin of our own.
    const DRAIN_TIMEOUT_NS: u64 = 1_000_000;

    /// One claimed channel.
    #[derive(Debug, Clone, Copy)]
    struct ChannelState {
        /// The owner, by its position among the component's data areas
        client: usize,
        /// The DREQ it was claimed for
        dreq: u32,
        /// A DMA Lite channel (§4.5): 16-bit lengths
        lite: bool,
        /// Blocks of the last chain started on it
        blocks: usize,
        cyclic: bool,
        /// A chain runs and its end was not serviced yet
        running: bool,
        /// The block it was on when its finished blocks were last counted
        last: usize,
        /// `dma_v1.errors` bits seen since the chain started
        errors: u32,
    }

    /// The controller and its channels.
    struct Dma {
        /// The register page's base
        base: usize,
        /// The page of control block slots, and its bus address
        slots: FrameKey,
        slots_bus: u32,
        channels: Channels,
        states: [Option<ChannelState>; CHANNELS],
        /// Interrupts, by their position in `DriverInit::interrupts`, whose
        /// handler was enabled (acknowledged once)
        enabled_interrupts: u16,
    }

    struct Shared(UnsafeCell<Option<Dma>>);

    // SAFETY: the cell is filled before the component says it is ready and
    // then reached only by the holder of a turn of `TURNSTILE`.
    unsafe impl Sync for Shared {}

    static DMA: Shared = Shared(UnsafeCell::new(None));
    static TURNSTILE: Turnstile = Turnstile::new();

    fn init() -> &'static DriverInit {
        // SAFETY: Kickstart maps every driver's init record at `INIT_VA`; it
        // is read in this component's own `AddressSpace` only.
        unsafe { driver_init(INIT_VA) }
    }

    /// The controller, for the holder of `turn`.
    #[allow(
        clippy::needless_pass_by_ref_mut,
        reason = "the exclusive borrow of the turn proves the caller holds it"
    )]
    fn dma<'turn>(_turn: &'turn mut Turn<'_>) -> &'turn mut Dma {
        // SAFETY: only the holder of a turn gets here, one at a time, and the
        // cell was filled before the component became ready.
        unsafe { (*DMA.0.get()).as_mut() }.expect("the controller is set up before it is served")
    }

    /// The client whose key carries `badge`, by its position among the data
    /// areas.
    fn client_of(badge: u64) -> Option<usize> {
        init().area_for_badge(badge).map(|(client, _area)| client)
    }

    impl Dma {
        /// The global registers.
        fn global(&self) -> MmioRegisterBlock<'static> {
            // SAFETY: `base` is the register page `map_device_pages` mapped
            // read/write as device memory for the component's lifetime; the
            // holder of a turn is the only one touching it.
            unsafe {
                RegisterBlock::new_mmio(core::ptr::with_exposed_provenance_mut::<RegisterBlock>(
                    self.base,
                ))
            }
        }

        /// The registers of `channel`.
        fn channel(&self, channel: usize) -> MmioChannelRegisters<'static> {
            // SAFETY: as for `global`; channel `n`'s block is at
            // `CHANNEL_STRIDE` × `n` in the page (§4.2).
            unsafe {
                ChannelRegisters::new_mmio(
                    core::ptr::with_exposed_provenance_mut::<ChannelRegisters>(
                        self.base + channel * CHANNEL_STRIDE,
                    ),
                )
            }
        }

        /// The bus address of `channel`'s first control block slot.
        fn slots_bus_of(&self, channel: usize) -> u32 {
            slot_address(self.slots_bus, channel * MAX_CHAIN_BLOCKS)
        }

        /// The state of `channel` if the client with `badge` claimed it.
        fn owned(&mut self, badge: u64, channel: u32) -> Result<&mut ChannelState, Exception> {
            let not_owner = Exception::NotOwner {
                dma_channel: channel,
            };
            let client = client_of(badge).ok_or(not_owner)?;
            usize::try_from(channel)
                .ok()
                .and_then(|channel| self.states.get_mut(channel))
                .and_then(Option::as_mut)
                .filter(|state| state.client == client)
                .ok_or(not_owner)
        }

        /// Enable the handler of interrupt `index`, once.
        fn enable_interrupt(&mut self, index: usize) {
            if self.enabled_interrupts & (1 << index) != 0 {
                return;
            }
            let Some(interrupt) = init().interrupts().get(index) else {
                return;
            };
            IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
                .ack()
                .unwrap_or_else(|error| panic!("dma: interrupt Ack failed: {:?}", error.code()));
            self.enabled_interrupts |= 1 << index;
        }

        /// Reset `channel`, clear its error flags and let it run.
        fn prepare(&self, channel: usize) {
            let mut registers = self.channel(channel);
            registers.write_control_status(resetting());
            registers.write_control_block_address(0);
            clear_errors(&mut registers);
            let mut global = self.global();
            let enable = global.read_enable();
            global.write_enable(enable | (1 << channel));
        }

        /// Pause `channel`, wait for its writes to be answered, and reset
        /// it: §4.2.1.2, `CS.ACTIVE` pauses, `RESET` resets.
        fn halt(&self, channel: usize) {
            let mut registers = self.channel(channel);
            registers.write_control_status(pausing());
            let until = libuser::clock::now_ns() + DRAIN_TIMEOUT_NS;
            while registers.read_control_status().waiting_for_writes()
                && libuser::clock::now_ns() < until
            {
                core::hint::spin_loop();
            }
            registers.write_control_status(resetting());
            registers.write_control_block_address(0);
        }

        /// Write `chain` into `channel`'s slots, clean them, and start the
        /// channel on the first.
        fn run(&mut self, channel: usize, chain: &Chain) {
            let offset = channel * CHAIN_BYTES;
            for (index, block) in chain.blocks().iter().enumerate() {
                let address = SLOTS_VA_ADDRESS + offset + index * size_of::<ControlBlock>();
                // SAFETY: the slots page is mapped read/write for good; the
                // channel is idle, so the DMA does not read these slots now.
                unsafe {
                    core::ptr::write_volatile(
                        core::ptr::with_exposed_provenance_mut::<ControlBlock>(address),
                        *block,
                    );
                }
            }
            let start = offset as u64;
            self.slots
                .clean_data(start, start + chain.bytes() as u64)
                .unwrap_or_else(|error| {
                    panic!("dma: cleaning control blocks failed: {:?}", error.code())
                });
            if let Some(state) = self.states[channel].as_mut() {
                state.blocks = chain.blocks().len();
                state.cyclic = chain.is_cyclic();
                state.running = true;
                state.last = 0;
                state.errors = 0;
            }
            let first = self.slots_bus_of(channel);
            let mut registers = self.channel(channel);
            clear_errors(&mut registers);
            registers.write_control_block_address(first);
            // QEMU runs the whole chain inside this write.
            registers.write_control_status(starting());
        }

        /// Service the claimed channels that raised an interrupt.
        fn service(&mut self) {
            let status = self.global().read_interrupt_status();
            for channel in 0..CHANNELS {
                if status & (1 << channel) == 0 {
                    continue;
                }
                // A channel nobody claimed through this component belongs
                // to the firmware: its INT is not ours to clear.
                let Some(mut state) = self.states[channel] else {
                    continue;
                };
                let mut registers = self.channel(channel);
                if !state.running {
                    registers.write_control_status(acknowledging(false));
                    continue;
                }
                let control_status = registers.read_control_status();
                let address = registers.read_control_block_address();
                let now = position(self.slots_bus_of(channel), state.blocks, address);
                let still_running = matches!(now, Position::On(_));
                registers.write_control_status(acknowledging(still_running));
                let finished = finished_blocks(state.last, now, state.blocks, state.cyclic);
                state.errors |= error_bits(control_status, registers.read_debug());
                match now {
                    Position::On(index) => state.last = index,
                    Position::Ended | Position::Lost => {
                        state.running = false;
                        state.last = state.blocks;
                    }
                }
                self.states[channel] = Some(state);
                semi::println!(
                    "dma: channel {channel} interrupt: INT_STATUS {status:#06x}, END {}, INT {}, ERROR {}, {finished} blocks finished, {}",
                    control_status.end(),
                    control_status.interrupt(),
                    control_status.error(),
                    if still_running {
                        "running"
                    } else {
                        "chain ended"
                    }
                );
                if finished > 0
                    && let Some(area) = init().areas().get(state.client)
                {
                    EventCountKey::from_key(RawKey::from_wire(area.channel_event))
                        .advance(finished as u64)
                        .unwrap_or_else(|error| {
                            panic!("dma: client advance failed: {:?}", error.code())
                        });
                }
            }
        }
    }

    /// Clear the error flags `DEBUG` shows on a channel.
    fn clear_errors(registers: &mut MmioChannelRegisters<'static>) {
        if let Some(clearing) = clearing_debug_errors(registers.read_debug()) {
            registers.write_debug(clearing);
        }
    }

    /// `ChainError` as the interface's exception.
    fn refusal(error: ChainError, dreq: u32) -> Exception {
        match error {
            ChainError::Request => Exception::UnknownRequest { dreq },
            ChainError::BlockCount | ChainError::Length | ChainError::Address => {
                Exception::BadTransfer
            }
        }
    }

    /// The DMA controller.
    struct Controller;

    impl DmaV1 for Controller {
        fn enter(_badge: u64) {
            init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
                init().return_key,
            )));
        }

        fn claim(badge: u64, dreq: u32) -> Result<u32, Exception> {
            let client = client_of(badge).ok_or(Exception::NoChannel)?;
            if !is_known_request(dreq) {
                return Err(Exception::UnknownRequest { dreq });
            }
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            let interrupts = init().interrupts().len();
            let channel = dma
                .channels
                .claim(|channel| interrupt_index(channel) < interrupts)
                .ok_or(Exception::NoChannel)?;
            dma.prepare(channel);
            let lite = dma.channel(channel).read_debug().lite();
            dma.states[channel] = Some(ChannelState {
                client,
                dreq,
                lite,
                blocks: 0,
                cyclic: false,
                running: false,
                last: 0,
                errors: 0,
            });
            dma.enable_interrupt(interrupt_index(channel));
            semi::println!(
                "dma: channel {channel} claimed for DREQ {dreq} by client {client}{}",
                if lite { ", Lite" } else { "" }
            );
            Ok(u32::try_from(channel).unwrap_or(u32::MAX))
        }

        fn release(badge: u64, dma_channel: u32) -> Result<(), Exception> {
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            let running = dma.owned(badge, dma_channel)?.running;
            let channel = dma_channel as usize;
            if running {
                dma.halt(channel);
            }
            dma.states[channel] = None;
            dma.channels.release(channel);
            Ok(())
        }

        fn copy(
            badge: u64,
            dma_channel: u32,
            source: u32,
            destination: u32,
            block_bytes: u32,
            blocks: u32,
        ) -> Result<(), Exception> {
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            let state = *dma.owned(badge, dma_channel)?;
            if state.running {
                return Err(Exception::Busy { dma_channel });
            }
            let channel = dma_channel as usize;
            let chain = copy_chain(
                dma.slots_bus_of(channel),
                source,
                destination,
                block_bytes,
                blocks,
                state.lite,
            )
            .map_err(|error| refusal(error, state.dreq))?;
            dma.run(channel, &chain);
            Ok(())
        }

        fn start_ring(
            badge: u64,
            dma_channel: u32,
            ring: u32,
            period_bytes: u32,
            periods: u32,
        ) -> Result<(), Exception> {
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            let state = *dma.owned(badge, dma_channel)?;
            let fifo = peripheral_address(state.dreq)
                .ok_or(Exception::UnknownRequest { dreq: state.dreq })?;
            if state.running {
                return Err(Exception::Busy { dma_channel });
            }
            let channel = dma_channel as usize;
            let chain = ring_chain(
                dma.slots_bus_of(channel),
                ring,
                period_bytes,
                periods,
                state.dreq,
                fifo,
                state.lite,
            )
            .map_err(|error| refusal(error, state.dreq))?;
            dma.run(channel, &chain);
            Ok(())
        }

        fn position(badge: u64, dma_channel: u32) -> Result<u32, Exception> {
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            let state = *dma.owned(badge, dma_channel)?;
            let channel = dma_channel as usize;
            let address = dma.channel(channel).read_control_block_address();
            let index = match position(dma.slots_bus_of(channel), state.blocks, address) {
                Position::On(index) if state.running => index,
                _ => state.blocks,
            };
            Ok(u32::try_from(index).unwrap_or(u32::MAX))
        }

        fn stop(badge: u64, dma_channel: u32) -> Result<(), Exception> {
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            dma.owned(badge, dma_channel)?;
            let channel = dma_channel as usize;
            dma.halt(channel);
            if let Some(state) = dma.states[channel].as_mut() {
                state.running = false;
                state.last = state.blocks;
            }
            Ok(())
        }

        fn errors(badge: u64, dma_channel: u32) -> Result<u32, Exception> {
            let mut turn = TURNSTILE.enter(init());
            let dma = dma(&mut turn);
            let seen = dma.owned(badge, dma_channel)?.errors;
            let registers = dma.channel(dma_channel as usize);
            let now = error_bits(registers.read_control_status(), registers.read_debug());
            Ok(seen | now)
        }
    }

    dma::dma_v1_export!(Controller);

    /// Carve and map the page of control block slots from the component's
    /// memory into `slot`. Returns it with its bus address.
    fn slots_page(init: &DriverInit, slot: u64) -> (FrameKey, u32) {
        assert_ne!(
            init.memory_bus, NO_BUS_ADDRESS,
            "dma: the node's bus has no DMA addresses for its memory"
        );
        let table = KeyTableKey::from_key(RawKey::from_wire(init.table));
        let frame = FrameKey::from_key(
            UntypedKey::from_key(RawKey::from_wire(init.memory))
                .retype(
                    ObjectType::FRAME,
                    12,
                    0,
                    1,
                    &table,
                    u32::try_from(slot).expect("a slot fits in u32"),
                    Rights(Rights::READ | Rights::WRITE | Rights::GRANT),
                )
                .unwrap_or_else(|error| panic!("dma: slots Retype failed: {:?}", error.code())),
        );
        let (paddr, _size) = frame
            .get_extent()
            .unwrap_or_else(|error| panic!("dma: slots extent: {:?}", error.code()));
        frame
            .map(
                RawKey::from_wire(init.address_space),
                SLOTS_VA,
                Rights(Rights::READ | Rights::WRITE),
                FrameAttribute::Default,
            )
            .unwrap_or_else(|error| panic!("dma: slots Map failed: {:?}", error.code()));
        let bus = paddr
            .checked_sub(init.memory_paddr)
            .and_then(|offset| init.memory_bus.checked_add(offset))
            .and_then(|bus| u32::try_from(bus).ok())
            .expect("the slots page has a 32-bit bus address");
        (frame, bus)
    }

    /// Trace what the component was handed: the channels, the interrupts and
    /// the channels on each, and where the control blocks live.
    #[cfg(feature = "qemu")]
    fn trace_setup(init: &DriverInit, channels: Channels, slots_bus: u32) {
        let interrupts = init.interrupts();
        semi::println!(
            "dma: channels {:#06x} granted, {} interrupts{}, control blocks at bus {slots_bus:#010x}",
            channels.granted(),
            interrupts.len(),
            if interrupts.len() == dma_bcm2835::NODE_INTERRUPTS {
                ""
            } else {
                " (not the binding's 13: channels without one are not handed out)"
            }
        );
        for (index, interrupt) in interrupts.iter().enumerate() {
            semi::println!(
                "dma: interrupt {index}: line {} for channels {:#06x}",
                interrupt.line,
                dma_bcm2835::channels_of_interrupt(index)
            );
        }
    }

    /// Map the controller, carve the control block slots, set up the
    /// turnstile, then service interrupts: calls run on their callers'
    /// Threads.
    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let next_slot = map_device_pages(init)
            .unwrap_or_else(|error| panic!("dma: mapping registers failed: {:?}", error.code()));
        let registers =
            device_address(init, init.regions()[0].start).expect("dma: registers mapped");
        let (slots, slots_bus) = slots_page(init, next_slot);
        let channels = Channels::new(u32::try_from(init.dma_channel_mask).unwrap_or(0));
        #[cfg(feature = "qemu")]
        trace_setup(init, channels, slots_bus);
        let turns = UntypedKey::from_key(RawKey::from_wire(init.memory))
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &KeyTableKey::from_key(RawKey::from_wire(init.table)),
                u32::try_from(next_slot + 1).expect("a slot fits in u32"),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("dma: turns Retype failed: {:?}", error.code()));
        TURNSTILE.open(turns);
        // SAFETY: nothing else touches the cell until the component says it
        // is ready, just below.
        unsafe {
            *DMA.0.get() = Some(Dma {
                base: usize::try_from(registers).expect("a register address fits in usize"),
                slots,
                slots_bus,
                channels,
                states: [None; CHANNELS],
                enabled_interrupts: 0,
            });
        }
        EventCountKey::from_key(RawKey::from_wire(init.event_count))
            .advance(1)
            .unwrap_or_else(|error| panic!("dma: ready advance failed: {:?}", error.code()));

        // The interrupt Thread: this one. A handler is enabled when the
        // first channel on its line is claimed, and acknowledged again after
        // each interrupt it delivered was serviced.
        let notification = NotificationKey::from_key(RawKey::from_wire(init.notification));
        loop {
            let delivered = notification
                .await_bits(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("dma: interrupt wait failed: {:?}", error.code()));
            let mut turn = TURNSTILE.enter(init);
            dma(&mut turn).service();
            // Every handler that delivered is masked until acknowledged.
            for interrupt in init.interrupts() {
                if delivered & interrupt.bits != 0 {
                    IrqHandlerKey::from_key(RawKey::from_wire(interrupt.handler))
                        .ack()
                        .unwrap_or_else(|error| {
                            panic!("dma: interrupt Ack failed: {:?}", error.code())
                        });
                }
            }
        }
    }
}
