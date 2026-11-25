// Host builds only check the command interpreter; the program itself runs on
// Vesper.
#![cfg_attr(target_os = "vesper", no_std, no_main)]
// libqemu's semihosting `println!` (the lamp's progress under QEMU) expands to it.
#![cfg_attr(all(target_os = "vesper", feature = "qemu"), feature(format_args_nl))]

//! The shell: a command prompt on the serial console.
//!
//! Kickstart loads it as a program (a `[[component]]` with a `priority`) and
//! attaches it to the PL011 driver's serial channel (`serial = true`): the
//! channel's control page and data area are mapped at the addresses
//! `libserial-channel` fixes, and its `EventCount`s are in `DriverInit::serial`.
//! The shell echoes what it receives, collects lines (`shell::LineEditor`),
//! and runs the commands of `shell::parse` against the mailbox (the LED), the
//! power component (resetting the board) and the compositor (the display).

#[cfg(target_os = "vesper")]
mod display;

#[cfg(target_os = "vesper")]
mod program {
    use {
        compositor_client::{
            compositor_v1::{Bindings as CompositorBindings, Client as CompositorClient},
            display_layout_v1::{
                Bindings as LayoutBindings, Client as LayoutClient, Error as LayoutError,
                Exception as LayoutException, Rotation,
            },
        },
        libdevice_driver::{DriverInit, driver_init},
        libio_channel::{Consumer, IoRec, NoEvent, Packet, Producer},
        libobject::{EventCountKey, KeyTableKey, RawKey},
        libserial_channel::{CONTROL_VA, DATA_AREA_VA, DATA_PAGES, SLOTS, SerialChannel},
        mailbox_client::mailbox_v1::{
            Bindings as MailboxBindings, Client as MailboxClient, Error as MailboxError,
            Exception as MailboxException,
        },
        power_client::power_v1::{Bindings as PowerBindings, Client as PowerClient},
        shell::{
            Board, Console, Edit, Failure, LineEditor, MAX_OUTPUTS, Outcome, Output, PROMPT,
            execute, parse,
        },
    };

    /// Keys of `display_layout_v1`: one per function.
    const LAYOUT_KEYS: usize = 7;

    /// Where in the shell's table the layout keys go: past the table and the
    /// copy slot `draw` takes from the first free slot.
    const LAYOUT_SLOT_OFFSET: u32 = 2;

    /// `Failure::status` of the layout's own refusals; `detail` says which.
    const LAYOUT_REFUSED: u64 = 0x4000;
    /// `detail` of a layout refusal: no such display, not allowed (the
    /// shell's manifest entry lacks `layout = true`), it would overlap
    /// another, or the display failed to turn.
    const NO_OUTPUT: u64 = 1;
    const LAYOUT_NOT_PERMITTED: u64 = 2;
    const OVERLAPS: u64 = 3;
    const TURN_FAILED: u64 = 4;

    libuser::entry!(main);

    /// Bytes one transmit packet carries: the transmit buffer is the start of
    /// the data area.
    const TRANSMIT_BYTES: usize = 256;

    /// Receive buffers kept submitted, of [`RECEIVE_BYTES`] each, from the
    /// middle of the data area.
    const RECEIVE_BUFFERS: usize = 8;
    const RECEIVE_BYTES: u64 = 64;
    const RECEIVE_AREA_VA: u64 = DATA_AREA_VA + DATA_PAGES as u64 * 4096 / 2;

    const _: () = assert!(TRANSMIT_BYTES as u64 <= RECEIVE_AREA_VA - DATA_AREA_VA);
    // Every receive buffer can be out at once.
    const _: () = assert!(RECEIVE_BUFFERS * 2 <= SLOTS);

    /// `Failure::status` of a call the kernel refused, or an answer it could
    /// not decode.
    const CALL_REFUSED: u64 = 0x1000;
    const UNEXPECTED: u64 = 0x1001;
    /// `Failure::status` of the firmware's own errors; `detail` is its code.
    const FIRMWARE_ERROR: u64 = 0x2000;
    const MAILBOX_BUSY: u64 = 0x2001;
    const NOT_PERMITTED: u64 = 0x2002;
    /// `Failure::status` of a service the shell has no binding for, or does
    /// not provide yet.
    const NOT_GRANTED: u64 = 0x3000;

    /// The console: the serial channel's two directions.
    struct SerialConsole {
        submit: Producer<'static, SLOTS, EventCountKey>,
        sent: Consumer<'static, SLOTS, EventCountKey>,
        give_buffer: Producer<'static, SLOTS, NoEvent>,
        filled: Consumer<'static, SLOTS, EventCountKey>,
        /// Bytes of the last received packet not read yet: the buffer's index
        /// in the receive area, the next byte and the end.
        pending: Option<(usize, u64, u64)>,
    }

    /// An empty receive buffer, tagged with its index.
    fn receive_buffer(index: usize) -> Packet {
        Packet::new(
            index as u64,
            &[IoRec {
                base: RECEIVE_AREA_VA + index as u64 * RECEIVE_BYTES,
                len: RECEIVE_BYTES,
            }],
        )
        .expect("one record")
    }

    impl SerialConsole {
        /// The console on the channel in the control page, with every receive
        /// buffer submitted.
        fn new(init: &libdevice_driver::DriverInit) -> Self {
            let keys = init.serial;
            assert_ne!(
                keys.transmit_submitted, 0,
                "the shell has no serial channel: its manifest entry needs `serial = true`"
            );
            // SAFETY: Kickstart mapped the control page read/write at
            // `CONTROL_VA` for this component's lifetime.
            let channel = unsafe { SerialChannel::at(CONTROL_VA) };
            let event = |wire: u64| EventCountKey::from_key(RawKey::from_wire(wire));
            let (submit, sent) = channel.transmit.client(
                event(keys.transmit_submitted),
                event(keys.transmit_completed),
            );
            let (mut give_buffer, filled) = channel
                .receive
                .client(NoEvent, event(keys.receive_completed));
            for index in 0..RECEIVE_BUFFERS {
                give_buffer
                    .put(&receive_buffer(index))
                    .expect("room for every receive buffer");
            }
            Self {
                submit,
                sent,
                give_buffer,
                filled,
                pending: None,
            }
        }

        /// Send `bytes`, a packet at a time, each handed back before the next.
        fn send(&mut self, bytes: &[u8]) {
            for chunk in bytes.chunks(TRANSMIT_BYTES) {
                for (offset, &byte) in chunk.iter().enumerate() {
                    // SAFETY: the data area is mapped read/write for this
                    // component's lifetime, and the driver holds no packet
                    // in the transmit buffer: each packet is waited for.
                    unsafe {
                        ((DATA_AREA_VA + offset as u64) as *mut u8).write_volatile(byte);
                    }
                }
                let packet = Packet::new(
                    0,
                    &[IoRec {
                        base: DATA_AREA_VA,
                        len: chunk.len() as u64,
                    }],
                )
                .expect("one record");
                self.submit
                    .put(&packet)
                    .expect("room for the one transmit packet");
                self.submit
                    .signal()
                    .unwrap_or_else(|error| panic!("transmit Advance failed: {:?}", error.code()));
                self.sent
                    .wait()
                    .unwrap_or_else(|_| panic!("the transmit channel failed"));
            }
        }

        /// The next received byte, waiting for the driver to bring some.
        fn read_byte(&mut self) -> u8 {
            loop {
                if let Some((index, next, end)) = self.pending {
                    if next < end {
                        // SAFETY: the driver returned this buffer, which lies in
                        // the data area mapped read/write for this
                        // component's lifetime.
                        let byte = unsafe {
                            ((RECEIVE_AREA_VA + index as u64 * RECEIVE_BYTES + next) as *const u8)
                                .read_volatile()
                        };
                        self.pending = Some((index, next + 1, end));
                        return byte;
                    }
                    self.give_buffer
                        .put(&receive_buffer(index))
                        .expect("room for every receive buffer");
                    self.pending = None;
                }
                let packet = self
                    .filled
                    .wait()
                    .unwrap_or_else(|_| panic!("the receive channel failed"));
                let index = usize::try_from(packet.value()).unwrap_or(usize::MAX);
                let len = packet.recs().first().map_or(0, |rec| rec.len);
                assert!(
                    index < RECEIVE_BUFFERS,
                    "a receive buffer the shell never submitted"
                );
                self.pending = Some((index, 0, len.min(RECEIVE_BYTES)));
            }
        }
    }

    impl Console for SerialConsole {
        fn print(&mut self, text: &str) {
            self.send(text.as_bytes());
        }
    }

    /// What the commands act on: the components the shell imports from.
    struct Services {
        mailbox: MailboxClient,
        compositor: CompositorClient,
        /// The compositor's layout, once a `layout` command asked for it.
        layout: Option<LayoutClient>,
        init: &'static DriverInit,
        /// Resets the board; under QEMU `end` exits it instead.
        #[cfg_attr(feature = "qemu", allow(dead_code))]
        power: PowerClient,
    }

    fn mailbox_failure(error: MailboxError) -> Failure {
        match error {
            MailboxError::Call { status, detail } => Failure {
                status: CALL_REFUSED,
                detail: status << 32 | detail[0] & 0xffff_ffff,
            },
            MailboxError::Exception(MailboxException::FirmwareError { code }) => Failure {
                status: FIRMWARE_ERROR,
                detail: u64::from(code),
            },
            MailboxError::Exception(MailboxException::Busy) => Failure {
                status: MAILBOX_BUSY,
                detail: 0,
            },
            MailboxError::Exception(MailboxException::NotPermitted) => Failure {
                status: NOT_PERMITTED,
                detail: 0,
            },
            MailboxError::NotGranted => Failure {
                status: NOT_GRANTED,
                detail: 0,
            },
            MailboxError::Malformed | MailboxError::UnexpectedAnswer { .. } => Failure {
                status: UNEXPECTED,
                detail: 0,
            },
        }
    }

    fn layout_failure(error: LayoutError) -> Failure {
        let refused = |detail| Failure {
            status: LAYOUT_REFUSED,
            detail,
        };
        match error {
            LayoutError::Exception(LayoutException::NoOutput) => refused(NO_OUTPUT),
            LayoutError::Exception(LayoutException::NotPermitted) => refused(LAYOUT_NOT_PERMITTED),
            LayoutError::Exception(LayoutException::Overlaps) => refused(OVERLAPS),
            LayoutError::Exception(LayoutException::Failed) => refused(TURN_FAILED),
            LayoutError::Call { status, detail } => Failure {
                status: CALL_REFUSED,
                detail: status << 32 | detail[0] & 0xffff_ffff,
            },
            LayoutError::NotGranted => Failure {
                status: NOT_GRANTED,
                detail: 0,
            },
            LayoutError::Malformed | LayoutError::UnexpectedAnswer { .. } => Failure {
                status: UNEXPECTED,
                detail: 0,
            },
        }
    }

    const fn rotation(turns: u8) -> Rotation {
        match turns % 4 {
            1 => Rotation::R90,
            2 => Rotation::R180,
            3 => Rotation::R270,
            _ => Rotation::R0,
        }
    }

    const fn turns(rotation: Rotation) -> u8 {
        match rotation {
            Rotation::R0 => 0,
            Rotation::R90 => 1,
            Rotation::R180 => 2,
            Rotation::R270 => 3,
        }
    }

    impl Services {
        /// The compositor's layout: the first time, ask the compositor to
        /// install its keys in the shell's table, and read their wire keys
        /// back from the data area.
        fn layout(&mut self) -> Result<&LayoutClient, Failure> {
            if self.layout.is_none() {
                let not_granted = Failure {
                    status: NOT_GRANTED,
                    detail: 0,
                };
                let own = KeyTableKey::from_key(RawKey::from_wire(self.init.table));
                let first_slot = u32::try_from(self.init.first_free_slot)
                    .map_err(|_overflow| not_granted)?
                    + LAYOUT_SLOT_OFFSET;
                self.compositor
                    .display_layout(&own, first_slot)
                    .map_err(|_error| not_granted)?;
                let area = self.compositor.data_area().ok_or(not_granted)?;
                let mut keys = [0_u64; LAYOUT_KEYS];
                if !area.read_u64s(0, &mut keys) {
                    return Err(not_granted);
                }
                self.layout = LayoutBindings::from_installed(&keys, None).map(LayoutClient::new);
            }
            self.layout.as_ref().ok_or(Failure {
                status: NOT_GRANTED,
                detail: 0,
            })
        }
    }

    impl Board for Services {
        fn set_led(&mut self, on: bool) -> Result<(), Failure> {
            self.mailbox.set_led(on).map_err(mailbox_failure)
        }

        fn draw(&mut self) -> Result<(), Failure> {
            crate::display::draw(self.init, &self.compositor)
        }

        fn outputs(&mut self, into: &mut [Output; MAX_OUTPUTS]) -> Result<usize, Failure> {
            let layout = self.layout()?;
            let count = usize::try_from(layout.count().map_err(layout_failure)?)
                .unwrap_or(0)
                .min(MAX_OUTPUTS);
            for (index, slot) in (0_u32..).zip(into.iter_mut().take(count)) {
                let (id, x, y, width, height) = layout.output(index).map_err(layout_failure)?;
                let (rotation, width_mm, height_mm) =
                    layout.output_details(index).map_err(layout_failure)?;
                *slot = Output {
                    id,
                    x,
                    y,
                    width,
                    height,
                    turns: turns(rotation),
                    width_mm,
                    height_mm,
                };
            }
            Ok(count)
        }

        fn turn_output(&mut self, id: u32, turns: u8) -> Result<(), Failure> {
            self.layout()?
                .set_rotation(id, rotation(turns))
                .map_err(layout_failure)
        }

        fn move_output(&mut self, id: u32, x: i32, y: i32) -> Result<(), Failure> {
            self.layout()?
                .set_position(id, x, y)
                .map_err(layout_failure)
        }
    }

    /// Leave the shell: shut QEMU down.
    #[cfg(feature = "qemu")]
    fn end(_services: &Services) -> ! {
        libuser::semihosting::exit_success()
    }

    /// Leave the shell: reset the board.
    #[cfg(not(feature = "qemu"))]
    fn end(services: &Services) -> ! {
        // A reset that works never returns; if it is refused there is nothing
        // left to do but stop.
        let _refused = services.power.reset().is_err();
        libuser::halt()
    }

    fn main(init_va: u64) -> ! {
        // SAFETY: Kickstart passes the init page's address in x0.
        let init = unsafe { driver_init(init_va) };
        let mut console = SerialConsole::new(init);
        let mut services = Services {
            compositor: CompositorClient::new(CompositorBindings::from_imports(init.imports())),
            layout: None,
            init,
            mailbox: MailboxClient::new(MailboxBindings::from_imports(init.imports())),
            power: PowerClient::new(PowerBindings::from_imports(init.imports())),
        };

        console.print("Vesper shell, type 'help'\r\n");
        let font = init.resource(0).and_then(libgraphics::Font::new);
        console.print(match font {
            Some(_) => "font: 95 glyphs mapped\r\n",
            None => "font: missing\r\n",
        });
        console.print(PROMPT);
        let mut editor = LineEditor::new();
        loop {
            let byte = console.read_byte();
            match editor.push(byte) {
                Edit::Nothing => {}
                Edit::Echo(byte) => console.send(&[byte]),
                Edit::Erase => console.print(LineEditor::ERASE),
                Edit::Line => {
                    console.print("\r\n");
                    let outcome = execute(parse(editor.line()), &mut console, &mut services);
                    editor.clear();
                    if outcome == Outcome::End {
                        console.print("Bye\r\n");
                        end(&services);
                    }
                    console.print(PROMPT);
                }
            }
        }
    }
}

#[cfg(not(target_os = "vesper"))]
fn main() {}
