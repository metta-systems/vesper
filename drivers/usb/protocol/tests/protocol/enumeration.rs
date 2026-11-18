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

/// An enumerator past the reset and the controller's addressing to
/// `address`, and the step it asks for next.
fn addressed(address: u8) -> (Enumerator, Step) {
    let mut enumerator = Enumerator::new();
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Full));
    enumerator.next(Event::Delayed);
    enumerator.next(Event::Addressed(address));
    let step = enumerator.next(Event::Delayed);
    (enumerator, step)
}

/// Enumeration up to the header reply, with the device reporting
/// `total_length` in its configuration header. Returns the enumerator and
/// the step that reads the whole configuration.
fn enumerate_to_full_configuration(total_length: u8) -> (Enumerator, Step) {
    let mut header = CONFIGURATION_HEADER;
    header[2] = total_length;
    let (mut enumerator, _) = addressed(3);
    enumerator.next(Event::Control(&DEVICE[..8]));
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
fn full_enumeration_configures_the_device_at_the_address_the_controller_gave() {
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
fn the_controller_addresses_the_device_after_the_reset_recovery() {
    let mut enumerator = Enumerator::new();
    assert_eq!(enumerator.next(Event::Start), Step::ResetPort);
    assert_eq!(
        enumerator.next(Event::PortReset(Speed::High)),
        Step::Delay {
            milliseconds: RECOVERY_MS
        }
    );
    assert_eq!(enumerator.next(Event::Delayed), Step::AddressDevice);
    // The device gets its settle time before the first request at the new
    // address (USB 2.0 §9.2.6.3).
    assert_eq!(
        enumerator.next(Event::Addressed(5)),
        Step::Delay {
            milliseconds: ADDRESS_SETTLE_MS
        }
    );
}

#[test]
fn no_set_address_is_sent_by_the_enumeration() {
    let (mut enumerator, mut step) = addressed(7);
    let replies: [&[u8]; 5] = [
        &DEVICE[..8],
        &DEVICE,
        &CONFIGURATION_HEADER,
        &CONFIGURATION,
        &[],
    ];
    for reply in replies {
        let Step::Control { setup } = step else {
            panic!("a control transfer, got {step:?}");
        };
        assert_ne!(setup, SetupPacket::set_address(7));
        assert_ne!(setup.request, SetupPacket::set_address(7).request);
        step = enumerator.next(Event::Control(reply));
    }
    assert!(matches!(step, Step::Configured { address: 7, .. }));
}

#[test]
fn first_device_read_asks_for_eight_bytes_and_the_full_read_for_eighteen() {
    let (mut enumerator, first) = addressed(4);
    assert_eq!(
        first,
        Step::Control {
            setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 8)
        }
    );
    assert_eq!(
        enumerator.next(Event::Control(&DEVICE[..8])),
        Step::Control {
            setup: SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18)
        }
    );
}

#[test]
fn an_address_outside_the_range_is_a_protocol_failure() {
    for address in [0, 128, 255] {
        let mut enumerator = Enumerator::new();
        enumerator.next(Event::Start);
        enumerator.next(Event::PortReset(Speed::Full));
        enumerator.next(Event::Delayed);
        assert_eq!(
            enumerator.next(Event::Addressed(address)),
            Step::Failed(Failure::Protocol)
        );
    }
    let (_, step) = addressed(FIRST_ADDRESS);
    assert!(matches!(step, Step::Control { .. }));
    let (_, step) = addressed(LAST_ADDRESS);
    assert!(matches!(step, Step::Control { .. }));
}

#[test]
fn the_port_speed_is_kept_from_the_reset() {
    let mut enumerator = Enumerator::new();
    enumerator.next(Event::Start);
    enumerator.next(Event::PortReset(Speed::Low));
    assert_eq!(enumerator.speed(), Speed::Low);
}

#[test]
fn stall_at_any_step_fails_enumeration() {
    let (mut enumerator, _) = addressed(2);
    assert_eq!(
        enumerator.next(Event::ControlFailed(Failure::Stalled)),
        Step::Failed(Failure::Stalled)
    );
}

#[test]
fn a_failed_state_stays_failed() {
    let mut enumerator = Enumerator::new();
    enumerator.next(Event::ControlFailed(Failure::Transfer));
    assert_eq!(
        enumerator.next(Event::Start),
        Step::Failed(Failure::Protocol)
    );
}

#[test]
fn event_out_of_order_is_a_protocol_failure() {
    let mut enumerator = Enumerator::new();
    assert_eq!(
        enumerator.next(Event::Delayed),
        Step::Failed(Failure::Protocol)
    );
    // An address before the controller was asked for one.
    let mut enumerator = Enumerator::new();
    enumerator.next(Event::Start);
    assert_eq!(
        enumerator.next(Event::Addressed(3)),
        Step::Failed(Failure::Protocol)
    );
}

#[test]
fn device_with_no_configuration_is_rejected() {
    let mut no_configuration = DEVICE;
    no_configuration[17] = 0;
    let (mut enumerator, _) = addressed(2);
    enumerator.next(Event::Control(&DEVICE[..8]));
    assert_eq!(
        enumerator.next(Event::Control(&no_configuration)),
        Step::Failed(Failure::Protocol)
    );
}

#[test]
fn short_first_device_reply_is_truncated() {
    let (mut enumerator, _) = addressed(2);
    // Only 3 of the 8 bytes asked for arrived.
    assert_eq!(
        enumerator.next(Event::Control(&[0x12, 0x01, 0x00])),
        Step::Failed(Failure::Descriptor(ParseError::Truncated))
    );
}

#[test]
fn reply_that_is_not_a_device_descriptor_is_bad_length() {
    let (mut enumerator, _) = addressed(2);
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

#[test]
fn addresses_are_handed_out_lowest_first_and_come_back() {
    let mut pool = AddressPool::new();
    assert_eq!(pool.take(), Some(FIRST_ADDRESS));
    assert_eq!(pool.take(), Some(2));
    assert_eq!(pool.take(), Some(3));
    assert!(pool.is_taken(2));
    pool.release(2);
    assert!(!pool.is_taken(2));
    assert_eq!(pool.take(), Some(2), "the lowest free address again");
    assert_eq!(pool.take(), Some(4));
}

#[test]
fn the_pool_runs_out_after_the_last_address_and_ignores_strays() {
    let mut pool = AddressPool::new();
    for expected in FIRST_ADDRESS..=LAST_ADDRESS {
        assert_eq!(pool.take(), Some(expected));
    }
    assert_eq!(pool.take(), None);
    assert!(!pool.is_taken(0) && !pool.is_taken(128));
    pool.release(0);
    pool.release(128);
    pool.release(255);
    assert_eq!(pool.take(), None, "addresses outside the range are ignored");
    pool.release(100);
    assert_eq!(pool.take(), Some(100));
    // Releasing an address twice frees it once.
    pool.release(7);
    pool.release(7);
    assert_eq!(pool.take(), Some(7));
    assert_eq!(pool.take(), None);
}
