//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Host tests of the xHCI register fields, the TRB format and the rings.

#[cfg(test)]
mod tests {
    use {
        usb_hcd_xhci::{
            regs::{
                PortStatus, Ports, Structural2, neutral, port_offset, port_speed, port_status_bits,
                protocol_ports, split_address,
            },
            ring::{Consumer, Producer},
            trb::{SUCCESS, TRB_BYTES, Trb, kind, segment_table_entry},
        },
        vesper_usb_protocol::enumeration::Speed,
    };

    /// `PORTSC` bits by name, as §5.4.8 numbers them.
    const CONNECTED: u32 = 1 << 0;
    const ENABLED: u32 = 1 << 1;
    const RESET: u32 = 1 << 4;
    const POWER: u32 = 1 << 9;
    const CONNECT_CHANGED: u32 = 1 << 17;
    const ENABLE_CHANGED: u32 = 1 << 18;
    const RESET_CHANGED: u32 = 1 << 21;
    const WAKE_ON_CONNECT: u32 = 1 << 25;

    fn speed_bits(speed: u32) -> u32 {
        speed << 10
    }

    #[test]
    fn writing_a_port_back_neutral_keeps_power_and_acknowledges_nothing() {
        let status = PortStatus::new_with_raw_value(
            CONNECTED
                | ENABLED
                | POWER
                | speed_bits(3)
                | CONNECT_CHANGED
                | ENABLE_CHANGED
                | RESET_CHANGED
                | WAKE_ON_CONNECT,
        );
        // PED and the change bits act on a 1: written back as read they would
        // disable the port and acknowledge its changes.
        assert_eq!(neutral(status).raw_value(), POWER | WAKE_ON_CONNECT);
        assert_eq!(
            neutral(status).with_reset(true).raw_value(),
            POWER | WAKE_ON_CONNECT | RESET
        );
        assert_eq!(
            neutral(status).with_reset_changed(true).raw_value(),
            POWER | WAKE_ON_CONNECT | RESET_CHANGED
        );
    }

    #[test]
    fn port_speeds_are_the_default_protocol_speed_ids() {
        let with_speed = |id| PortStatus::new_with_raw_value(CONNECTED | speed_bits(id));
        assert_eq!(port_speed(with_speed(1)), Some(Speed::Full));
        assert_eq!(port_speed(with_speed(2)), Some(Speed::Low));
        assert_eq!(port_speed(with_speed(3)), Some(Speed::High));
        // SuperSpeed and unassigned IDs have no usb_hcd_v1 speed.
        assert_eq!(port_speed(with_speed(4)), None);
        assert_eq!(port_speed(with_speed(0)), None);
    }

    #[test]
    fn port_status_bits_follow_usb_hcd_v1() {
        assert_eq!(port_status_bits(PortStatus::new_with_raw_value(0)), 0);
        assert_eq!(
            port_status_bits(PortStatus::new_with_raw_value(
                CONNECTED | ENABLED | CONNECT_CHANGED
            )),
            1 | 1 << 1 | 1 << 16
        );
        // A finished reset counts as the enable change.
        assert_eq!(
            port_status_bits(PortStatus::new_with_raw_value(RESET_CHANGED)),
            1 << 17
        );
        assert_eq!(
            port_status_bits(PortStatus::new_with_raw_value(ENABLE_CHANGED)),
            1 << 17
        );
    }

    #[test]
    fn port_register_sets_follow_the_operational_registers() {
        assert_eq!(port_offset(1), 0x400);
        assert_eq!(port_offset(5), 0x440);
    }

    #[test]
    fn scratchpad_count_joins_its_two_halves() {
        // Low five bits in 31:27, high five in 25:21.
        assert_eq!(
            Structural2::new_with_raw_value(4 << 27).scratchpad_buffers(),
            4
        );
        assert_eq!(
            Structural2::new_with_raw_value(1 << 21 | 2 << 27).scratchpad_buffers(),
            34
        );
        assert_eq!(Structural2::new_with_raw_value(0).scratchpad_buffers(), 0);
    }

    #[test]
    fn addresses_split_into_low_and_high_words() {
        assert_eq!(split_address(0x1_2345_6780), (0x2345_6780, 1));
    }

    #[test]
    fn trbs_round_trip_through_their_words() {
        let trb = Trb {
            parameter: 0x1122_3344_5566_7788,
            status: 0x0100_0000,
            control: 0x8400_8401,
        };
        assert_eq!(
            trb.words(),
            [0x5566_7788, 0x1122_3344, 0x0100_0000, 0x8400_8401]
        );
        assert_eq!(Trb::from_words(trb.words()), trb);
        assert_eq!(trb.kind(), kind::COMMAND_COMPLETION);
        assert_eq!(trb.completion_code(), SUCCESS);
        assert_eq!(trb.slot(), 0x84);
        assert!(trb.cycle());
    }

    #[test]
    fn a_no_op_command_and_a_link_carry_their_types() {
        let no_op = Trb::no_op_command();
        assert_eq!(no_op.kind(), kind::NO_OP_COMMAND);
        assert!(!no_op.cycle());
        assert!(no_op.with_cycle(true).cycle());
        assert!(!no_op.with_cycle(true).with_cycle(false).cycle());
        let link = Trb::link(0x4000);
        assert_eq!(link.kind(), kind::LINK);
        assert_eq!(link.parameter, 0x4000);
        assert!(link.toggles_cycle());
    }

    #[test]
    fn a_port_status_change_names_its_port() {
        let event = Trb {
            parameter: 3 << 24,
            status: u32::from(SUCCESS) << 24,
            control: u32::from(kind::PORT_STATUS_CHANGE) << 10 | 1,
        };
        assert_eq!(event.kind(), kind::PORT_STATUS_CHANGE);
        assert_eq!(event.port(), 3);
    }

    #[test]
    fn the_producer_links_back_and_flips_its_cycle() {
        let base = 0x10_0000;
        // Four TRBs: three to fill, the fourth the link.
        let mut ring = Producer::new(base, 4);
        let (first, link) = ring.push(Trb::no_op_command());
        assert_eq!((first.index, first.bus, link), (0, base, None));
        assert!(first.trb.cycle());
        let (second, link) = ring.push(Trb::no_op_command());
        assert_eq!(
            (second.index, second.bus, link),
            (1, base + TRB_BYTES, None)
        );
        // The third fills the segment up to the link, which comes with it,
        // in the same pass's cycle.
        let (third, link) = ring.push(Trb::no_op_command());
        assert_eq!(third.index, 2);
        let link = link.expect("the link after the last TRB");
        assert_eq!((link.index, link.bus), (3, base + 3 * TRB_BYTES));
        assert_eq!(link.trb.kind(), kind::LINK);
        assert_eq!(link.trb.parameter, base);
        assert!(link.trb.cycle());
        // The next pass starts at the beginning with the cycle bit clear.
        let (again, link) = ring.push(Trb::no_op_command());
        assert_eq!((again.index, link), (0, None));
        assert!(!again.trb.cycle());
    }

    #[test]
    fn the_consumer_takes_events_of_its_cycle_and_wraps() {
        let base = 0x20_0000;
        let mut ring = Consumer::new(base, 2);
        let event = |cycle| Trb::no_op_command().with_cycle(cycle);
        // The segment starts zeroed: no event is the controller's yet.
        assert!(!ring.owns(&Trb::default()));
        assert!(ring.owns(&event(true)));
        ring.advance();
        assert_eq!(ring.dequeue_pointer(), base + TRB_BYTES);
        ring.advance();
        // Past the end it wraps, and last pass's TRBs are old.
        assert_eq!((ring.index(), ring.dequeue_pointer()), (0, base));
        assert!(!ring.owns(&event(true)));
        assert!(ring.owns(&event(false)));
    }

    #[test]
    fn a_segment_table_entry_holds_the_base_and_size() {
        assert_eq!(segment_table_entry(0x1_0000_2000, 256), [0x2000, 1, 256, 0]);
    }

    /// BAR0 words: a USB Legacy Support capability at 0x100 pointing on to a
    /// USB 3 and then a USB 2 Supported Protocol capability.
    fn bar(offset: usize) -> Option<u32> {
        match offset {
            // ID 1, next 4 words on.
            0x100 => Some(0x0000_0401),
            // ID 2, next 4 words on, USB 3.0.
            0x110 => Some(0x0300_0402),
            0x118 => Some(0x0000_0101),
            // ID 2, last, USB 2.0: ports 2 to 5.
            0x120 => Some(0x0200_0002),
            0x128 => Some(0x0000_0402),
            offset if offset < 0x1000 => Some(0),
            _ => None,
        }
    }

    #[test]
    fn supported_protocol_capabilities_name_the_port_ranges() {
        let usb2 = protocol_ports(bar, 0x100 / 4, 2).expect("USB 2 ports");
        assert_eq!(
            usb2,
            Ports {
                major: 2,
                first: 2,
                count: 4
            }
        );
        assert_eq!(usb2.port_id(0), Some(2));
        assert_eq!(usb2.port_id(3), Some(5));
        assert_eq!(usb2.port_id(4), None);
        let usb3 = protocol_ports(bar, 0x100 / 4, 3).expect("USB 3 ports");
        assert_eq!((usb3.first, usb3.count), (1, 1));
        // No list, and a list without the revision.
        assert_eq!(protocol_ports(bar, 0, 2), None);
        assert_eq!(protocol_ports(bar, 0x120 / 4, 3), None);
    }
}
