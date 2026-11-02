use vesper_usb_protocol::setup::*;

#[test]
fn get_device_descriptor_wire_form() {
    // GET_DESCRIPTOR(device, 18 bytes): 80 06 00 01 00 00 12 00.
    let packet = SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18);
    assert_eq!(
        packet.to_bytes(),
        [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00]
    );
}

#[test]
fn get_configuration_descriptor_puts_index_in_low_byte() {
    // The value word is (descriptor type << 8) | index.
    let packet = SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 2, 0x0100);
    assert_eq!(packet.value, 0x0202);
    assert_eq!(packet.to_bytes()[6..], [0x00, 0x01]);
}

#[test]
fn set_address_wire_form() {
    // SET_ADDRESS(5): 00 05 05 00 00 00 00 00.
    assert_eq!(
        SetupPacket::set_address(5).to_bytes(),
        [0x00, 0x05, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
}

#[test]
fn set_configuration_wire_form() {
    // SET_CONFIGURATION(1): 00 09 01 00 00 00 00 00.
    assert_eq!(
        SetupPacket::set_configuration(1).to_bytes(),
        [0x00, 0x09, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
}

#[test]
fn from_bytes_inverts_to_bytes() {
    for packet in [
        SetupPacket::get_descriptor(DESCRIPTOR_DEVICE, 0, 18),
        SetupPacket::get_descriptor(DESCRIPTOR_CONFIGURATION, 2, 0x0100),
        SetupPacket::set_address(5),
        SetupPacket::set_configuration(1),
    ] {
        assert_eq!(SetupPacket::from_bytes(packet.to_bytes()), packet);
    }
    // Index and length are separate little-endian words.
    let packet = SetupPacket::from_bytes([0xa3, 0x00, 0x00, 0x00, 0x34, 0x12, 0x78, 0x56]);
    assert_eq!((packet.index, packet.length), (0x1234, 0x5678));
}
