use {
    super::{IrqControlKey, IrqControlOp, IrqHandlerKey, IrqHandlerOp, IssuedIrq},
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, KeyTableKey, RawKey},
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

// Mirrors `libsyscall::protected_call0`.
pub(super) unsafe fn protected_call0(key: u64, op: u64) -> Response {
    respond((key, op, [0; 6], 0))
}

// Mirrors `libsyscall::protected_call2`.
pub(super) unsafe fn protected_call2(key: u64, op: u64, a0: u64, a1: u64) -> Response {
    respond((key, op, [a0, a1, 0, 0, 0, 0], 2))
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

fn arm(response: Response) {
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
}

fn take_request() -> Request {
    REQUEST.with(|request| request.take().expect("missing syscall request"))
}

fn no_request() {
    REQUEST.with(|request| assert!(request.take().is_none(), "unexpected syscall"));
}

fn irq_control() -> IrqControlKey {
    IrqControlKey::from_key(RawKey::new(KeySlot::IRQ_CONTROL, 1))
}

fn handler() -> IrqHandlerKey {
    IrqHandlerKey::from_key(RawKey::new(KeySlot(40), 2))
}

#[test]
fn irq_op_ids_match_the_wire_baseline() {
    assert_eq!(IrqControlOp::Issue as u64, 0);
    assert_eq!(
        IrqControlOp::try_from(0).map_err(CapError::code),
        Ok(IrqControlOp::Issue)
    );
    assert_eq!(
        IrqControlOp::try_from(1).map_err(CapError::code),
        Err(CapError::InvalidOperation.code())
    );
    for (id, op) in [
        (0, IrqHandlerOp::SetNotification),
        (1, IrqHandlerOp::Clear),
        (2, IrqHandlerOp::Ack),
        (3, IrqHandlerOp::Retire),
        (4, IrqHandlerOp::Query),
    ] {
        assert_eq!(op as u64, id);
        assert_eq!(IrqHandlerOp::try_from(id).map_err(CapError::code), Ok(op));
    }
    for id in [5, 255, u64::MAX] {
        assert_eq!(
            IrqHandlerOp::try_from(id).map_err(CapError::code),
            Err(CapError::InvalidOperation.code())
        );
    }
}

#[test]
fn issue_packs_the_destination_count_controller_and_cells() {
    let table = KeyTableKey::from_key(RawKey::new(KeySlot::SELF_KEYTABLE, 1));
    let issued_key = RawKey::new(KeySlot(40), 1);
    arm((0, issued_key.to_wire(), 153));
    let issued = irq_control()
        .issue(&table, 40, 1, &[0, 121, 4])
        .map_err(CapError::code)
        .unwrap();
    assert_eq!(
        issued,
        IssuedIrq {
            handler: issued_key,
            line: 153
        }
    );
    let (key, op, args, len) = take_request();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot::IRQ_CONTROL, 1).to_wire());
    assert_eq!(op, IrqControlOp::Issue as u64);
    assert_eq!(args, [table.to_wire(), 40, 3 | (1 << 8), 0, 121, 4]);

    // Unused cells go out as zero.
    arm((0, issued_key.to_wire(), 3));
    irq_control()
        .issue(&table, 41, 0, &[3])
        .map_err(CapError::code)
        .unwrap();
    let (_, _, args, _) = take_request();
    assert_eq!(args, [table.to_wire(), 41, 1, 3, 0, 0]);
}

#[test]
fn issue_rejects_bad_cell_counts_without_a_syscall() {
    let table = KeyTableKey::from_key(RawKey::new(KeySlot::SELF_KEYTABLE, 1));
    for cells in [&[][..], &[1, 2, 3, 4][..]] {
        let error = irq_control().issue(&table, 40, 0, cells).unwrap_err();
        assert!(matches!(error, CapError::InvalidOperation));
        no_request();
    }
}

#[test]
fn issue_decodes_an_active_line() {
    let table = KeyTableKey::from_key(RawKey::new(KeySlot::SELF_KEYTABLE, 1));
    arm(CapError::IrqLineActive { line: 30 }.code());
    let error = irq_control()
        .issue(&table, 40, 0, &[1, 14, 0xf08])
        .unwrap_err();
    take_request();
    assert!(matches!(error, CapError::IrqLineActive { line: 30 }));
}

#[test]
fn handler_operations_encode_their_operands() {
    let (notification, receiver) = (RawKey::new(KeySlot(41), 1), RawKey::new(KeySlot(42), 3));
    arm((0, 0, 0));
    handler()
        .set_notification(notification, receiver)
        .map_err(CapError::code)
        .unwrap();
    assert_eq!(
        take_request(),
        (
            handler().to_wire(),
            IrqHandlerOp::SetNotification as u64,
            [notification.to_wire(), receiver.to_wire(), 0, 0, 0, 0],
            2
        )
    );
    for (op, call) in [
        (
            IrqHandlerOp::Clear,
            IrqHandlerKey::clear as fn(&IrqHandlerKey) -> _,
        ),
        (IrqHandlerOp::Ack, IrqHandlerKey::ack),
        (IrqHandlerOp::Retire, IrqHandlerKey::retire),
    ] {
        arm((0, 0, 0));
        call(&handler()).map_err(CapError::code).unwrap();
        assert_eq!(take_request(), (handler().to_wire(), op as u64, [0; 6], 0));
    }

    arm(CapError::InsufficientRights.code());
    let error = handler().retire().unwrap_err();
    take_request();
    assert!(matches!(error, CapError::InsufficientRights));
}

#[test]
fn query_decodes_the_count_and_the_time_spent_handling_interrupts() {
    arm((0, 17, 4_200_000));
    let stats = handler().query().map_err(CapError::code).unwrap();
    assert_eq!(
        take_request(),
        (handler().to_wire(), IrqHandlerOp::Query as u64, [0; 6], 0)
    );
    assert_eq!(stats.handled, 17);
    assert_eq!(stats.nanoseconds, 4_200_000);
}
