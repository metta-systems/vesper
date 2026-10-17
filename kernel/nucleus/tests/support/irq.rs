//! `IRQControl.Issue` and `IRQHandler` operations through production
//! dispatch on the serial QEMU nucleus fixture, against a fake
//! interrupt-controller component that records masks and triggers.
//! Notifications, Times and Threads are carved from the unit-test arena.

use {
    super::{FIXTURE_GUARD, cap_invoke, with_nucleus},
    crate::{
        api::{self, KeyEntry},
        objects::{
            ArchObjectsImpl, ExecutionContext, Notification, Nucleus, Thread, Time,
            arch_objects::AddressSpaceId,
            carved::CarvedId,
            irq::{IrqLine, IrqStats, IrqTable},
            nucleus::test_arena,
        },
        platform,
    },
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicU64, Ordering},
    },
    libirqchip::{
        ABI_VERSION, Controller, INVALID_LINE, IrqChipConfig, IrqChipOps, MAX_CONTROLLERS,
        PlatformHandoff, SPURIOUS, Specifier, TRIGGER_EDGE, TRIGGER_FIXED, TRIGGER_LEVEL,
    },
    libobject::{
        CapError, InconsistencyReason, KeySlot, NotificationOp, RawKey, Rights,
        irq::{IrqControlOp, IrqHandlerOp},
    },
};

// ── The fake component ──────────────────────────────────────────────────

/// Lines the fake component numbers: `0..32` on controller 0, `32..64` on
/// controller 1.
const LINES: u32 = 64;
/// The fake physical timer line.
const TIMER_LINE: u32 = 1;
/// Lines from here up refuse `set_trigger`.
const FIXED_FROM: u32 = 24;

/// Masked lines, one bit each.
static MASKED: AtomicU64 = AtomicU64::new(0);
/// Lines configured level triggered, one bit each.
static LEVEL: AtomicU64 = AtomicU64::new(0);
/// Lines configured edge triggered, one bit each.
static EDGE: AtomicU64 = AtomicU64::new(0);

fn bit(line: u32) -> u64 {
    1 << line
}

fn is_masked(line: u32) -> bool {
    MASKED.load(Ordering::Relaxed) & bit(line) != 0
}

unsafe extern "C" fn init(_config: *const IrqChipConfig) -> i32 {
    0
}

unsafe extern "C" fn claim() -> u32 {
    SPURIOUS
}

unsafe extern "C" fn complete(_line: u32) {}

unsafe extern "C" fn mask(line: u32) {
    MASKED.fetch_or(bit(line), Ordering::Relaxed);
}

unsafe extern "C" fn unmask(line: u32) {
    MASKED.fetch_and(!bit(line), Ordering::Relaxed);
}

unsafe extern "C" fn set_trigger(line: u32, level: u32) -> i32 {
    if line >= FIXED_FROM {
        return -1;
    }
    let (set, clear) = if level != 0 {
        (&LEVEL, &EDGE)
    } else {
        (&EDGE, &LEVEL)
    };
    set.fetch_or(bit(line), Ordering::Relaxed);
    clear.fetch_and(!bit(line), Ordering::Relaxed);
    0
}

/// Kind 0: `<n flags>`, line `n` below 32, flags 1 edge and 4 level. Kind 1:
/// `<n>`, line `32 + n`, trigger fixed.
unsafe extern "C" fn xlate(kind: u32, cells: *const u32, count: u32, trigger: *mut u32) -> u32 {
    // SAFETY: the caller passes `count` readable cells.
    let cells = unsafe { core::slice::from_raw_parts(cells, count as usize) };
    let (line, kind_trigger) = match (kind, cells) {
        (0, [n, flags]) if *n < 32 => (
            *n,
            match flags {
                1 => TRIGGER_EDGE,
                4 => TRIGGER_LEVEL,
                _ => TRIGGER_FIXED,
            },
        ),
        (1, [n]) if *n < 32 => (32 + n, TRIGGER_FIXED),
        _ => return INVALID_LINE,
    };
    // SAFETY: the caller passes a writable trigger word.
    unsafe {
        trigger.write(kind_trigger);
    }
    line
}

static OPS: IrqChipOps = IrqChipOps {
    abi_version: ABI_VERSION,
    line_count: LINES,
    init,
    claim,
    complete,
    mask,
    unmask,
    set_trigger,
    xlate,
};

/// Install the fake component once for the whole test binary.
fn install_fake_component() {
    if platform::is_installed() {
        return;
    }
    let mut controllers = [Controller::default(); MAX_CONTROLLERS];
    controllers[1].kind = 1;
    let handoff = PlatformHandoff {
        ops: (&raw const OPS) as u64,
        config: IrqChipConfig {
            controller_count: 2,
            controllers,
        },
        physical_timer: Specifier {
            kind: 0,
            cell_count: 2,
            cells: [TIMER_LINE, 4, 0, 0],
        },
    };
    // SAFETY: the fake component touches only its own statics.
    let timer = unsafe { platform::install(&handoff) }
        .unwrap_or_else(|error| panic!("fake component refused: {error:?}"));
    assert_eq!(timer, TIMER_LINE);
}

// ── Fixture ─────────────────────────────────────────────────────────────

/// Records for the fake component's lines, reinitialized by each test.
struct Lines(UnsafeCell<[IrqLine; LINES as usize]>);

// SAFETY: tests run sequentially on one core.
unsafe impl Sync for Lines {}

static LINE_RECORDS: Lines = Lines(UnsafeCell::new([IrqLine::FREE; LINES as usize]));

const IRQ_CONTROL_SLOT: u32 = 20;
const NOTIFICATION_SLOT: u32 = 21;
const RECEIVER_SLOT: u32 = 22;
/// Where `Issue` installs handlers: consecutive slots from here.
const HANDLER_SLOT: u32 = 30;
/// The fixture table's self key, the Issue destination.
const SELF_TABLE_SLOT: u32 = 23;

/// Keys installed in the caller's table.
struct Keys {
    control: RawKey,
    table: RawKey,
    notification: RawKey,
    receiver: RawKey,
}

fn install(nucleus: &mut Nucleus<ArchObjectsImpl>, slot: u32, entry: KeyEntry) -> RawKey {
    nucleus
        .current_thread_table_mut()
        .unwrap()
        .insert(KeySlot(slot), entry, FIXTURE_GUARD)
        .unwrap_or_else(|failure| panic!("fixture install failed: {:?}", failure.error.code()))
}

/// A caller Thread in `fixture_as`, a fresh IRQ table over the fake
/// component, and the keys the IRQ operations need: the Notification with
/// `badge`, and a receiver Thread that owns a Time.
fn setup(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    table_addr: u64,
    fixture_as: AddressSpaceId,
    badge: u16,
) -> Keys {
    install_fake_component();
    MASKED.store(0, Ordering::Relaxed);
    LEVEL.store(0, Ordering::Relaxed);
    EDGE.store(0, Ordering::Relaxed);
    // SAFETY: the records are exclusively this fixture's; tests are sequential.
    nucleus.irq_table =
        unsafe { IrqTable::initialize(LINE_RECORDS.0.get().cast::<IrqLine>(), LINES) };
    nucleus
        .create_console_thread(fixture_as)
        .expect("caller Thread");
    nucleus.current_thread = nucleus.fixture_thread(0);

    let notification = test_arena::carve(Notification::new()).expect("arena exhausted");
    let time = test_arena::carve(Time::empty(0)).expect("arena exhausted");
    let receiver = test_arena::carve(Thread::new(CarvedId::UNBOUND, ExecutionContext::Inactive))
        .expect("arena exhausted");
    nucleus.thread_mut(receiver).unwrap().time = Some(time);
    let (guard, size_bits) = nucleus
        .current_thread_table()
        .unwrap()
        .self_table_capability()
        .map(|(_, guard, size_bits)| (guard, size_bits))
        .unwrap();
    Keys {
        control: install(
            nucleus,
            IRQ_CONTROL_SLOT,
            KeyEntry::new_irq_control(Rights::all()),
        ),
        table: install(
            nucleus,
            SELF_TABLE_SLOT,
            KeyEntry::new_keytable(
                super::table_id(table_addr),
                guard,
                size_bits,
                Rights::all(),
                0,
            ),
        ),
        notification: install(
            nucleus,
            NOTIFICATION_SLOT,
            KeyEntry::new_carved(notification, Rights::all(), badge),
        ),
        receiver: install(
            nucleus,
            RECEIVER_SLOT,
            KeyEntry::new_thread(receiver, Rights::all(), 0),
        ),
    }
}

fn issue_raw(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    keys: &Keys,
    slot: u32,
    shape: u64,
    cells: [u64; 3],
) -> Result<(RawKey, u32), CapError> {
    match cap_invoke(
        nucleus,
        keys.control,
        IrqControlOp::Issue as u64,
        &[
            keys.table.to_wire(),
            u64::from(slot),
            shape,
            cells[0],
            cells[1],
            cells[2],
        ],
    )? {
        api::InvokeOutcome::Complete((key, line)) => {
            Ok((RawKey::from_wire(key), u32::try_from(line).unwrap()))
        }
        _ => panic!("Issue did not complete"),
    }
}

/// Issue `<line flags>` on controller 0 into `slot`.
fn issue(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    keys: &Keys,
    slot: u32,
    line: u32,
    flags: u32,
) -> Result<RawKey, CapError> {
    let (key, issued) = issue_raw(
        nucleus,
        keys,
        slot,
        2,
        [u64::from(line), u64::from(flags), 0],
    )?;
    assert_eq!(issued, line);
    Ok(key)
}

fn handler_op(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    handler: RawKey,
    op: IrqHandlerOp,
    args: [u64; 6],
) -> Result<(), CapError> {
    cap_invoke(nucleus, handler, op as u64, &args).map(|_| ())
}

fn set_notification(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    handler: RawKey,
    notification: RawKey,
    receiver: RawKey,
) -> Result<(), CapError> {
    handler_op(
        nucleus,
        handler,
        IrqHandlerOp::SetNotification,
        [notification.to_wire(), receiver.to_wire(), 0, 0, 0, 0],
    )
}

/// Deliver `line` the way the interrupt path does.
fn deliver(nucleus: &mut Nucleus<ArchObjectsImpl>, line: u32) -> bool {
    // SAFETY: tests are serial and no other access context is live.
    let access = unsafe { crate::objects::access::Access::new() };
    api::irq::deliver(&access, line, nucleus)
}

/// Consume the Notification's pending bits.
fn poll(nucleus: &mut Nucleus<ArchObjectsImpl>, notification: RawKey) -> u64 {
    match cap_invoke(nucleus, notification, NotificationOp::Poll as u64, &[0; 6]) {
        Ok(api::InvokeOutcome::Complete((bits, _))) => bits,
        _ => panic!("Poll failed"),
    }
}

fn is_retired_key(error: &CapError) -> bool {
    matches!(
        error,
        CapError::InconsistentKey {
            reason: InconsistencyReason::ObjectRetired,
            ..
        }
    )
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test_case]
fn issue_installs_a_masked_handler_with_the_specifier_trigger() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue level line: {:?}", error.code()));
        assert!(is_masked(5), "an issued line stays masked until Ack");
        assert_ne!(LEVEL.load(Ordering::Relaxed) & bit(5), 0);
        issue(nucleus, &keys, HANDLER_SLOT + 1, 6, 1)
            .unwrap_or_else(|error| panic!("Issue edge line: {:?}", error.code()));
        assert_ne!(EDGE.load(Ordering::Relaxed) & bit(6), 0);
        // A cascaded controller's specifier: controller index 1 in bits 15:8.
        let (_, line) = issue_raw(nucleus, &keys, HANDLER_SLOT + 2, 1 | (1 << 8), [3, 0, 0])
            .unwrap_or_else(|error| panic!("Issue on the cascaded controller: {:?}", error.code()));
        assert_eq!(line, 35);
        assert!(is_masked(35));
    });
}

#[test_case]
fn issue_refuses_active_and_timer_lines() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT + 1, 5, 4),
            Err(CapError::IrqLineActive { line: 5 })
        ));
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT + 1, TIMER_LINE, 4),
            Err(CapError::IrqLineActive { line: TIMER_LINE })
        ));
        // Neither refusal installed anything.
        assert!(issue(nucleus, &keys, HANDLER_SLOT + 1, 7, 4).is_ok());
    });
}

#[test_case]
fn issue_rejects_malformed_requests_and_changes_nothing() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        for (shape, cells) in [
            // No cells, too many cells.
            (0, [0, 0, 0]),
            (4, [5, 4, 0]),
            // A controller the component does not have.
            (2 | (2 << 8), [5, 4, 0]),
            // Reserved bits above the controller.
            (2 | (1 << 16), [5, 4, 0]),
            // A cell beyond the count that is not zero.
            (2, [5, 4, 9]),
            // A cell wider than 32 bits.
            (2, [1 << 32, 4, 0]),
            // A specifier the component cannot decode.
            (2, [40, 4, 0]),
        ] {
            assert!(matches!(
                issue_raw(nucleus, &keys, HANDLER_SLOT, shape, cells),
                Err(CapError::InvalidOperation)
            ));
        }
        // An occupied destination: the line stays free.
        issue(nucleus, &keys, HANDLER_SLOT, 9, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT, 10, 4),
            Err(CapError::SlotOccupied(_))
        ));
        assert!(!is_masked(10), "a refused Issue leaves the line alone");
        assert!(issue(nucleus, &keys, HANDLER_SLOT + 1, 10, 4).is_ok());
    });
}

#[test_case]
fn issue_refuses_an_unconfigurable_trigger() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        // The fake component refuses to configure lines from FIXED_FROM.
        let line = FIXED_FROM + 2;
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT, line, 4),
            Err(CapError::InvalidOperation)
        ));
        assert!(!is_masked(line), "a refused Issue leaves the line alone");
        // A specifier that leaves the trigger alone configures nothing.
        issue(nucleus, &keys, HANDLER_SLOT, line, 0)
            .unwrap_or_else(|error| panic!("Issue with a fixed trigger: {:?}", error.code()));
        assert_eq!(
            (LEVEL.load(Ordering::Relaxed) | EDGE.load(Ordering::Relaxed)) & bit(line),
            0
        );
    });
}

#[test_case]
fn issue_requires_grant_and_install() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let mut keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let read_only = install(nucleus, 24, KeyEntry::new_irq_control(Rights(Rights::READ)));
        let control = core::mem::replace(&mut keys.control, read_only);
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT, 5, 4),
            Err(CapError::InsufficientRights)
        ));
        keys.control = control;
        // A destination table without INSTALL.
        let (guard, size_bits) = nucleus
            .current_thread_table()
            .unwrap()
            .self_table_capability()
            .map(|(_, guard, size_bits)| (guard, size_bits))
            .unwrap();
        keys.table = install(
            nucleus,
            25,
            KeyEntry::new_keytable(
                super::table_id(table_addr),
                guard,
                size_bits,
                Rights(Rights::DERIVE),
                0,
            ),
        );
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT, 5, 4),
            Err(CapError::InsufficientRights)
        ));
        // Another kind in the destination-table operand.
        keys.table = keys.notification;
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT, 5, 4),
            Err(CapError::TypeMismatch { .. })
        ));
        assert!(!is_masked(5), "no refusal touched the line");
    });
}

#[test_case]
fn a_bound_line_signals_its_notification_until_cleared() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        // Unbound: delivery signals nothing.
        assert!(!deliver(nucleus, 5));
        set_notification(nucleus, handler, keys.notification, keys.receiver)
            .unwrap_or_else(|error| panic!("SetNotification: {:?}", error.code()));
        assert!(deliver(nucleus, 5));
        assert_eq!(poll(nucleus, keys.notification), 0x4);

        handler_op(nucleus, handler, IrqHandlerOp::Ack, [0; 6])
            .unwrap_or_else(|error| panic!("Ack: {:?}", error.code()));
        assert!(!is_masked(5), "Ack unmasks the line");

        handler_op(nucleus, handler, IrqHandlerOp::Clear, [0; 6])
            .unwrap_or_else(|error| panic!("Clear: {:?}", error.code()));
        assert!(!deliver(nucleus, 5));
        assert_eq!(poll(nucleus, keys.notification), 0);
        // Another line is never delivered through this binding.
        assert!(!deliver(nucleus, 6));
    });
}

#[test_case]
fn set_notification_checks_its_keys() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        let notification_id = nucleus
            .current_thread_table()
            .unwrap()
            .lookup(keys.notification, FIXTURE_GUARD)
            .and_then(KeyEntry::carved_id)
            .unwrap_or_else(|error| panic!("lookup: {:?}", error.code()));

        // An unbadged Notification has no bits to signal.
        let unbadged = install(
            nucleus,
            24,
            KeyEntry::new_carved(notification_id, Rights::all(), 0),
        );
        assert!(matches!(
            set_notification(nucleus, handler, unbadged, keys.receiver),
            Err(CapError::InvalidOperation)
        ));
        // A Notification key without SEND.
        let receive_only = install(
            nucleus,
            25,
            KeyEntry::new_carved(notification_id, Rights(Rights::RECV), 0x4),
        );
        assert!(matches!(
            set_notification(nucleus, handler, receive_only, keys.receiver),
            Err(CapError::InsufficientRights)
        ));
        // Another kind in the Notification operand.
        assert!(matches!(
            set_notification(nucleus, handler, keys.receiver, keys.receiver),
            Err(CapError::TypeMismatch { .. })
        ));
        // A receiver Thread without its own Time.
        let timeless =
            test_arena::carve(Thread::new(CarvedId::UNBOUND, ExecutionContext::Inactive))
                .expect("arena exhausted");
        let timeless = install(
            nucleus,
            26,
            KeyEntry::new_thread(timeless, Rights::all(), 0),
        );
        assert!(matches!(
            set_notification(nucleus, handler, keys.notification, timeless),
            Err(CapError::InvalidOperation)
        ));
        // A receiver key without WRITE.
        let receiver_id = nucleus
            .current_thread_table()
            .unwrap()
            .lookup(keys.receiver, FIXTURE_GUARD)
            .and_then(KeyEntry::thread_id)
            .unwrap_or_else(|error| panic!("lookup: {:?}", error.code()));
        let read_receiver = install(
            nucleus,
            27,
            KeyEntry::new_thread(receiver_id, Rights(Rights::READ), 0),
        );
        assert!(matches!(
            set_notification(nucleus, handler, keys.notification, read_receiver),
            Err(CapError::InsufficientRights)
        ));
        // Reserved operands.
        assert!(matches!(
            handler_op(
                nucleus,
                handler,
                IrqHandlerOp::SetNotification,
                [
                    keys.notification.to_wire(),
                    keys.receiver.to_wire(),
                    1,
                    0,
                    0,
                    0
                ]
            ),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            handler_op(nucleus, handler, IrqHandlerOp::Ack, [1, 0, 0, 0, 0, 0]),
            Err(CapError::InvalidOperation)
        ));
        // None of it bound the line.
        assert!(!deliver(nucleus, 5));
    });
}

/// A bound line refuses a second `SetNotification` with `AlreadyMapped` and
/// keeps delivering to its first binding; after `Clear` it can be bound again.
#[test_case]
fn set_notification_refuses_to_rebind_until_cleared() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        set_notification(nucleus, handler, keys.notification, keys.receiver)
            .unwrap_or_else(|error| panic!("bind: {:?}", error.code()));
        let bound = nucleus.irq_table.binding(5);
        assert!(bound.is_some());

        let rebind = set_notification(nucleus, handler, keys.notification, keys.receiver);
        assert_eq!(
            rebind.err().map(|error| error.code()),
            Some(CapError::AlreadyMapped.code())
        );
        assert_eq!(nucleus.irq_table.binding(5), bound);
        assert!(deliver(nucleus, 5));

        handler_op(nucleus, handler, IrqHandlerOp::Clear, [0; 6])
            .unwrap_or_else(|error| panic!("Clear: {:?}", error.code()));
        assert_eq!(nucleus.irq_table.binding(5), None);
        set_notification(nucleus, handler, keys.notification, keys.receiver)
            .unwrap_or_else(|error| panic!("rebind after Clear: {:?}", error.code()));
        assert_eq!(nucleus.irq_table.binding(5), bound);
    });
}

#[test_case]
fn a_retired_notification_leaves_the_line_unbound() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        set_notification(nucleus, handler, keys.notification, keys.receiver)
            .unwrap_or_else(|error| panic!("bind: {:?}", error.code()));
        let notification_id = nucleus
            .current_thread_table()
            .unwrap()
            .lookup(keys.notification, FIXTURE_GUARD)
            .and_then(KeyEntry::carved_id)
            .unwrap_or_else(|error| panic!("lookup: {:?}", error.code()));
        // SAFETY: an arena record no guard refers to.
        unsafe { crate::objects::carved::Carved::<Notification>::retire(notification_id) }
            .unwrap_or_else(|error| panic!("retire: {error:?}"));
        assert!(!deliver(nucleus, 5));
    });
}

#[test_case]
fn retire_invalidates_every_copy_and_frees_the_line() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        let entry = *nucleus
            .current_thread_table()
            .unwrap()
            .lookup(handler, FIXTURE_GUARD)
            .unwrap_or_else(|error| panic!("lookup: {:?}", error.code()));
        let copy = install(nucleus, 24, entry);
        set_notification(nucleus, handler, keys.notification, keys.receiver)
            .unwrap_or_else(|error| panic!("bind: {:?}", error.code()));
        handler_op(nucleus, handler, IrqHandlerOp::Ack, [0; 6])
            .unwrap_or_else(|error| panic!("Ack: {:?}", error.code()));

        // Retire needs RETIRE.
        let write_only = install(
            nucleus,
            25,
            KeyEntry::new_irq_handler(
                entry
                    .irq_handler_id()
                    .unwrap_or_else(|error| panic!("handler id: {:?}", error.code())),
                Rights(Rights::WRITE),
            ),
        );
        assert!(matches!(
            handler_op(nucleus, write_only, IrqHandlerOp::Retire, [0; 6]),
            Err(CapError::InsufficientRights)
        ));

        handler_op(nucleus, copy, IrqHandlerOp::Retire, [0; 6])
            .unwrap_or_else(|error| panic!("Retire: {:?}", error.code()));
        assert!(is_masked(5), "Retire masks the line");
        assert!(!deliver(nucleus, 5), "Retire drops the binding");
        for key in [handler, copy, write_only] {
            let error = handler_op(nucleus, key, IrqHandlerOp::Ack, [0; 6]).unwrap_err();
            assert!(is_retired_key(&error), "a stale handler key still works");
        }
        assert!(is_masked(5), "a stale Ack did not unmask");

        // The line is free again, under a new generation.
        let reissued = issue(nucleus, &keys, HANDLER_SLOT + 1, 5, 4)
            .unwrap_or_else(|error| panic!("reissue: {:?}", error.code()));
        handler_op(nucleus, reissued, IrqHandlerOp::Ack, [0; 6])
            .unwrap_or_else(|error| panic!("Ack: {:?}", error.code()));
        let error = handler_op(nucleus, handler, IrqHandlerOp::Clear, [0; 6]).unwrap_err();
        assert!(is_retired_key(&error));
    });
}

#[test_case]
fn an_exhausted_line_is_never_issued_again() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        nucleus.irq_table.set_free_generation(5, u32::MAX);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        handler_op(nucleus, handler, IrqHandlerOp::Retire, [0; 6])
            .unwrap_or_else(|error| panic!("Retire: {:?}", error.code()));
        assert!(matches!(
            issue(nucleus, &keys, HANDLER_SLOT + 1, 5, 4),
            Err(CapError::IrqLineActive { line: 5 })
        ));
        let error = handler_op(nucleus, handler, IrqHandlerOp::Ack, [0; 6]).unwrap_err();
        assert!(is_retired_key(&error));
    });
}

#[test_case]
fn interrupt_statistics_count_every_line_and_survive_retire() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        // A line with no handler is recorded too, like the nucleus's own
        // timer line and an unbound line.
        assert_eq!(nucleus.irq_table.stats(7), Some(IrqStats::default()));
        nucleus.account_interrupts(&[(7, 40), (7, 60), (5, 10)], 110);
        assert_eq!(
            nucleus.irq_table.stats(7),
            Some(IrqStats {
                handled: 2,
                ticks: 100
            })
        );
        assert_eq!(
            nucleus.irq_table.stats(5),
            Some(IrqStats {
                handled: 1,
                ticks: 10
            })
        );
        // A line beyond the table has no record and is ignored.
        nucleus.account_interrupts(&[(u32::MAX, 5)], 5);
        assert_eq!(nucleus.irq_table.stats(u32::MAX), None);

        // The statistics belong to the line: they survive Retire and a fresh
        // Issue, and keep counting.
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        handler_op(nucleus, handler, IrqHandlerOp::Retire, [0; 6])
            .unwrap_or_else(|error| panic!("Retire: {:?}", error.code()));
        issue(nucleus, &keys, HANDLER_SLOT + 1, 5, 4)
            .unwrap_or_else(|error| panic!("reissue: {:?}", error.code()));
        nucleus.account_interrupts(&[(5, 25)], 25);
        assert_eq!(
            nucleus.irq_table.stats(5),
            Some(IrqStats {
                handled: 2,
                ticks: 35
            })
        );
    });
}

#[test_case]
fn query_reports_the_lines_count_and_time_and_needs_read() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let keys = setup(nucleus, table_addr, fixture_as, 0x4);
        let handler = issue(nucleus, &keys, HANDLER_SLOT, 5, 4)
            .unwrap_or_else(|error| panic!("Issue: {:?}", error.code()));
        let query = |nucleus: &mut Nucleus<ArchObjectsImpl>, args: [u64; 6]| {
            cap_invoke(nucleus, handler, IrqHandlerOp::Query as u64, &args)
        };
        let Ok(crate::api::InvokeOutcome::Complete((count, nanoseconds))) = query(nucleus, [0; 6])
        else {
            panic!("Query completes");
        };
        assert_eq!((count, nanoseconds), (0, 0));

        // Two interrupts on the line, and one on another: only the line's own
        // are reported, with the time rounded up to nanoseconds.
        let frequency = nucleus.counter_frequency();
        nucleus.account_interrupts(
            &[(5, frequency / 1_000), (5, frequency / 500), (6, 99)],
            frequency / 1_000 + frequency / 500 + 99,
        );
        let Ok(crate::api::InvokeOutcome::Complete((count, nanoseconds))) = query(nucleus, [0; 6])
        else {
            panic!("Query completes");
        };
        assert_eq!(count, 2);
        assert_eq!(nanoseconds, 3_000_000);

        // An unused register must be zero.
        assert!(matches!(
            query(nucleus, [1, 0, 0, 0, 0, 0]),
            Err(CapError::InvalidOperation)
        ));
        // A copy without READ cannot ask.
        let entry = *nucleus
            .current_thread_table()
            .unwrap()
            .lookup(handler, FIXTURE_GUARD)
            .unwrap_or_else(|error| panic!("lookup: {:?}", error.code()));
        let write_only = install(
            nucleus,
            HANDLER_SLOT + 5,
            KeyEntry::new_irq_handler(
                entry
                    .irq_handler_id()
                    .unwrap_or_else(|error| panic!("handler id: {:?}", error.code())),
                Rights(Rights::WRITE),
            ),
        );
        assert!(matches!(
            cap_invoke(nucleus, write_only, IrqHandlerOp::Query as u64, &[0; 6]),
            Err(CapError::InsufficientRights)
        ));
    });
}
