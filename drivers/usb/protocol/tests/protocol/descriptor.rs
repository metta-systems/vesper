use vesper_usb_protocol::descriptor::*;

/// A keyboard's device descriptor, 18 bytes, as its wire form.
const DEVICE: [u8; 18] = [
    0x12, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x08, //
    0x6b, 0x1d, 0x02, 0x00, 0x00, 0x01, 0x01, 0x02, 0x03, 0x01,
];

/// Configuration 1 with one HID boot keyboard interface and an interrupt-IN
/// endpoint 1 (interval 10), 9 + 9 + 7 + 9 (HID class) bytes.
const CONFIGURATION: [u8; 34] = [
    0x09, 0x02, 0x22, 0x00, 0x01, 0x01, 0x00, 0xa0, 0x32, // configuration
    0x09, 0x04, 0x00, 0x00, 0x01, 0x03, 0x01, 0x01, 0x00, // interface 0: HID boot keyboard
    0x09, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, 0x3f, 0x00, // HID class descriptor
    0x07, 0x05, 0x81, 0x03, 0x08, 0x00, 0x0a, // endpoint 0x81 interrupt-IN
];

#[test]
fn device_descriptor_fields() {
    let mut parsed = walk(&DEVICE);
    let Some(Ok(Descriptor::Device(device))) = parsed.next() else {
        panic!("expected a device descriptor");
    };
    assert_eq!(device.class_code, 0);
    assert_eq!(device.max_packet0, 8);
    assert_eq!(device.vendor, 0x1d6b);
    assert_eq!(device.product, 0x0002);
    assert_eq!(device.configurations, 1);
    assert!(parsed.next().is_none());
}

#[test]
fn configuration_reports_total_length_and_value() {
    let Some(Ok(Descriptor::Configuration(configuration))) = walk(&CONFIGURATION).next() else {
        panic!("expected a configuration descriptor");
    };
    assert_eq!(configuration.total_length, 0x22);
    assert_eq!(configuration.value, 1);
    assert_eq!(configuration.interfaces, 1);
}

#[test]
fn keyboard_interface_and_interrupt_endpoint_are_found_in_order() {
    let descriptors: [Option<Result<Descriptor<'_>, ParseError>>; 4] = {
        let mut iterator = walk(&CONFIGURATION);
        [
            iterator.next(),
            iterator.next(),
            iterator.next(),
            iterator.next(),
        ]
    };
    let Some(Ok(Descriptor::Interface(interface))) = descriptors[1] else {
        panic!("expected the interface after the configuration");
    };
    assert_eq!(
        (interface.class_code, interface.subclass, interface.protocol),
        (3, 1, 1)
    );
    assert_eq!(interface.endpoints, 1);

    assert!(matches!(
        descriptors[2],
        Some(Ok(Descriptor::Other { kind: 0x21, .. }))
    ));
    let Some(Ok(Descriptor::Endpoint(endpoint))) = descriptors[3] else {
        panic!("expected the endpoint last");
    };
    assert_eq!(endpoint.number(), 1);
    assert!(endpoint.is_in());
    assert_eq!(endpoint.transfer_kind(), TransferKind::Interrupt);
    assert_eq!(endpoint.max_packet, 8);
    assert_eq!(endpoint.interval, 10);
}

#[test]
fn walk_stops_after_the_whole_stream() {
    assert_eq!(walk(&CONFIGURATION).count(), 4);
    assert_eq!(walk(&[]).count(), 0);
}

#[test]
fn length_below_two_is_bad_and_ends_the_stream() {
    let mut walker = walk(&[0x01, 0x02, 0x03]);
    assert_eq!(walker.next(), Some(Err(ParseError::BadLength)));
    assert_eq!(walker.next(), None);
}

#[test]
fn descriptor_running_past_the_stream_is_truncated() {
    // Claims 9 bytes, only 4 present.
    let mut walker = walk(&[0x09, 0x02, 0x22, 0x00]);
    assert_eq!(walker.next(), Some(Err(ParseError::Truncated)));
    assert_eq!(walker.next(), None);
}

#[test]
fn short_known_descriptor_is_bad_length() {
    // An interface descriptor needs 9 bytes; this claims 4.
    let mut walker = walk(&[0x04, 0x04, 0x00, 0x00]);
    assert_eq!(walker.next(), Some(Err(ParseError::BadLength)));
}

#[test]
fn endpoint_max_packet_drops_high_speed_transaction_bits() {
    // wMaxPacketSize 0x1008: size 8 in bits 0-10, two extra transactions per
    // microframe in bits 11-12 (0x1000 = 2 << 11). Only the size is kept.
    let Some(Ok(Descriptor::Endpoint(parsed))) =
        walk(&[0x07, 0x05, 0x81, 0x03, 0x08, 0x10, 0x04]).next()
    else {
        panic!("expected an endpoint");
    };
    assert_eq!(parsed.max_packet, 8);
}

#[test]
fn endpoint_direction_and_number_come_from_the_address() {
    let out = Endpoint {
        address: 0x02,
        attributes: 0x02,
        max_packet: 64,
        interval: 0,
    };
    assert!(!out.is_in());
    assert_eq!(out.number(), 2);
    assert_eq!(out.transfer_kind(), TransferKind::Bulk);
}
