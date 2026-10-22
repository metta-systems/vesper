//! Host tests of the HID component's report decoding and device bookkeeping.

#[cfg(test)]
mod tests {
    use {
        usb_hid::{
            decode::{
                Frame, KeyboardDecoder, MAX_FRAME, MAX_FRAME_BYTES, MouseDecoder, interval_ms,
            },
            devices::{DeviceKind, Devices, MAX_DEVICES, keyboard_descriptor, mouse_descriptor},
        },
        vesper_input_event::{
            descriptor::Descriptor,
            record::{EVENT_SIZE, Event, KEY_DOWN, KEY_UP, Kind},
            state::State,
            usage::{code, generic_desktop, keyboard, page},
        },
        vesper_usb_protocol::{enumeration::Speed, hid_boot::ReportError},
    };

    const TIME: u64 = 1_000;
    const A: u32 = 0x0007_0004;
    const B: u32 = 0x0007_0005;

    fn key(code: u32, value: i32) -> Event {
        Event::new(TIME, 1, Kind::Key, code, value)
    }

    fn keyboard_events(decoder: &mut KeyboardDecoder, report: [u8; 8]) -> Vec<Event> {
        let mut frame = Frame::new();
        decoder.decode(&report, TIME, 1, &mut frame).unwrap();
        frame.events().to_vec()
    }

    #[test]
    fn a_keyboard_report_becomes_the_changes_and_a_sync() {
        let mut decoder = KeyboardDecoder::new();
        assert_eq!(
            keyboard_events(&mut decoder, [0, 0, 4, 0, 0, 0, 0, 0]),
            [key(A, KEY_DOWN), Event::sync(TIME, 1)]
        );
        // Shift goes down and b with it; a stays down and says nothing.
        assert_eq!(
            keyboard_events(&mut decoder, [0x02, 0, 4, 5, 0, 0, 0, 0]),
            [
                key(code(page::KEYBOARD, keyboard::LEFT_SHIFT), KEY_DOWN),
                key(B, KEY_DOWN),
                Event::sync(TIME, 1)
            ]
        );
        assert_eq!(
            keyboard_events(&mut decoder, [0, 0, 0, 5, 0, 0, 0, 0]),
            [
                key(code(page::KEYBOARD, keyboard::LEFT_SHIFT), KEY_UP),
                key(A, KEY_UP),
                Event::sync(TIME, 1)
            ]
        );
    }

    #[test]
    fn a_report_that_changes_nothing_adds_no_frame() {
        let mut decoder = KeyboardDecoder::new();
        keyboard_events(&mut decoder, [0, 0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(keyboard_events(&mut decoder, [0, 0, 4, 0, 0, 0, 0, 0]), []);
        let mut mouse = MouseDecoder::new();
        let mut frame = Frame::new();
        mouse.decode(&[0, 0, 0], TIME, 1, &mut frame).unwrap();
        assert!(frame.is_empty());
    }

    #[test]
    fn every_modifier_has_its_usage() {
        let mut decoder = KeyboardDecoder::new();
        let events = keyboard_events(&mut decoder, [0xFF, 0, 0, 0, 0, 0, 0, 0]);
        let codes: Vec<u32> = events.iter().map(|event| event.code).collect();
        let expected: Vec<u32> = (0xE0..=0xE7)
            .map(|usage| code(page::KEYBOARD, usage))
            .chain([0])
            .collect();
        assert_eq!(codes, expected, "bit order, then the Sync");
    }

    #[test]
    fn roll_over_and_short_reports_are_refused_and_change_nothing() {
        let mut decoder = KeyboardDecoder::new();
        keyboard_events(&mut decoder, [0, 0, 4, 0, 0, 0, 0, 0]);
        let mut frame = Frame::new();
        assert_eq!(
            decoder.decode(&[0, 0, 1, 1, 1, 1, 1, 1], TIME, 1, &mut frame),
            Err(ReportError::RollOver)
        );
        assert_eq!(
            decoder.decode(&[0, 0, 4], TIME, 1, &mut frame),
            Err(ReportError::WrongLength)
        );
        assert!(frame.is_empty());
        // The held key is still known: releasing it is reported.
        assert_eq!(
            keyboard_events(&mut decoder, [0; 8]),
            [key(A, KEY_UP), Event::sync(TIME, 1)]
        );
    }

    #[test]
    fn a_mouse_report_becomes_buttons_movement_and_a_sync() {
        let mut decoder = MouseDecoder::new();
        let mut frame = Frame::new();
        decoder
            .decode(&[0b101, 5, 0xFE, 0xFF], TIME, 1, &mut frame)
            .unwrap();
        let left = code(page::BUTTON, 1);
        let middle = code(page::BUTTON, 3);
        let x = code(page::GENERIC_DESKTOP, generic_desktop::X);
        let y = code(page::GENERIC_DESKTOP, generic_desktop::Y);
        let wheel = code(page::GENERIC_DESKTOP, generic_desktop::WHEEL);
        assert_eq!(
            frame.events(),
            [
                key(left, KEY_DOWN),
                key(middle, KEY_DOWN),
                Event::new(TIME, 1, Kind::Rel, x, 5),
                Event::new(TIME, 1, Kind::Rel, y, -2),
                Event::new(TIME, 1, Kind::Rel, wheel, -1),
                Event::sync(TIME, 1),
            ]
        );
        frame.clear();
        // Only the left button is released; the mouse does not move.
        decoder.decode(&[0b100, 0, 0], TIME, 1, &mut frame).unwrap();
        assert_eq!(frame.events(), [key(left, KEY_UP), Event::sync(TIME, 1)]);
        assert_eq!(
            decoder.decode(&[0; 5], TIME, 1, &mut frame),
            Err(ReportError::WrongLength)
        );
    }

    #[test]
    fn a_frame_holds_the_largest_report_and_writes_whole_records() {
        let mut frame = Frame::new();
        let mut decoder = KeyboardDecoder::new();
        // Eight modifiers and six keys down: 14 changes and a Sync.
        decoder
            .decode(&[0xFF, 0, 4, 5, 6, 7, 8, 9], TIME, 1, &mut frame)
            .unwrap();
        assert_eq!(frame.events().len(), 8 + 6 + 1);
        decoder.decode(&[0; 8], TIME, 1, &mut frame).unwrap();
        assert!(frame.events().len() <= MAX_FRAME);
        let mut bytes = [0xAA; MAX_FRAME_BYTES + 8];
        let written = frame.write_to(&mut bytes).unwrap();
        assert_eq!(written, frame.events().len() * EVENT_SIZE);
        for (index, chunk) in bytes[..written].chunks(EVENT_SIZE).enumerate() {
            assert_eq!(Event::from_bytes(chunk), Ok(frame.events()[index]));
        }
        assert_eq!(bytes[written], 0xAA, "nothing past the records is touched");
        assert_eq!(frame.write_to(&mut bytes[..EVENT_SIZE]), None);
    }

    #[test]
    fn intervals_are_milliseconds_whatever_the_speed() {
        assert_eq!(interval_ms(Speed::Low, 10), 10);
        assert_eq!(interval_ms(Speed::Full, 8), 8);
        assert_eq!(interval_ms(Speed::Full, 0), 1, "at least a frame");
        assert_eq!(interval_ms(Speed::Full, 255), 255);
        // 2^(b-1) microframes of 125 µs.
        assert_eq!(interval_ms(Speed::High, 1), 1);
        assert_eq!(interval_ms(Speed::High, 4), 1);
        assert_eq!(interval_ms(Speed::High, 7), 8);
        assert_eq!(interval_ms(Speed::High, 16), 255, "capped");
        assert_eq!(interval_ms(Speed::High, 0), 1);
    }

    #[test]
    fn descriptors_round_trip_and_name_what_the_device_has() {
        let keyboard = keyboard_descriptor(0x046d, 0xc31c);
        let parsed = Descriptor::from_bytes(&keyboard.to_bytes()).unwrap();
        assert_eq!(parsed, keyboard);
        assert!(parsed.has_key(A));
        assert!(parsed.has_key(code(page::KEYBOARD, keyboard::RIGHT_GUI)));
        assert!(!parsed.has_key(code(page::BUTTON, 1)));
        assert_eq!((parsed.vendor(), parsed.product()), (0x046d, 0xc31c));

        let mouse = mouse_descriptor(1, 2);
        let parsed = Descriptor::from_bytes(&mouse.to_bytes()).unwrap();
        assert!(parsed.has_key(code(page::BUTTON, 3)));
        assert_eq!(parsed.axes().count(), 3);
    }

    #[test]
    fn devices_are_found_by_pipe_and_ids_are_never_reused() {
        let mut devices = Devices::new();
        let keyboard = devices
            .add(DeviceKind::Keyboard, 0x10001, keyboard_descriptor(1, 1))
            .unwrap();
        let mouse = devices
            .add(DeviceKind::Mouse, 0x10002, mouse_descriptor(1, 2))
            .unwrap();
        assert_eq!((keyboard, mouse), (1, 2));

        let mut frame = Frame::new();
        assert_eq!(
            devices.report(0x10001, &[0, 0, 4, 0, 0, 0, 0, 0], TIME, &mut frame),
            Ok(Some(1))
        );
        assert_eq!(frame.events()[0].subject, 1);
        frame.clear();
        assert_eq!(
            devices.report(0x10002, &[1, 3, 0], TIME, &mut frame),
            Ok(Some(2))
        );
        assert!(frame.events().iter().all(|event| event.subject == 2));
        frame.clear();
        assert_eq!(devices.report(0x99, &[0; 8], TIME, &mut frame), Ok(None));
        assert_eq!(
            devices.report(0x10001, &[0; 3], TIME, &mut frame),
            Err(ReportError::WrongLength)
        );

        devices.remove(keyboard);
        assert!(devices.get(keyboard).is_none());
        let again = devices
            .add(DeviceKind::Keyboard, 0x10003, keyboard_descriptor(1, 1))
            .unwrap();
        assert_eq!(again, 3, "an id is not reused");
    }

    #[test]
    fn the_table_has_a_limit() {
        let mut devices = Devices::new();
        for pipe in 0..u32::try_from(MAX_DEVICES).unwrap() {
            assert!(
                devices
                    .add(DeviceKind::Keyboard, pipe, keyboard_descriptor(1, 1))
                    .is_some()
            );
        }
        assert!(
            devices
                .add(DeviceKind::Keyboard, 99, keyboard_descriptor(1, 1))
                .is_none()
        );
    }

    #[test]
    fn a_snapshot_replays_into_the_state_the_reports_built() {
        let mut devices = Devices::new();
        let keyboard = devices
            .add(DeviceKind::Keyboard, 7, keyboard_descriptor(1, 1))
            .unwrap();
        let mut frame = Frame::new();
        for report in [
            [0x02, 0, 4, 0, 0, 0, 0, 0],
            [0x02, 0, 4, 5, 0, 0, 0, 0],
            [0x02, 0, 5, 0, 0, 0, 0, 0],
        ] {
            frame.clear();
            devices.report(7, &report, TIME, &mut frame).unwrap();
        }
        frame.clear();
        devices.snapshot(keyboard, TIME, &mut frame).unwrap();
        assert_eq!(frame.events().last(), Some(&Event::sync(TIME, 1)));
        let mut rebuilt = State::new();
        for event in frame.events() {
            rebuilt.apply(event).unwrap();
        }
        assert_eq!(&rebuilt, devices.get(keyboard).unwrap().state());
        assert!(rebuilt.is_down(B));
        assert!(!rebuilt.is_down(A));
        assert!(devices.snapshot(42, TIME, &mut Frame::new()).is_none());
    }
}
