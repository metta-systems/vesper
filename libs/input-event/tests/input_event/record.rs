use {
    core::mem::size_of,
    vesper_input_event::{
        record::{DecodeError, EVENT_SIZE, Event, KEY_DOWN, KEY_REPEAT, KEY_UP, Kind, offset},
        usage::{code, page},
    },
};

const ALL_KINDS: [(Kind, u8); 12] = [
    (Kind::Key, 1),
    (Kind::Rel, 2),
    (Kind::Abs, 3),
    (Kind::Sync, 4),
    (Kind::Dropped, 5),
    (Kind::DeviceAdded, 6),
    (Kind::DeviceRemoved, 7),
    (Kind::Focus, 8),
    (Kind::Enter, 9),
    (Kind::Leave, 10),
    (Kind::ResizeOffer, 11),
    (Kind::CloseRequest, 12),
];

#[test]
fn the_record_is_24_bytes_with_the_designed_offsets() {
    assert_eq!(EVENT_SIZE, 24);
    assert_eq!(offset::TIMESTAMP, 0);
    assert_eq!(offset::SUBJECT, 8);
    assert_eq!(offset::CODE, 12);
    assert_eq!(offset::VALUE, 16);
    assert_eq!(offset::KIND, 20);
    assert_eq!(offset::FLAGS, 21);
    assert_eq!(offset::PADDING, 22);
    assert_eq!(size_of::<[u8; EVENT_SIZE]>(), 24);
}

#[test]
fn kind_numbers_are_pinned() {
    for (kind, wire) in ALL_KINDS {
        assert_eq!(kind.to_wire(), wire, "{kind:?}");
        assert_eq!(Kind::from_wire(wire), Some(kind));
    }
    assert_eq!(Kind::from_wire(0), None, "zero is never a kind");
    for wire in 13..=255 {
        assert_eq!(Kind::from_wire(wire), None, "{wire}");
    }
}

#[test]
fn a_key_press_has_this_exact_wire_form() {
    // 'a' (usage 0x04 on the Keyboard page) pressed on device 7 at t = 0x0102030405060708.
    let event = Event::new(
        0x0102_0304_0506_0708,
        7,
        Kind::Key,
        code(page::KEYBOARD, 0x04),
        KEY_DOWN,
    );
    assert_eq!(
        event.to_bytes(),
        [
            0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, // timestamp
            0x07, 0x00, 0x00, 0x00, // subject
            0x04, 0x00, 0x07, 0x00, // code
            0x01, 0x00, 0x00, 0x00, // value
            0x01, // kind
            0x00, // flags
            0x00, 0x00, // padding
        ]
    );
}

#[test]
fn negative_values_are_twos_complement() {
    let event = Event::new(0, 1, Kind::Rel, 0x0001_0030, -2);
    assert_eq!(event.to_bytes()[16..20], [0xFE, 0xFF, 0xFF, 0xFF]);
    assert_eq!(Event::from_bytes(&event.to_bytes()), Ok(event));
}

#[test]
fn every_kind_round_trips_with_extreme_fields() {
    for (kind, _) in ALL_KINDS {
        let value = if kind == Kind::Key {
            KEY_REPEAT
        } else {
            i32::MIN
        };
        let event = Event::new(u64::MAX, u32::MAX, kind, u32::MAX, value);
        assert_eq!(Event::from_bytes(&event.to_bytes()), Ok(event), "{kind:?}");
    }
}

#[test]
fn sync_is_a_kind_four_record_without_code_or_value() {
    let sync = Event::sync(9, 3);
    assert_eq!(sync.kind, Kind::Sync);
    assert_eq!((sync.code, sync.value), (0, 0));
    assert_eq!(sync.to_bytes()[20], 4);
}

#[test]
fn a_buffer_of_the_wrong_length_is_refused() {
    let bytes = Event::sync(1, 1).to_bytes();
    assert_eq!(Event::from_bytes(&[]), Err(DecodeError::Length(0)));
    assert_eq!(
        Event::from_bytes(&bytes[..23]),
        Err(DecodeError::Length(23))
    );
    let mut longer = bytes.to_vec();
    longer.push(0);
    assert_eq!(Event::from_bytes(&longer), Err(DecodeError::Length(25)));
}

#[test]
fn an_unknown_kind_is_refused() {
    let mut bytes = Event::sync(1, 1).to_bytes();
    for kind in [0, 13, 0x80, 255] {
        bytes[offset::KIND] = kind;
        assert_eq!(
            Event::from_bytes(&bytes),
            Err(DecodeError::UnknownKind(kind))
        );
    }
}

#[test]
fn reserved_bytes_must_be_zero() {
    let good = Event::sync(1, 1).to_bytes();
    for at in [offset::FLAGS, offset::PADDING, offset::PADDING + 1] {
        let mut bytes = good;
        bytes[at] = 1;
        assert_eq!(
            Event::from_bytes(&bytes),
            Err(DecodeError::Reserved),
            "{at}"
        );
    }
}

#[test]
fn a_key_value_is_up_down_or_repeat() {
    for value in [KEY_UP, KEY_DOWN, KEY_REPEAT] {
        let event = Event::new(0, 1, Kind::Key, 0x0007_0004, value);
        assert_eq!(Event::from_bytes(&event.to_bytes()), Ok(event));
    }
    for value in [3, -1, i32::MAX, i32::MIN] {
        let event = Event::new(0, 1, Kind::Key, 0x0007_0004, value);
        assert_eq!(
            Event::from_bytes(&event.to_bytes()),
            Err(DecodeError::KeyValue(value))
        );
    }
    // Other kinds carry any value.
    let rel = Event::new(0, 1, Kind::Rel, 0x0001_0030, 3);
    assert_eq!(Event::from_bytes(&rel.to_bytes()), Ok(rel));
}
