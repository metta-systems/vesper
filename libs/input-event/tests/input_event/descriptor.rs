use vesper_input_event::{
    descriptor::{
        AxisInfo, DESCRIPTOR_SIZE, Descriptor, DescriptorError, KeyRange, LAYOUT_VERSION,
        MAX_DESCRIPTOR_AXES, MAX_KEY_RANGES, MAX_NAME, Unit, bus,
    },
    usage::{button, code, generic_desktop, keyboard, page},
};

fn mouse() -> Descriptor {
    let mut descriptor = Descriptor::new(bus::USB, 0x046D, 0xC077, 0x0102);
    descriptor.set_name("USB Optical Mouse");
    for usage in [generic_desktop::X, generic_desktop::Y] {
        descriptor
            .push_axis(AxisInfo {
                code: code(page::GENERIC_DESKTOP, usage),
                min: -127,
                max: 127,
                resolution: 800,
                unit: Unit::None,
            })
            .unwrap();
    }
    descriptor
        .push_axis(AxisInfo {
            code: code(page::GENERIC_DESKTOP, generic_desktop::WHEEL),
            min: -127,
            max: 127,
            resolution: 1,
            unit: Unit::None,
        })
        .unwrap();
    descriptor
        .push_key_range(KeyRange {
            page: page::BUTTON,
            first: 1,
            last: 3,
        })
        .unwrap();
    descriptor
}

#[test]
fn the_descriptor_is_272_bytes() {
    assert_eq!(DESCRIPTOR_SIZE, 272);
    assert_eq!(
        16 + MAX_NAME + MAX_DESCRIPTOR_AXES * 20 + MAX_KEY_RANGES * 8,
        272
    );
    assert_eq!(
        Descriptor::new(0, 0, 0, 0).to_bytes().len(),
        DESCRIPTOR_SIZE
    );
}

#[test]
fn a_mouse_descriptor_has_this_exact_wire_form() {
    let bytes = mouse().to_bytes();
    assert_eq!(bytes[0..4], LAYOUT_VERSION.to_le_bytes());
    assert_eq!(bytes[0..4], [1, 0, 0, 0]);
    assert_eq!(bytes[4..6], [0x03, 0x00], "bus USB");
    assert_eq!(bytes[6..8], [0x6D, 0x04], "vendor");
    assert_eq!(bytes[8..10], [0x77, 0xC0], "product");
    assert_eq!(bytes[10..12], [0x02, 0x01], "version");
    assert_eq!(
        bytes[12..16],
        [17, 3, 1, 0],
        "name length, axes, key ranges, reserved"
    );
    assert_eq!(&bytes[16..33], b"USB Optical Mouse");
    assert!(
        bytes[33..48].iter().all(|&b| b == 0),
        "the name is zero padded"
    );
    // The first axis: X, -127..127, 800 counts, no unit.
    assert_eq!(
        bytes[48..68],
        [
            0x30, 0x00, 0x01, 0x00, // code 0x00010030
            0x81, 0xFF, 0xFF, 0xFF, // min -127
            0x7F, 0x00, 0x00, 0x00, // max 127
            0x20, 0x03, 0x00, 0x00, // resolution 800
            0x00, 0x00, 0x00, 0x00, // unit, reserved
        ]
    );
    // The third axis starts 40 bytes later; the fourth slot is empty.
    assert_eq!(bytes[88..92], [0x38, 0x00, 0x01, 0x00]);
    assert!(bytes[108..208].iter().all(|&b| b == 0));
    // The key range: Button page, 1 to 3.
    assert_eq!(
        bytes[208..216],
        [0x09, 0x00, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00]
    );
    assert!(bytes[216..].iter().all(|&b| b == 0));
}

#[test]
fn a_descriptor_round_trips() {
    let descriptor = mouse();
    assert_eq!(
        Descriptor::from_bytes(&descriptor.to_bytes()),
        Ok(descriptor)
    );
    assert_eq!(descriptor.name(), b"USB Optical Mouse");
    assert_eq!(descriptor.axes().count(), 3);
    assert_eq!(descriptor.key_ranges().count(), 1);
}

#[test]
fn a_full_descriptor_round_trips() {
    let mut descriptor = Descriptor::new(bus::VIRTUAL, 0xFFFF, 0xFFFF, 0xFFFF);
    descriptor.set_name("0123456789abcdef0123456789abcdef");
    assert_eq!(descriptor.name().len(), MAX_NAME);
    for index in 0..MAX_DESCRIPTOR_AXES {
        descriptor
            .push_axis(AxisInfo {
                code: u32::MAX - u32::try_from(index).unwrap(),
                min: i32::MIN,
                max: i32::MAX,
                resolution: u32::MAX,
                unit: Unit::Degree,
            })
            .unwrap();
    }
    for index in 0..MAX_KEY_RANGES {
        descriptor
            .push_key_range(KeyRange {
                page: 0xFFFF,
                first: u16::try_from(index).unwrap(),
                last: u16::MAX,
            })
            .unwrap();
    }
    assert_eq!(
        Descriptor::from_bytes(&descriptor.to_bytes()),
        Ok(descriptor)
    );
}

#[test]
fn names_are_cut_at_a_character_boundary() {
    let mut descriptor = Descriptor::new(0, 0, 0, 0);
    // 31 ASCII bytes then a two-byte character: it does not fit and is dropped whole.
    descriptor.set_name("0123456789012345678901234567890é");
    assert_eq!(descriptor.name().len(), 31);
    core::str::from_utf8(descriptor.name()).unwrap();
    descriptor.set_name("short");
    assert_eq!(
        descriptor.name(),
        b"short",
        "a shorter name replaces the old one"
    );
    assert!(descriptor.to_bytes()[21..48].iter().all(|&b| b == 0));
}

#[test]
fn entries_past_the_capacity_or_out_of_order_are_refused() {
    let mut descriptor = Descriptor::new(0, 0, 0, 0);
    let axis = AxisInfo {
        code: 1,
        min: 0,
        max: 1,
        resolution: 1,
        unit: Unit::None,
    };
    for _ in 0..MAX_DESCRIPTOR_AXES {
        descriptor.push_axis(axis).unwrap();
    }
    assert_eq!(descriptor.push_axis(axis), Err(DescriptorError::Full));
    let backwards = AxisInfo {
        min: 2,
        max: 1,
        ..axis
    };
    assert_eq!(
        Descriptor::new(0, 0, 0, 0).push_axis(backwards),
        Err(DescriptorError::Order)
    );
    let range = KeyRange {
        page: 7,
        first: 4,
        last: 4,
    };
    for _ in 0..MAX_KEY_RANGES {
        descriptor.push_key_range(range).unwrap();
    }
    assert_eq!(descriptor.push_key_range(range), Err(DescriptorError::Full));
    assert_eq!(
        Descriptor::new(0, 0, 0, 0).push_key_range(KeyRange {
            page: 7,
            first: 5,
            last: 4
        }),
        Err(DescriptorError::Order)
    );
}

#[test]
fn has_key_looks_through_the_ranges() {
    let mut keyboard_descriptor = Descriptor::new(bus::USB, 1, 2, 3);
    keyboard_descriptor
        .push_key_range(KeyRange {
            page: page::KEYBOARD,
            first: 0x04,
            last: 0xE7,
        })
        .unwrap();
    keyboard_descriptor
        .push_key_range(KeyRange {
            page: page::LED,
            first: 1,
            last: 3,
        })
        .unwrap();
    assert!(keyboard_descriptor.has_key(code(page::KEYBOARD, 0x04)));
    assert!(keyboard_descriptor.has_key(code(page::KEYBOARD, keyboard::RIGHT_GUI)));
    assert!(!keyboard_descriptor.has_key(code(page::KEYBOARD, 0x03)));
    assert!(!keyboard_descriptor.has_key(code(page::KEYBOARD, 0xE8)));
    assert!(!keyboard_descriptor.has_key(button(1)));
    assert!(keyboard_descriptor.has_key(code(page::LED, 2)));
}

#[test]
fn a_malformed_buffer_is_refused() {
    let good = mouse().to_bytes();
    assert_eq!(
        Descriptor::from_bytes(&good[..271]),
        Err(DescriptorError::Length(271))
    );
    assert_eq!(Descriptor::from_bytes(&[]), Err(DescriptorError::Length(0)));

    let mut bytes = good;
    bytes[0] = 2;
    assert_eq!(
        Descriptor::from_bytes(&bytes),
        Err(DescriptorError::Version(2))
    );

    for (at, value) in [(12, 33), (13, 9), (14, 9)] {
        let mut bytes = good;
        bytes[at] = value;
        assert_eq!(
            Descriptor::from_bytes(&bytes),
            Err(DescriptorError::Count),
            "{at}"
        );
    }

    // Reserved byte, name tail, an axis slot past the count, its padding, a range slot
    // past the count and its padding.
    for at in [15, 40, 108, 48 + 17, 216, 214] {
        let mut bytes = good;
        bytes[at] = 1;
        assert_eq!(
            Descriptor::from_bytes(&bytes),
            Err(DescriptorError::Reserved),
            "{at}"
        );
    }

    let mut bytes = good;
    bytes[48 + 16] = 3;
    assert_eq!(
        Descriptor::from_bytes(&bytes),
        Err(DescriptorError::Unit(3))
    );

    // min above max.
    let mut bytes = good;
    bytes[48 + 4..48 + 8].copy_from_slice(&200_i32.to_le_bytes());
    assert_eq!(Descriptor::from_bytes(&bytes), Err(DescriptorError::Order));
    // first above last.
    let mut bytes = good;
    bytes[208 + 2..208 + 4].copy_from_slice(&9_u16.to_le_bytes());
    assert_eq!(Descriptor::from_bytes(&bytes), Err(DescriptorError::Order));
}
