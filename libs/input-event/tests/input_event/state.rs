use vesper_input_event::{
    record::{Event, KEY_DOWN, KEY_REPEAT, KEY_UP, Kind},
    state::{MAX_AXES, MAX_OTHER_KEYS, State, StateError},
    usage::{button, code, consumer, generic_desktop, keyboard, page},
};

fn key(code: u32, value: i32) -> Event {
    Event::new(0, 1, Kind::Key, code, value)
}

fn abs(code: u32, value: i32) -> Event {
    Event::new(0, 1, Kind::Abs, code, value)
}

fn of(kind: Kind) -> Event {
    Event::new(0, 1, kind, 0, 0)
}

const A: u32 = 0x0007_0004;

#[test]
fn a_fresh_state_has_nothing_down() {
    let state = State::new();
    assert_eq!(state.down_count(), 0);
    assert!(!state.is_down(A));
    assert_eq!(state.modifiers(), 0);
    assert_eq!(
        state.axis(code(page::GENERIC_DESKTOP, generic_desktop::X)),
        None
    );
    assert!(!state.is_stale());
    assert_eq!(State::default(), state);
}

#[test]
fn press_hold_and_release_a_key() {
    let mut state = State::new();
    state.apply(&key(A, KEY_DOWN)).unwrap();
    assert!(state.is_down(A));
    state.apply(&key(A, KEY_REPEAT)).unwrap();
    assert!(state.is_down(A), "a repeat keeps it down");
    assert_eq!(state.down_count(), 1);
    state.apply(&key(A, KEY_UP)).unwrap();
    assert!(!state.is_down(A));
    state.apply(&key(A, KEY_UP)).unwrap();
    assert_eq!(state.down_count(), 0, "a second release changes nothing");
}

#[test]
fn keyboard_usages_to_255_use_the_bitmap_without_clashing() {
    let mut state = State::new();
    for usage in [0_u16, 4, 63, 64, 127, 128, 200, 255] {
        state
            .apply(&key(code(page::KEYBOARD, usage), KEY_DOWN))
            .unwrap();
    }
    assert_eq!(state.down_count(), 8);
    for usage in [0_u16, 4, 63, 64, 127, 128, 200, 255] {
        assert!(state.is_down(code(page::KEYBOARD, usage)), "{usage}");
    }
    assert!(!state.is_down(code(page::KEYBOARD, 5)));
    assert!(!state.is_down(code(page::KEYBOARD, 65)));
    state.apply(&key(code(page::KEYBOARD, 64), KEY_UP)).unwrap();
    assert!(!state.is_down(code(page::KEYBOARD, 64)));
    assert!(state.is_down(code(page::KEYBOARD, 63)));
    assert!(state.is_down(code(page::KEYBOARD, 127)));
}

#[test]
fn modifiers_read_back_as_the_boot_report_byte() {
    let mut state = State::new();
    state
        .apply(&key(code(page::KEYBOARD, keyboard::LEFT_SHIFT), KEY_DOWN))
        .unwrap();
    assert_eq!(state.modifiers(), 0b0000_0010);
    state
        .apply(&key(code(page::KEYBOARD, keyboard::RIGHT_GUI), KEY_DOWN))
        .unwrap();
    state
        .apply(&key(code(page::KEYBOARD, keyboard::LEFT_CTRL), KEY_DOWN))
        .unwrap();
    assert_eq!(state.modifiers(), 0b1000_0011);
    state
        .apply(&key(code(page::KEYBOARD, keyboard::LEFT_SHIFT), KEY_UP))
        .unwrap();
    assert_eq!(state.modifiers(), 0b1000_0001);
    // An ordinary key does not show up as a modifier.
    state.apply(&key(A, KEY_DOWN)).unwrap();
    assert_eq!(state.modifiers(), 0b1000_0001);
}

#[test]
fn buttons_one_to_sixty_four_use_the_button_bitmap() {
    let mut state = State::new();
    for number in [1_u16, 2, 3, 64] {
        state.apply(&key(button(number), KEY_DOWN)).unwrap();
    }
    assert!(state.is_down(button(1)) && state.is_down(button(64)));
    assert!(!state.is_down(button(4)));
    state.apply(&key(button(2), KEY_UP)).unwrap();
    assert!(!state.is_down(button(2)));
    assert_eq!(state.down_count(), 3);
    // Button 0 does not exist; it falls to the small set rather than a bit.
    state.apply(&key(button(0), KEY_DOWN)).unwrap();
    assert!(state.is_down(button(0)));
    assert_eq!(state.down_count(), 4);
}

#[test]
fn other_pages_use_a_small_set_that_can_fill_up() {
    let mut state = State::new();
    let pan = code(page::CONSUMER, consumer::AC_PAN);
    state.apply(&key(pan, KEY_DOWN)).unwrap();
    assert!(state.is_down(pan));
    state.apply(&key(pan, KEY_DOWN)).unwrap();
    assert_eq!(state.down_count(), 1, "pressing twice holds it once");
    state.apply(&key(pan, KEY_UP)).unwrap();
    assert_eq!(state.down_count(), 0);

    for usage in 0..u16::try_from(MAX_OTHER_KEYS).unwrap() {
        state
            .apply(&key(code(page::CONSUMER, usage), KEY_DOWN))
            .unwrap();
    }
    let before = state;
    assert_eq!(
        state.apply(&key(code(page::CONSUMER, 0x100), KEY_DOWN)),
        Err(StateError::Full)
    );
    assert_eq!(state, before, "a refused event changes nothing");
    state.apply(&key(code(page::CONSUMER, 3), KEY_UP)).unwrap();
    state
        .apply(&key(code(page::CONSUMER, 0x100), KEY_DOWN))
        .unwrap();
    assert!(state.is_down(code(page::CONSUMER, 0x100)));
    assert!(!state.is_down(code(page::CONSUMER, 3)));
    assert!(
        state.is_down(code(page::CONSUMER, 4)),
        "the others stay down"
    );
}

#[test]
fn absolute_axes_keep_their_last_value() {
    let mut state = State::new();
    let x = code(page::GENERIC_DESKTOP, generic_desktop::X);
    let y = code(page::GENERIC_DESKTOP, generic_desktop::Y);
    state.apply(&abs(x, 100)).unwrap();
    state.apply(&abs(y, -5)).unwrap();
    state.apply(&abs(x, 250)).unwrap();
    assert_eq!(state.axis(x), Some(250));
    assert_eq!(state.axis(y), Some(-5));
    assert_eq!(
        state.axis(code(page::GENERIC_DESKTOP, generic_desktop::Z)),
        None
    );

    for usage in 0..u16::try_from(MAX_AXES - 2).unwrap() {
        state.apply(&abs(code(page::DIGITIZER, usage), 1)).unwrap();
    }
    assert_eq!(
        state.apply(&abs(code(page::DIGITIZER, 0x99), 1)),
        Err(StateError::Full)
    );
    state.apply(&abs(x, 7)).unwrap();
    assert_eq!(
        state.axis(x),
        Some(7),
        "a known axis still updates when full"
    );
}

#[test]
fn relative_motion_frames_and_hotplug_announcements_are_not_state() {
    let mut state = State::new();
    state.apply(&key(A, KEY_DOWN)).unwrap();
    let before = state;
    for kind in [
        Kind::Rel,
        Kind::Sync,
        Kind::DeviceAdded,
        Kind::Focus,
        Kind::Enter,
        Kind::Leave,
        Kind::ResizeOffer,
        Kind::CloseRequest,
    ] {
        state.apply(&of(kind)).unwrap();
        assert_eq!(state, before, "{kind:?}");
    }
}

#[test]
fn removing_a_device_releases_everything() {
    let mut state = State::new();
    state.apply(&key(A, KEY_DOWN)).unwrap();
    state.apply(&key(button(1), KEY_DOWN)).unwrap();
    state
        .apply(&key(code(page::CONSUMER, 0xE9), KEY_DOWN))
        .unwrap();
    state
        .apply(&abs(code(page::GENERIC_DESKTOP, generic_desktop::X), 9))
        .unwrap();
    state.apply(&of(Kind::DeviceRemoved)).unwrap();
    assert_eq!(state, State::new());
}

#[test]
fn a_drop_marks_the_state_stale_until_a_snapshot_replaces_it() {
    let mut state = State::new();
    state.apply(&key(A, KEY_DOWN)).unwrap();
    state.apply(&of(Kind::Dropped)).unwrap();
    assert!(state.is_stale());
    assert!(
        state.is_down(A),
        "the stale state may be wrong, but it is kept"
    );

    // The producer's real state: the key was released while records were lost.
    let mut snapshot = State::new();
    snapshot.apply(&key(button(1), KEY_DOWN)).unwrap();
    state.resync(&snapshot);
    assert!(!state.is_stale());
    assert!(!state.is_down(A), "the stuck key is gone");
    assert!(state.is_down(button(1)));
}
