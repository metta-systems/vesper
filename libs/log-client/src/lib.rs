#![no_std]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Log lines to the log server (`userspace/log-server`) on the component's
//! `log_v1` io-channel, never waiting (`doc/agentic/logging-design.md`).
//!
//! A component that imports `log_v1` (`uses log_v1 via component log_server
//! { flush }` in its `imports.if`) gets the channel; this crate finds it in
//! the init record the first time a line is sent. Each line takes one of the
//! channel's buffers: [`send`] writes it there, puts the packet and calls
//! `flush`, which only wakes the server. A line that finds no buffer free, or
//! another Thread of the component in the middle of sending, is dropped and
//! counted; the count rides on the next line that gets through, and the
//! server prints it. Nothing here waits, so a real-time Thread can log.
//!
//! - [`Logger`] is a `log::Log` for components that use the `log` crate;
//! - [`log_println!`] logs a formatted line at `Info`;
//! - [`send`] is the bare call (`libserial-trace` uses it).
//!
//! Under `qemu` the logger and the macro print every line on the
//! semihosting console as well, as before the log server existed; [`send`]
//! does not.
//!
//! The component's last words go here too (`doc/agentic/logging-design.md`,
//! section Panics and faults):
//!
//! - once the channel is found, the runtime's panic handler reports through
//!   it: `PANICKED: <component>: <message> at <file>:<line>`;
//! - the crate exports the component's fault handler ([`FAULT_HANDLER_EXPORT`],
//!   installed by Kickstart), which logs `FAULT: <component> thread <n>: …`
//!   for a fault of any of its Threads, a stack overflow included, and stops
//!   the Thread.
//!
//! Both wait a bounded time for the log server to take the line, since the
//! Thread never runs again.
//!
//! [`FAULT_HANDLER_EXPORT`]: libdevice_driver::FAULT_HANDLER_EXPORT

pub use log_server::Level;
use {
    core::{
        cell::UnsafeCell,
        fmt::{self, Write},
        panic::PanicInfo,
        sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    libdevice_driver::{INIT_VA, STACK_REGION, STACK_REGION_BYTES, driver_init},
    libio_channel::{Consumer, NoEvent, Packet, Producer},
    libobject::{
        EventCountKey, RawKey,
        export::{self, PpcResult},
        fault::FaultAction,
        ppc_export,
        thread::ThreadReturnKey,
    },
    log_interface_client::log_v1::{Bindings, CHANNEL_PAGES as DECLARED_PAGES, Client},
    log_server::{
        BUFFERS, CHANNEL_PAGES, CHANNEL_SLOTS, FreeBuffers, Header, LINE_BYTES, LINE_LEN,
        LineWriter, buffer_of, buffer_record, channel_at, report::FaultReport,
    },
};

const _: () = assert!(
    DECLARED_PAGES == CHANNEL_PAGES,
    "log_v1.if and log-server agree"
);

/// The component's end of its `log_v1` channel.
struct Channel {
    client: Client,
    submit: Producer<'static, CHANNEL_SLOTS, NoEvent>,
    returns: Consumer<'static, CHANNEL_SLOTS, NoEvent>,
    data_base: u64,
    free: FreeBuffers,
    /// Lines dropped since the last one put
    dropped: u32,
}

impl Channel {
    /// The channel the init record grants, if the component imports
    /// `log_v1`.
    fn attach() -> Option<Self> {
        // SAFETY: Kickstart maps every component's init record at `INIT_VA`.
        let init = unsafe { driver_init(INIT_VA) };
        let client = Client::new(Bindings::from_imports(init.imports()));
        let area = client.channel()?;
        if area.data.len < BUFFERS as u64 * LINE_BYTES {
            return None;
        }
        // SAFETY: Kickstart mapped the channel's control page read/write for
        // good, shared with the log server only.
        let channel = unsafe { channel_at(area.control) };
        let (submit, returns) = channel.client(NoEvent, NoEvent);
        PAUSE_EVENT.store(area.event, Ordering::Relaxed);
        libuser::set_panic_reporter(report_panic);
        Some(Self {
            client,
            submit,
            returns,
            data_base: area.data.base,
            free: FreeBuffers::all(),
            dropped: 0,
        })
    }

    /// Take back the buffers the server wrote out.
    fn reclaim(&mut self) {
        while let Ok(Some(back)) = self.returns.get() {
            if let Some(index) = back
                .recs()
                .first()
                .and_then(|&record| buffer_of(self.data_base, record))
            {
                self.free.give(index);
            }
        }
    }

    /// Write the line into a free buffer and put it; whether it went.
    fn put(&mut self, level: Option<Level>, arguments: fmt::Arguments<'_>, dropped: u32) -> bool {
        self.dropped = self.dropped.saturating_add(dropped);
        self.reclaim();
        let Some(index) = self.free.take() else {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        };
        let record = buffer_record(self.data_base, index, LINE_BYTES);
        let address = usize::try_from(record.base).unwrap_or(usize::MAX);
        // SAFETY: the buffer lies in the channel's data pages (checked at
        // attach), mapped read/write for good; the server reads it only
        // while it holds the packet naming it, and this one is free.
        let buffer = unsafe {
            core::slice::from_raw_parts_mut(
                core::ptr::with_exposed_provenance_mut::<u8>(address),
                LINE_LEN,
            )
        };
        let mut writer = LineWriter::new(buffer);
        // Writing to a `LineWriter` cannot fail.
        let _written = writer.write_fmt(arguments);
        let (len, cut) = writer.finish();
        let header = Header {
            level,
            cut,
            dropped: u16::try_from(self.dropped).unwrap_or(u16::MAX),
            micros: libuser::clock::now_ns() / 1000,
        };
        let packet = Packet::new(
            header.encode(),
            &[buffer_record(self.data_base, index, len as u64)],
        )
        .expect("one record");
        if self.submit.put(&packet).is_err() {
            self.free.give(index);
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.dropped = 0;
        true
    }

    /// Whether the server gave every buffer back: it has taken every line.
    fn all_returned(&mut self) -> bool {
        self.reclaim();
        self.free.count() as usize == BUFFERS
    }
}

/// Whether the channel was looked for, and what was found.
enum State {
    Unattached,
    Attached(Option<Channel>),
}

/// The component's channel, behind a try-lock: a sender that finds it taken
/// drops its line rather than wait.
struct Shared {
    busy: AtomicBool,
    state: UnsafeCell<State>,
    /// Lines dropped because another Thread was sending
    dropped_while_busy: AtomicU32,
}

// SAFETY: `state` is touched only by the Thread that set `busy`.
unsafe impl Sync for Shared {}

static SHARED: Shared = Shared {
    busy: AtomicBool::new(false),
    state: UnsafeCell::new(State::Unattached),
    dropped_while_busy: AtomicU32::new(0),
};

/// What became of a line given to [`send`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// Put on the channel; the server writes it out.
    Put,
    /// Dropped and counted: no buffer was free, or another Thread was
    /// sending.
    Dropped,
    /// The component has no log channel (it does not import `log_v1`, or the
    /// image has no log server).
    NoChannel,
}

/// Put one line on the component's log channel and wake the log server.
/// Never waits.
///
/// Never inlined: its locals (a packet, the channel's bindings) then take
/// stack only while it runs, not in every caller's frame; Threads with small
/// stacks (`usb-core`'s overflowed) trace from deep call chains.
#[inline(never)]
pub fn send(level: Option<Level>, arguments: fmt::Arguments<'_>) -> Sent {
    if SHARED.busy.swap(true, Ordering::Acquire) {
        SHARED.dropped_while_busy.fetch_add(1, Ordering::Relaxed);
        return Sent::Dropped;
    }
    // SAFETY: `busy` is held, so this is the only reference.
    let state = unsafe { &mut *SHARED.state.get() };
    if matches!(state, State::Unattached) {
        *state = State::Attached(Channel::attach());
    }
    let (sent, client) = match state {
        State::Attached(Some(channel)) => {
            let dropped = SHARED.dropped_while_busy.swap(0, Ordering::Relaxed);
            if channel.put(level, arguments, dropped) {
                (Sent::Put, Some(&raw const channel.client))
            } else {
                (Sent::Dropped, None)
            }
        }
        State::Attached(None) | State::Unattached => (Sent::NoChannel, None),
    };
    SHARED.busy.store(false, Ordering::Release);
    if let Some(client) = client {
        // SAFETY: the channel, once attached, stays in `SHARED` for good and
        // the client is never written again; `flush` only reads its
        // bindings.
        let client = unsafe { &*client };
        // The server looks on its own every few milliseconds when this fails.
        let _flushed = client.flush();
    }
    sent
}

// ── Last words: a panic or a fault ──────────────────────────────────────

/// The channel's `EventCount` (`RECV`), which the log server never advances:
/// waiting on it with a timeout is a sleep. Zero until the channel is found.
static PAUSE_EVENT: AtomicU64 = AtomicU64::new(0);

/// How often a last line retries a channel another Thread is sending on.
const LAST_LINE_TRIES: u32 = 4;
/// The wait between those tries and between looks at the returned buffers.
const LAST_LINE_PAUSE_NS: u64 = 5_000_000;
/// How many looks the server gets to take the line: 100 ms in all.
const LAST_LINE_LOOKS: u32 = 20;
/// Once the server took the line, the time the UART driver gets to send it
/// (a 256-byte line takes 22 ms at 115 200 baud) before the Thread stops;
/// under QEMU the panic then ends the run.
const LAST_LINE_DRAIN_NS: u64 = 50_000_000;

/// Let other Threads run for `ns`: a wait on [`PAUSE_EVENT`] that only its
/// timeout ends, or, without one, a spin on the clock.
fn pause(ns: u64) {
    let event = PAUSE_EVENT.load(Ordering::Relaxed);
    if event != 0
        && EventCountKey::from_key(RawKey::from_wire(event))
            .sleep(ns)
            .is_ok()
    {
        return;
    }
    let deadline = libuser::clock::now_ns().saturating_add(ns);
    while libuser::clock::now_ns() < deadline {
        core::hint::spin_loop();
    }
}

/// Send the line a Thread logs before it stops for good, and give the log
/// server a bounded time to take it: [`send`], retried a few times while
/// another Thread holds the channel, then up to 100 ms until every buffer
/// has come back, then 50 ms for the UART. A panic inside [`send`] on this
/// Thread leaves the channel held: the line is dropped, never waited for.
fn send_last_line(arguments: fmt::Arguments<'_>) -> Sent {
    let mut sent = send(Some(Level::Error), arguments);
    for _ in 1..LAST_LINE_TRIES {
        if sent != Sent::Dropped {
            break;
        }
        pause(LAST_LINE_PAUSE_NS);
        sent = send(Some(Level::Error), arguments);
    }
    if sent != Sent::Put {
        return sent;
    }
    for _ in 0..LAST_LINE_LOOKS {
        pause(LAST_LINE_PAUSE_NS);
        if SHARED.busy.swap(true, Ordering::Acquire) {
            continue;
        }
        // SAFETY: `busy` is held, so this is the only reference.
        let state = unsafe { &mut *SHARED.state.get() };
        let returned = match state {
            State::Attached(Some(channel)) => channel.all_returned(),
            State::Attached(None) | State::Unattached => true,
        };
        SHARED.busy.store(false, Ordering::Release);
        if returned {
            // The server wrote the line into a serial buffer; give the UART
            // driver time to send it.
            pause(LAST_LINE_DRAIN_NS);
            break;
        }
    }
    sent
}

/// The component's manifest name, from its init record.
fn component_name() -> &'static str {
    // SAFETY: Kickstart maps every component's init record at `INIT_VA`.
    unsafe { driver_init(INIT_VA) }.name()
}

/// The runtime's panic reporter (`libuser::set_panic_reporter`), set once the
/// channel is found: `PANICKED: <component>: <message> at <file>:<line>`.
/// The line is formatted straight into a channel buffer (cut at 256 bytes).
fn report_panic(info: &PanicInfo<'_>) {
    let name = component_name();
    let message = info.message();
    match info.location() {
        Some(location) => send_last_line(format_args!(
            "PANICKED: {name}: {message} at {}:{}",
            location.file(),
            location.line()
        )),
        None => send_last_line(format_args!("PANICKED: {name}: {message}")),
    };
}

/// The component's fault handler, which Kickstart installs at
/// `KeySlot::FAULT_HANDLER` (`libdevice_driver::FAULT_HANDLER_EXPORT`). It runs
/// on the faulting Thread, on a stack of its own, logs the `FAULT:` line
/// (on the semihosting console too, under QEMU) and stops the Thread.
extern "C" fn fault_body(
    _lent_key: u64,
    _badge: u64,
    kind: u64,
    syndrome: u64,
    fault_address: u64,
    pc: u64,
    sp: u64,
    _depth: u64,
) -> PpcResult {
    // SAFETY: Kickstart installs this handler only in components it loaded,
    // with their init record at `INIT_VA`.
    let init = unsafe { driver_init(INIT_VA) };
    export::init_return_key(&ThreadReturnKey::from_key(RawKey::from_wire(
        init.return_key,
    )));
    let mut thread_stacks = [(0, 0); 1 + libdevice_driver::MAX_FURTHER_THREADS];
    for (number, span) in init.thread_stacks() {
        if let Some(record) = thread_stacks.get_mut(number) {
            *record = (span.bottom, span.top);
        }
    }
    let report = FaultReport {
        component: init.name(),
        kind,
        syndrome,
        fault_address,
        pc,
        sp,
        thread_stacks: &thread_stacks,
        stack_region: (STACK_REGION, STACK_REGION + STACK_REGION_BYTES),
    };
    libuser::semihosting::println!("{report}");
    send_last_line(format_args!("{report}"));
    PpcResult {
        r0: FaultAction::Terminate as u64,
        r1: 0,
    }
}

ppc_export!(
    /// The fault handler's entry (`libdevice_driver::FAULT_HANDLER_EXPORT`).
    pub fn vesper_fault_entry => fault_body
);

/// A line at `level` on the semihosting console (under QEMU) and the log
/// channel.
pub fn line(level: Level, arguments: fmt::Arguments<'_>) {
    libuser::semihosting::println!("{arguments}");
    send(Some(level), arguments);
}

/// `log_println!("text {value}")`: one line at `Info` on the semihosting
/// console (under QEMU) and the log channel. Never waits.
#[macro_export]
macro_rules! log_println {
    ($($argument:tt)*) => {
        $crate::line($crate::Level::Info, format_args!($($argument)*))
    };
}

/// The `log` crate's records as lines on the log channel, each starting with
/// the component's name: `wood: text`, at the record's level. Under QEMU also
/// on the semihosting console as `wood: INFO text`.
pub struct Logger {
    name: &'static str,
}

impl Logger {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    /// Make `self` the `log` crate's logger, passing records from
    /// `max_level` up. A logger set already stays.
    pub fn install(&'static self, max_level: log::LevelFilter) {
        let _installed = log::set_logger(self);
        log::set_max_level(max_level);
    }
}

impl log::Log for Logger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let text = DropTrailingNewline(record.args());
        libuser::semihosting::println!("{}: {} {text}", self.name, record.level());
        let level = Level::from_number(record.level() as u64);
        send(level, format_args!("{}: {text}", self.name));
    }

    fn flush(&self) {}
}

/// Formats its arguments without the line feeds they end with (the engine
/// ends some messages with one of its own).
struct DropTrailingNewline<'record>(&'record fmt::Arguments<'record>);

impl fmt::Display for DropTrailingNewline<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.as_str() {
            Some(text) => formatter.write_str(text.trim_end_matches('\n')),
            None => self.0.fmt(formatter),
        }
    }
}
