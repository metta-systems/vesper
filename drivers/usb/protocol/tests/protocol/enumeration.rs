use vesper_usb_protocol::{
    descriptor::ParseError,
    enumeration::*,
    setup::{DESCRIPTOR_CONFIGURATION, DESCRIPTOR_DEVICE, SetupPacket},
};

const DEVICE: [u8; 18] = [
    0x12, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x08, //
    0x6b, 0x1d, 0x02, 0x00, 0x00, 0x01, 0x01, 0x02, 0x03, 0x01,
];

const CONFIGURATION_HEADER: [u8; 9] = [0x09, 0x02, 0x22, 0x00, 0x01, 0x01, 0x00, 0xa0, 0x32];

const CONFIGURATION: [u8; 34] = [
    0x09, 0x02, 0x22, 0x00, 0x01, 0x01, 0x00, 0xa0, 0x32, 0x09, 0x04, 0x00, 0x00, 0x01, 0x03, 0x01,
    0x01, 0x00, 0x09, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, 0x3f, 0x00, 0x07, 0x05, 0x81, 0x03, 0x08,
    0x00, 0x0a,
];

/// Enumeration up to the header reply, with the device reporting
/// `total_length` in its configuration header. Returns the enumerator and
/// the step that reads the whole configuration.
fn enumerate_to_full_configuration(total_length: u8) -> (Enumerator, Step) {
    let mut header = CONFIGURATION_HEADER;
    header[2] = total_length;
    let mut enumerator = Enumerator::new(3).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    enumerator.next(Event::Delayed);
    enumerator.next(Event::Control(&DEVICE[..8]));
    enumerator.next(Event::Control(&[]));
    enumerator.next(Event::Delayed);
    enumerator.next(Event::Control(&DEVICE));
    let step = enumerator.next(Event::Control(&header));
    (enumerator, step)
}

#[test]
fn full_configuration_read_is_sized_by_total_length() {
    let (_, short) = enumerate_to_full_configuration(0x10);
    assert_eq!(
        short,
        Step::Control {
            setup: SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 0, 0x10)
        }
    );
    let (_, long) = enumerate_to_full_configuration(0x22);
    assert_eq!(
        long,
        Step::Control {
            setup: SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 0, 0x22)
        }
    );
}

#[test]
fn full_enumeration_configures_the_device_at_its_address() {
    let (mut enumerator, _) = enumerate_to_full_configuration(0x22);
    assert_eq!(
        enumerator.next(Event::Control(&CONFIGURATION)),
        Step::Control {
            setup: SetupPacket::set_configuration(1)
        }
    );
    assert_eq!(
        enumerator.next(Event::Control(&[])),
        Step::Configured {
            address: 3,
            configuration_value: 1
        }
    );
}

#[test]
fn first_device_read_asks_for_eight_bytes_and_the_full_read_for_eighteen() {
    let mut enumerator = Enumerator::new(4).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    assert_eq!(
        enumerator.next(Event::Delayed),
        Step::Control {
            setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 8)
        }
    );
    enumerator.next(Event::Control(&DEVICE[..8]));
    enumerator.next(Event::Control(&[]));
    assert_eq!(
        enumerator.next(Event::Delayed),
        Step::Control {
            setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18)
        }
    );
}

#[test]
fn address_outside_range_is_refused() {
    assert!(Enumerator::new(0).is_none());
    assert!(Enumerator::new(128).is_none());
    assert!(Enumerator::new(FIRST_ADDRESS).is_some());
    assert!(Enumerator::new(LAST_ADDRESS).is_some());
}

#[test]
fn the_port_speed_is_kept_from_the_reset() {
    let mut enumerator = Enumerator::new(2).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Low));
    assert_eq!(enumerator.speed(), Speed::Low);
}

#[test]
fn stall_at_any_step_fails_enumeration() {
    let mut enumerator = Enumerator::new(2).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    enumerator.next(Event::Delayed);
    assert_eq!(
        enumerator.next(Event::ControlFailed(Failure::Stalled)),
        Step::Failed(Failure::Stalled)
    );
}

#[test]
fn a_failed_state_stays_failed() {
    let mut enumerator = Enumerator::new(2).expect("valid address");
    enumerator.next(Event::ControlFailed(Failure::Transfer));
    assert_eq!(
        enumerator.next(Event::Start),
        Step::Failed(Failure::Protocol)
    );
}

#[test]
fn event_out_of_order_is_a_protocol_failure() {
    let mut enumerator = Enumerator::new(2).expect("valid address");
    assert_eq!(
        enumerator.next(Event::Delayed),
        Step::Failed(Failure::Protocol)
    );
}

#[test]
fn device_with_no_configuration_is_rejected() {
    let mut no_configuration = DEVICE;
    no_configuration[17] = 0;
    let mut enumerator = Enumerator::new(2).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    enumerator.next(Event::Delayed);
    enumerator.next(Event::Control(&DEVICE[..8]));
    enumerator.next(Event::Control(&[]));
    enumerator.next(Event::Delayed);
    assert_eq!(
        enumerator.next(Event::Control(&no_configuration)),
        Step::Failed(Failure::Protocol)
    );
}

#[test]
fn short_first_device_reply_is_truncated() {
    let mut enumerator = Enumerator::new(2).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    enumerator.next(Event::Delayed);
    // Only 3 of the 8 bytes asked for arrived.
    assert_eq!(
        enumerator.next(Event::Control(&[0x12, 0x01, 0x00])),
        Step::Failed(Failure::Descriptor(ParseError::Truncated))
    );
}

#[test]
fn reply_that_is_not_a_device_descriptor_is_bad_length() {
    let mut enumerator = Enumerator::new(2).expect("valid address");
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    enumerator.next(Event::Delayed);
    // Descriptor type 5 (endpoint) where the device descriptor was asked for.
    assert_eq!(
        enumerator.next(Event::Control(&[
            0x07, 0x05, 0x81, 0x03, 0x08, 0x00, 0x0a, 0x00
        ])),
        Step::Failed(Failure::Descriptor(ParseError::BadLength))
    );
}

#[test]
fn endpoint_zero_starts_at_the_packet_size_its_speed_allows() {
    // USB 2.0 §5.5.3: high speed 64 only, low speed 8 only, full speed 8 to
    // 64 (8 until bMaxPacketSize0 is read).
    assert_eq!(default_max_packet0(Speed::High), 64);
    assert_eq!(default_max_packet0(Speed::Low), 8);
    assert_eq!(default_max_packet0(Speed::Full), 8);
}
