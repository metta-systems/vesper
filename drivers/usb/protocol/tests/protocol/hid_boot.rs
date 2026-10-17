use vesper_usb_protocol::hid_boot::*;

fn keyboard(modifiers: u8, keys: [u8; 6]) -> KeyboardReport {
    KeyboardReport { modifiers, keys }
}

fn changes(previous: KeyboardReport, current: KeyboardReport) -> std::vec::Vec<KeyChange> {
    let mut collected = std::vec::Vec::new();
    for_each_change(&previous, &current, |change| collected.push(change));
    collected
}

#[test]
fn keyboard_report_fields() {
    let report =
        KeyboardReport::parse(&[0x02, 0x00, 0x04, 0x05, 0x00, 0x00, 0x00, 0x00]).expect("8 bytes");
    assert_eq!(report.modifiers, 0x02);
    assert_eq!(report.keys, [0x04, 0x05, 0, 0, 0, 0]);
}

#[test]
fn keyboard_report_of_wrong_length_is_refused() {
    assert_eq!(
        KeyboardReport::parse(&[0; 7]),
        Err(ReportError::WrongLength)
    );
    assert_eq!(
        KeyboardReport::parse(&[0; 9]),
        Err(ReportError::WrongLength)
    );
}

#[test]
fn roll_over_report_is_refused_so_state_is_kept() {
    let rolled = [
        0x00, 0x00, ROLL_OVER, ROLL_OVER, ROLL_OVER, ROLL_OVER, ROLL_OVER, ROLL_OVER,
    ];
    assert_eq!(KeyboardReport::parse(&rolled), Err(ReportError::RollOver));
}

#[test]
fn pressing_one_key_reports_one_press() {
    let before = keyboard(0, [0; 6]);
    let after = keyboard(0, [0x04, 0, 0, 0, 0, 0]);
    assert_eq!(
        changes(before, after),
        [KeyChange::Key {
            usage: 0x04,
            pressed: true
        }]
    );
}

#[test]
fn releasing_one_key_reports_one_release() {
    let before = keyboard(0, [0x04, 0x05, 0, 0, 0, 0]);
    let after = keyboard(0, [0x05, 0, 0, 0, 0, 0]);
    assert_eq!(
        changes(before, after),
        [KeyChange::Key {
            usage: 0x04,
            pressed: false
        }]
    );
}

#[test]
fn a_held_key_produces_no_change() {
    let report = keyboard(0, [0x04, 0, 0, 0, 0, 0]);
    assert!(changes(report, report).is_empty());
}

#[test]
fn key_position_in_the_slot_array_does_not_matter() {
    let before = keyboard(0, [0x04, 0x05, 0, 0, 0, 0]);
    let after = keyboard(0, [0x05, 0x04, 0, 0, 0, 0]);
    assert!(changes(before, after).is_empty());
}

#[test]
fn modifier_bits_report_each_changed_bit_in_order() {
    // Left Shift (bit 1) and right Ctrl (bit 4) both go down; left Ctrl (bit 0) comes up.
    let before = keyboard(0x01, [0; 6]);
    let after = keyboard(0x12, [0; 6]);
    assert_eq!(
        changes(before, after),
        [
            KeyChange::Modifier {
                mask: 0x01,
                pressed: false
            },
            KeyChange::Modifier {
                mask: 0x02,
                pressed: true
            },
            KeyChange::Modifier {
                mask: 0x10,
                pressed: true
            },
        ]
    );
}

#[test]
fn releases_come_before_presses_in_one_report() {
    let before = keyboard(0, [0x04, 0, 0, 0, 0, 0]);
    let after = keyboard(0, [0x05, 0, 0, 0, 0, 0]);
    assert_eq!(
        changes(before, after),
        [
            KeyChange::Key {
                usage: 0x04,
                pressed: false
            },
            KeyChange::Key {
                usage: 0x05,
                pressed: true
            }
        ]
    );
}

#[test]
fn mouse_three_byte_report_has_no_wheel() {
    let report = MouseReport::parse(&[BUTTON_LEFT | BUTTON_MIDDLE, 0x05, 0xfe]).expect("3 bytes");
    assert_eq!(report.buttons, 0x05);
    assert_eq!((report.dx, report.dy, report.wheel), (5, -2, 0));
}

#[test]
fn mouse_four_byte_report_reads_signed_wheel() {
    let report = MouseReport::parse(&[BUTTON_RIGHT, 0x00, 0x00, 0xff]).expect("4 bytes");
    assert_eq!(report.buttons, BUTTON_RIGHT);
    assert_eq!(report.wheel, -1);
}

#[test]
fn mouse_report_of_other_lengths_is_refused() {
    assert_eq!(MouseReport::parse(&[0, 0]), Err(ReportError::WrongLength));
    assert_eq!(MouseReport::parse(&[0; 5]), Err(ReportError::WrongLength));
}
