use vesper_input_event::usage::*;

#[test]
fn codes_pack_page_above_usage() {
    assert_eq!(code(page::KEYBOARD, 0x04), 0x0007_0004);
    assert_eq!(page_of(0x0007_0004), 0x07);
    assert_eq!(usage_of(0x0007_0004), 0x04);
    assert_eq!(code(page::CONSUMER, consumer::AC_PAN), 0x000C_0238);
    assert_eq!(code(0xFFFF, 0xFFFF), u32::MAX);
    assert_eq!(page_of(u32::MAX), 0xFFFF);
    assert_eq!(usage_of(u32::MAX), 0xFFFF);
}

#[test]
fn page_numbers_are_the_hid_usage_table_numbers() {
    assert_eq!(page::GENERIC_DESKTOP, 0x01);
    assert_eq!(page::KEYBOARD, 0x07);
    assert_eq!(page::LED, 0x08);
    assert_eq!(page::BUTTON, 0x09);
    assert_eq!(page::CONSUMER, 0x0C);
    assert_eq!(page::DIGITIZER, 0x0D);
}

#[test]
fn axis_and_led_usages_are_the_hid_numbers() {
    assert_eq!(generic_desktop::X, 0x30);
    assert_eq!(generic_desktop::Y, 0x31);
    assert_eq!(generic_desktop::WHEEL, 0x38);
    assert_eq!(led::NUM_LOCK, 1);
    assert_eq!(led::CAPS_LOCK, 2);
    assert_eq!(led::SCROLL_LOCK, 3);
}

#[test]
fn buttons_count_from_one_on_the_button_page() {
    assert_eq!(button(1), 0x0009_0001);
    assert_eq!(button(3), 0x0009_0003);
}

#[test]
fn modifiers_are_usages_e0_to_e7_on_the_keyboard_page_only() {
    for usage in 0xE0..=0xE7 {
        assert!(is_modifier(code(page::KEYBOARD, usage)), "{usage:#x}");
    }
    assert!(!is_modifier(code(page::KEYBOARD, 0xDF)));
    assert!(!is_modifier(code(page::KEYBOARD, 0xE8)));
    assert!(!is_modifier(code(page::BUTTON, 0xE0)));
    assert!(!is_modifier(code(page::KEYBOARD, 0x04)));
    assert_eq!(keyboard::LEFT_CTRL, 0xE0);
    assert_eq!(keyboard::RIGHT_GUI, 0xE7);
}
