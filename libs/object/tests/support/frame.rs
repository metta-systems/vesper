use {
    super::{FrameAttribute, FrameKey},
    std::cell::Cell,
    vesper_objects::{CapError, KeySlot, RawKey, Rights},
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

// Mirrors `libsyscall::protected_call6`, including its lint exemption.
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

/// Issue `FrameKey::map` with the recorded key and return the captured
/// request, or the kernel error decoded from `response`.
pub(super) fn invoke_map(response: Response) -> Result<Request, CapError> {
    invoke_map_with(response, FrameAttribute::Default)
}

/// Issue `FrameKey::map` with `attribute`.
pub(super) fn invoke_map_with(
    response: Response,
    attribute: FrameAttribute,
) -> Result<Request, CapError> {
    let frame_key = RawKey::new(KeySlot(9), 0x0246_8ace);
    let as_key = RawKey::new(KeySlot(1), 1);
    let frame = FrameKey::from_key(frame_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = frame.map(as_key, 0x0000_0040_0000, Rights(Rights::READ), attribute);
    captured(result)
}

/// Issue `FrameKey::unmap` with the recorded key and return the captured
/// request, or the kernel error decoded from `response`.
pub(super) fn invoke_unmap(response: Response) -> Result<Request, CapError> {
    let frame_key = RawKey::new(KeySlot(9), 0x0246_8ace);
    let frame = FrameKey::from_key(frame_key);
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = frame.unmap();
    captured(result)
}

fn captured(result: Result<(), CapError>) -> Result<Request, CapError> {
    // Drain the recorded request on both paths: the transport always records
    // one, including error responses.
    let request = REQUEST.with(|request| request.take().expect("missing syscall request"));
    result.map(|()| request)
}

#[test]
fn frame_map_preserves_request_encoding_and_accepts_success() {
    let (key, op, args, len) = invoke_map((0, 0, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot(9), 0x0246_8ace).to_wire());
    assert_eq!(op, vesper_objects::FrameOp::Map as u64);
    assert_eq!(
        args,
        [
            RawKey::new(KeySlot(1), 1).to_wire(),
            0x0000_0040_0000,
            u64::from(Rights::READ),
            0,
            0,
            0,
        ]
    );
}

#[test]
fn frame_map_encodes_the_attribute_in_x5() {
    for (attribute, code) in [
        (FrameAttribute::Default, 0),
        (FrameAttribute::Cacheable, 1),
        (FrameAttribute::WriteCombining, 2),
        (FrameAttribute::DevicePosted, 3),
        (FrameAttribute::DeviceStrict, 4),
    ] {
        let (_, _, args, _) = invoke_map_with((0, 0, 0), attribute)
            .map_err(CapError::code)
            .unwrap();
        assert_eq!(args[3], code, "{attribute:?}");
        assert_eq!(FrameAttribute::from_code(code), Some(attribute));
    }
    assert_eq!(FrameAttribute::from_code(5), None);
    assert_eq!(FrameAttribute::from_code(u64::MAX), None);
}

#[test]
fn frame_attributes_are_ordered_strictest_first_and_index_mair() {
    use FrameAttribute::{Cacheable, Default, DevicePosted, DeviceStrict, WriteCombining};
    let chain = [DeviceStrict, DevicePosted, WriteCombining, Cacheable];
    for (rank, attribute) in chain.into_iter().enumerate() {
        assert_eq!(attribute.freedom(), Some(u8::try_from(rank).unwrap()));
        // MAIR indices run the other way: 0 is the loosest.
        assert_eq!(
            attribute.mair_index(),
            Some(u8::try_from(3 - rank).unwrap())
        );
    }
    assert_eq!(Default.freedom(), None);
    assert_eq!(Default.mair_index(), None);
    // A device ceiling admits itself and everything stricter, never `Cacheable`.
    assert!(DeviceStrict.within_device_ceiling(WriteCombining));
    assert!(DevicePosted.within_device_ceiling(WriteCombining));
    assert!(WriteCombining.within_device_ceiling(WriteCombining));
    assert!(!WriteCombining.within_device_ceiling(DevicePosted));
    assert!(!WriteCombining.within_device_ceiling(DeviceStrict));
    assert!(!Cacheable.within_device_ceiling(WriteCombining));
    assert!(!Default.within_device_ceiling(WriteCombining));
    assert!(!DeviceStrict.within_device_ceiling(Default));
}

#[test]
fn frame_unmap_preserves_request_encoding_and_accepts_success() {
    let (key, op, args, len) = invoke_unmap((0, 0, 0)).map_err(CapError::code).unwrap();
    assert_eq!(len, 6);
    assert_eq!(key, RawKey::new(KeySlot(9), 0x0246_8ace).to_wire());
    assert_eq!(op, vesper_objects::FrameOp::Unmap as u64);
    assert_eq!(args, [0, 0, 0, 0, 0, 0]);
}

#[test]
fn frame_wrappers_preserve_kernel_errors() {
    assert!(matches!(
        invoke_map((8, 0, 0)),
        Err(CapError::AlreadyMapped)
    ));
    assert!(matches!(
        invoke_map((30, 0x2000_0000, 0)),
        Err(CapError::MissingIntermediate { vaddr: 0x2000_0000 })
    ));
    assert!(matches!(
        invoke_map((31, 0x0040_0000, 0)),
        Err(CapError::PhysicalAlias { paddr: 0x0040_0000 })
    ));
    assert!(matches!(invoke_unmap((7, 0, 0)), Err(CapError::NotMapped)));
    assert!(matches!(
        invoke_map((u64::MAX, 42, 99)),
        Err(CapError::UnknownResponse { .. })
    ));
}

/// One `FrameKey` cache-maintenance wrapper.
type CacheCall = fn(&FrameKey, u64, u64) -> Result<(), CapError>;

/// Issue one cache-maintenance wrapper over bytes `0x40..0x1000` and return
/// the captured request, or the kernel error decoded from `response`.
fn invoke_cache(response: Response, call: CacheCall) -> Result<Request, CapError> {
    let frame = FrameKey::from_key(RawKey::new(KeySlot(9), 0x0246_8ace));
    RESPONSE.with(|pending| assert!(pending.replace(Some(response)).is_none()));
    let result = call(&frame, 0x40, 0x1000);
    captured(result)
}

#[test]
fn frame_cache_maintenance_preserves_request_encoding() {
    use vesper_objects::FrameOp;

    let calls: [(CacheCall, FrameOp); 4] = [
        (FrameKey::clean_data, FrameOp::CleanData),
        (FrameKey::invalidate_data, FrameOp::InvalidateData),
        (
            FrameKey::clean_invalidate_data,
            FrameOp::CleanInvalidateData,
        ),
        (FrameKey::unify_instruction, FrameOp::UnifyInstruction),
    ];
    for (call, expected) in calls {
        let (key, op, args, len) = invoke_cache((0, 0, 0), call)
            .map_err(CapError::code)
            .unwrap();
        assert_eq!(len, 6);
        assert_eq!(key, RawKey::new(KeySlot(9), 0x0246_8ace).to_wire());
        assert_eq!(op, expected as u64);
        assert_eq!(args, [0x40, 0x1000, 0, 0, 0, 0]);
    }
}

#[test]
fn frame_cache_maintenance_preserves_kernel_errors() {
    assert!(matches!(
        invoke_cache((9, 0, 0), FrameKey::clean_data),
        Err(CapError::InvalidOperation)
    ));
    assert!(matches!(
        invoke_cache((6, 0, 0), FrameKey::invalidate_data),
        Err(CapError::InsufficientRights)
    ));
}
