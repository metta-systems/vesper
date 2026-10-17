use {
    super::{SchedulerKey, ThreadStart},
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey, scheduler::SchedulerOp},
};

type Response = (u64, u64, u64);
type Request = (u64, u64, [u64; 6], usize);

std::thread_local! {
    static RESPONSE: Cell<Option<Response>> = const { Cell::new(None) };
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

fn respond(request: Request) -> Response {
    REQUEST.with(|recorded| assert!(recorded.replace(Some(request)).is_none()));
    RESPONSE.with(|response| response.take().expect("unexpected syscall"))
}

// Mirrors `libsyscall::protected_call1`.
pub(super) unsafe fn protected_call1(key: u64, op: u64, a0: u64) -> Response {
    respond((key, op, [a0, 0, 0, 0, 0, 0], 1))
}

// Mirrors `libsyscall::protected_call2`.
pub(super) unsafe fn protected_call2(key: u64, op: u64, a0: u64, a1: u64) -> Response {
    respond((key, op, [a0, a1, 0, 0, 0, 0], 2))
}

// Mirrors `libsyscall::protected_call3`.
pub(super) unsafe fn protected_call3(key: u64, op: u64, a0: u64, a1: u64, a2: u64) -> Response {
    respond((key, op, [a0, a1, a2, 0, 0, 0], 3))
}

// Mirrors `libsyscall::protected_call6`.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn protected_call6(
    key: u64,
    op: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) -> Response {
    respond((key, op, [a0, a1, a2, a3, a4, a5], 6))
}

fn root_scheduler() -> SchedulerKey {
    SchedulerKey::from_key(RawKey::new(KeySlot::ROOT_SCHEDULER, 1))
}

fn arm(response: Response) {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
}

fn take_request() -> Request {
    REQUEST.with(|request| request.take().expect("missing syscall request"))
}

#[test]
fn scheduler_op_ids_match_the_wire_baseline() {
    assert_eq!(SchedulerOp::ShareRegion as u64, 0);
    assert_eq!(SchedulerOp::ConfigureThread as u64, 1);
    assert_eq!(SchedulerOp::StartThread as u64, 2);
    assert_eq!(SchedulerOp::AssignRecord as u64, 3);
    assert_eq!(SchedulerOp::BindEventCount as u64, 4);
    for (id, op) in [
        (0, SchedulerOp::ShareRegion),
        (1, SchedulerOp::ConfigureThread),
        (2, SchedulerOp::StartThread),
        (3, SchedulerOp::AssignRecord),
        (4, SchedulerOp::BindEventCount),
    ] {
        assert_eq!(SchedulerOp::try_from(id).map_err(CapError::code), Ok(op));
    }
    for id in [5, 255, u64::MAX] {
        assert_eq!(
            SchedulerOp::try_from(id).map_err(CapError::code),
            Err(CapError::InvalidOperation.code())
        );
    }
}

#[test]
fn configure_thread_encodes_all_six_operands() {
    arm((0, 0, 0));
    let (thread, address_space, time) = (
        RawKey::new(KeySlot(40), 1),
        RawKey::new(KeySlot(2), 1),
        RawKey::new(KeySlot::BOOT_TIME, 1),
    );
    root_scheduler()
        .configure_thread(
            thread,
            address_space,
            ThreadStart {
                entry: 0x40_0000,
                stack_pointer: 0x7f_0000,
                argument: 0xA5,
            },
            time,
        )
        .map_err(CapError::code)
        .unwrap();
    let (key, op, args, len) = take_request();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot::ROOT_SCHEDULER, 1).to_wire());
    assert_eq!(op, SchedulerOp::ConfigureThread as u64);
    assert_eq!(
        args,
        [
            thread.to_wire(),
            address_space.to_wire(),
            0x40_0000,
            0x7f_0000,
            time.to_wire(),
            0xA5
        ]
    );
}

#[test]
fn start_thread_encodes_the_thread_key_and_decodes_errors() {
    arm((0, 0, 0));
    let thread = RawKey::new(KeySlot(40), 1);
    root_scheduler()
        .start_thread(thread)
        .map_err(CapError::code)
        .unwrap();
    let (_, op, args, len) = take_request();
    assert_eq!(
        (op, args, len),
        (
            SchedulerOp::StartThread as u64,
            [thread.to_wire(), 0, 0, 0, 0, 0],
            1
        )
    );

    arm(CapError::InvalidOperation.code());
    let error = root_scheduler().start_thread(thread).unwrap_err();
    take_request();
    assert!(matches!(error, CapError::InvalidOperation));
}

#[test]
fn share_region_encodes_frame_first_slot_and_stride() {
    arm((0, 0, 0));
    let frame = RawKey::new(KeySlot(41), 2);
    root_scheduler()
        .share_region(frame, 0, 128)
        .map_err(CapError::code)
        .unwrap();
    let (key, op, args, len) = take_request();
    assert_eq!(len, 3);
    assert_eq!(key, RawKey::new(KeySlot::ROOT_SCHEDULER, 1).to_wire());
    assert_eq!(op, SchedulerOp::ShareRegion as u64);
    assert_eq!(args, [frame.to_wire(), 0, 128, 0, 0, 0]);
}

#[test]
fn assign_record_encodes_thread_and_slot() {
    arm((0, 0, 0));
    let thread = RawKey::new(KeySlot(40), 1);
    root_scheduler()
        .assign_record(thread, 7)
        .map_err(CapError::code)
        .unwrap();
    let (_, op, args, len) = take_request();
    assert_eq!(len, 2);
    assert_eq!(op, SchedulerOp::AssignRecord as u64);
    assert_eq!(args, [thread.to_wire(), 7, 0, 0, 0, 0]);
}

#[test]
fn bind_event_count_encodes_the_event_count() {
    arm((0, 0, 0));
    let event_count = RawKey::new(KeySlot(42), 3);
    root_scheduler()
        .bind_event_count(event_count)
        .map_err(CapError::code)
        .unwrap();
    let (_, op, args, len) = take_request();
    assert_eq!(len, 1);
    assert_eq!(op, SchedulerOp::BindEventCount as u64);
    assert_eq!(args, [event_count.to_wire(), 0, 0, 0, 0, 0]);
}
