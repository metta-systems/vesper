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
                Moderation, PortStatus, Ports, Structural2, neutral, port_offset, port_speed,
                port_status_bits, protocol_ports, split_address,
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
    fn the_moderation_interval_is_imods_low_half() {
        // IMODI in 15:0, IMODC in 31:16 (§5.5.2.2); the reset value is
        // 4000, 1 ms.
        assert_eq!(Moderation::ZERO.with_interval(160).raw_value(), 160);
        let found = Moderation::new_with_raw_value(0x0012_0fa0);
        assert_eq!((found.interval(), found.counter()), (4000, 0x12));
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

#[cfg(test)]
mod addressing_tests {
    use {
        usb_hcd_xhci::{
            context::{
                ADD_ENDPOINT0, ADD_SLOT, Hub, Position, Slot3, Translator, endpoint0_context,
                slot_context, speed_id,
            },
            trb::{ENDPOINT0, TransferType, Trb, code, kind},
        },
        vesper_usb_protocol::{enumeration::Speed, setup::SetupPacket},
    };

    /// A TRB's type, slot ID (control bits 31:24) and endpoint (20:16).
    fn kind_slot_endpoint(trb: Trb) -> (u8, u8, u8) {
        (trb.kind(), trb.slot(), trb.endpoint())
    }

    #[test]
    fn commands_carry_their_type_slot_and_input_context() {
        assert_eq!(Trb::enable_slot().kind(), kind::ENABLE_SLOT);
        assert_eq!(
            kind_slot_endpoint(Trb::disable_slot(5)),
            (kind::DISABLE_SLOT, 5, 0)
        );
        let address = Trb::address_device(0x4300_1000, 7);
        assert_eq!(kind_slot_endpoint(address), (kind::ADDRESS_DEVICE, 7, 0));
        assert_eq!(address.parameter, 0x4300_1000);
        // Block Set Address Request (bit 9) stays clear: the controller sends
        // SET_ADDRESS.
        assert_eq!(address.control & 1 << 9, 0);
        assert_eq!(
            Trb::configure_endpoint(0x2000, 3).kind(),
            kind::CONFIGURE_ENDPOINT
        );
        assert_eq!(
            Trb::evaluate_context(0x2000, 3).kind(),
            kind::EVALUATE_CONTEXT
        );
        assert_eq!(
            kind_slot_endpoint(Trb::reset_endpoint(2, ENDPOINT0)),
            (kind::RESET_ENDPOINT, 2, 1)
        );
        let dequeue = Trb::set_dequeue(0x5000_0040, true, 2, ENDPOINT0);
        assert_eq!(
            kind_slot_endpoint(dequeue),
            (kind::SET_TR_DEQUEUE_POINTER, 2, 1)
        );
        assert_eq!(dequeue.parameter, 0x5000_0041, "the cycle state in bit 0");
        assert_eq!(Trb::set_dequeue(0x40, false, 2, 1).parameter, 0x40);
    }

    #[test]
    fn a_control_transfer_is_a_setup_a_data_and_a_status_stage() {
        let setup = SetupPacket::get_descriptor(1, 0, 18).to_bytes();
        let stage = Trb::setup_stage(setup, TransferType::In);
        assert_eq!(stage.kind(), kind::SETUP_STAGE);
        assert_eq!(stage.parameter.to_le_bytes(), setup, "the packet inline");
        assert_eq!(stage.status, 8, "8 bytes of setup packet");
        assert_ne!(stage.control & 1 << 6, 0, "immediate data");
        assert_eq!(stage.control >> 16 & 3, 3, "an IN data stage follows");
        assert_eq!(
            Trb::setup_stage(setup, TransferType::NoData).control >> 16 & 3,
            0
        );
        let data = Trb::data_stage(0x6000, 18, true);
        assert_eq!(data.kind(), kind::DATA_STAGE);
        assert_eq!((data.parameter, data.status), (0x6000, 18));
        assert_ne!(data.control & 1 << 16, 0, "IN");
        assert_ne!(data.control & 1 << 2, 0, "a short packet raises an event");
        let status = Trb::status_stage(false);
        assert_eq!(status.kind(), kind::STATUS_STAGE);
        assert_eq!(status.control & 1 << 16, 0, "OUT after an IN data stage");
        assert_ne!(
            status.control & 1 << 5,
            0,
            "its completion raises the event"
        );
    }

    #[test]
    fn a_transfer_event_reports_its_residual_and_endpoint() {
        let event = Trb {
            parameter: 0x6000,
            status: u32::from(code::SHORT_PACKET) << 24 | 6,
            control: 4 << 24 | 1 << 16 | u32::from(kind::TRANSFER_EVENT) << 10 | 1,
        };
        assert_eq!(event.completion_code(), code::SHORT_PACKET);
        assert_eq!(event.residual(), 6);
        assert_eq!((event.slot(), event.endpoint()), (4, ENDPOINT0));
    }

    #[test]
    fn route_strings_and_translators_follow_the_hubs() {
        let root = Position::root(1);
        // The VIA hub on root port 1, high speed.
        let hub = Position::root(1);
        // A full-speed keyboard on port 3 of the high-speed hub in slot 1:
        // that port's translator.
        let keyboard = hub.behind(1, Speed::High, 3, Speed::Full).unwrap();
        assert_eq!(keyboard.route, 0x3);
        assert_eq!(keyboard.tiers, 1);
        assert_eq!(keyboard.root_port, 1);
        assert_eq!(
            keyboard.translator,
            Some(Translator {
                hub_slot: 1,
                port: 3
            })
        );
        // A high-speed hub on port 2, and a low-speed mouse on its port 4: the
        // inner hub's translator, the route one tier deeper.
        let inner = root.behind(1, Speed::High, 2, Speed::High).unwrap();
        assert_eq!(inner.translator, None);
        let mouse = inner.behind(2, Speed::High, 4, Speed::Low).unwrap();
        assert_eq!(mouse.route, 0x42);
        assert_eq!(
            mouse.translator,
            Some(Translator {
                hub_slot: 2,
                port: 4
            })
        );
        // Behind a full-speed hub, its own translator is handed on.
        let full_hub = keyboard;
        let behind = full_hub.behind(3, Speed::Full, 1, Speed::Low).unwrap();
        assert_eq!(behind.translator, keyboard.translator);
        // Ports past 15 take 15; port 0 and a sixth tier do not exist.
        assert_eq!(
            root.behind(1, Speed::High, 20, Speed::High).unwrap().route,
            15
        );
        assert_eq!(root.behind(1, Speed::High, 0, Speed::High), None);
        let mut deep = root;
        for _ in 0..5 {
            deep = deep.behind(1, Speed::High, 1, Speed::High).unwrap();
        }
        assert_eq!(deep.route, 0x11111);
        assert_eq!(deep.behind(1, Speed::High, 1, Speed::High), None);
    }

    #[test]
    fn a_slot_context_holds_speed_route_root_port_and_translator() {
        let keyboard = Position {
            root_port: 1,
            route: 0x3,
            tiers: 1,
            translator: Some(Translator {
                hub_slot: 1,
                port: 3,
            }),
        };
        let [word0, word1, word2, word3] = slot_context(keyboard, Speed::Full, None, 1);
        assert_eq!(word0 & 0xf_ffff, 0x3, "route string");
        assert_eq!(word0 >> 20 & 0xf, u32::from(speed_id(Speed::Full)));
        assert_eq!(word0 >> 27, 1, "endpoint 0 is the last context");
        assert_eq!(word0 & 1 << 26, 0, "not a hub");
        assert_eq!(word1 >> 16 & 0xff, 1, "root port");
        assert_eq!(word2 & 0xffff, 3 << 8 | 1, "translator port and hub slot");
        assert_eq!(word3, 0);
        assert_eq!(
            [
                speed_id(Speed::Full),
                speed_id(Speed::Low),
                speed_id(Speed::High)
            ],
            [1, 2, 3]
        );
    }

    #[test]
    fn a_hub_slot_context_holds_its_ports_and_think_time() {
        let hub = Hub {
            ports: 4,
            think_time: 16,
            multi_tt: true,
        };
        let [word0, word1, word2, _] = slot_context(Position::root(1), Speed::High, Some(hub), 1);
        assert_ne!(word0 & 1 << 26, 0, "a hub");
        assert_ne!(word0 & 1 << 25, 0, "multi-TT");
        assert_eq!(word1 >> 24, 4, "ports");
        assert_eq!(word2 >> 16 & 3, 1, "16 FS bit times");
        for (bits, encoded) in [(8, 0), (16, 1), (24, 2), (32, 3)] {
            let hub = Hub {
                think_time: bits,
                ..hub
            };
            let [_, _, word2, _] = slot_context(Position::root(1), Speed::High, Some(hub), 1);
            assert_eq!(word2 >> 16 & 3, encoded, "{bits} bit times");
        }
    }

    #[test]
    fn endpoint0_is_a_control_endpoint_on_its_ring() {
        let [word0, word1, low, high, average] = endpoint0_context(64, 0x1_2345_6000);
        assert_eq!(word0, 0);
        assert_eq!(word1 >> 1 & 3, 3, "three retries");
        assert_eq!(word1 >> 3 & 7, 4, "control endpoint");
        assert_eq!(word1 >> 16, 64, "max packet");
        assert_eq!(low, 0x2345_6001, "dequeue pointer with cycle state 1");
        assert_eq!(high, 1);
        assert_eq!(average, 8);
        assert_eq!((ADD_SLOT, ADD_ENDPOINT0), (1, 2));
    }

    #[test]
    fn the_enqueue_pointer_follows_the_ring_round_its_link() {
        use usb_hcd_xhci::ring::Producer;
        let mut ring = Producer::new(0x9000, 4);
        assert_eq!(ring.enqueue_pointer(), (0x9000, true));
        ring.push(Trb::status_stage(true));
        assert_eq!(ring.enqueue_pointer(), (0x9010, true));
        ring.push(Trb::status_stage(true));
        // The third TRB fills the segment up to its link: the next goes at
        // the start with the cycle bit flipped.
        let (_, link) = ring.push(Trb::status_stage(true));
        assert!(link.is_some());
        assert_eq!(ring.enqueue_pointer(), (0x9000, false));
        assert_eq!(
            kind_slot_endpoint(Trb::stop_endpoint(3, ENDPOINT0)),
            (kind::STOP_ENDPOINT, 3, 1)
        );
    }

    #[test]
    fn pipe_handles_are_never_zero_and_die_with_their_device() {
        use usb_hcd_xhci::pipes::{MAX_PIPES, Pipe, PipeError, PipeTable, index_of};
        let pipe = |device_address| Pipe {
            device_address,
            context_index: 1,
            max_packet: None,
            interrupt: None,
            owner: 0,
        };
        let mut table = PipeTable::new();
        let first = table.open(pipe(1)).unwrap();
        let second = table.open(pipe(2)).unwrap();
        assert!(first != 0 && second != 0 && first != second);
        table.get_mut(first).unwrap().max_packet = Some(64);
        assert_eq!(table.get_mut(first).unwrap().max_packet, Some(64));
        table.close(first).unwrap();
        assert_eq!(table.close(first), Err(PipeError::Invalid));
        let reused = table.open(pipe(1)).unwrap();
        assert_ne!(reused, first, "the generation moved on");
        assert_eq!(table.get_mut(first), Err(PipeError::Invalid));
        table.close_device(1);
        assert_eq!(table.get_mut(reused), Err(PipeError::Invalid));
        table.get_mut(second).unwrap();
        // The open pipes are listed with their handles; each handle names
        // its own table index.
        let listed: Vec<u32> = table.open_pipes().map(|(handle, _)| handle).collect();
        assert_eq!(listed, [second]);
        assert!(index_of(second) < MAX_PIPES);
        for _ in 1..MAX_PIPES {
            table.open(pipe(3)).unwrap();
        }
        assert_eq!(table.open(pipe(3)), Err(PipeError::Full));
        let mut indices: Vec<usize> = table
            .open_pipes()
            .map(|(handle, _)| index_of(handle))
            .collect();
        indices.sort_unstable();
        assert_eq!(indices, (0..MAX_PIPES).collect::<Vec<_>>());
    }

    #[test]
    fn an_interrupt_pipe_wants_a_buffer_only_when_idle_and_not_halted() {
        use usb_hcd_xhci::{pipes::Interrupt, ring::Producer};
        let idle = Interrupt {
            ring: Producer::new(0x8000, 64),
            max_packet: 8,
            in_flight: None,
            halted: false,
        };
        assert!(idle.wants_buffer());
        assert!(
            !Interrupt {
                in_flight: Some(0x8000),
                ..idle
            }
            .wants_buffer()
        );
        assert!(
            !Interrupt {
                halted: true,
                ..idle
            }
            .wants_buffer()
        );
    }

    #[test]
    fn endpoints_map_to_their_device_context_indices() {
        use usb_hcd_xhci::context::{context_flag, context_index};
        assert_eq!(context_index(0x00), 1, "endpoint 0");
        assert_eq!(context_index(0x80), 1, "endpoint 0 either way");
        assert_eq!(context_index(0x81), 3, "IN 1");
        assert_eq!(context_index(0x01), 2, "OUT 1");
        assert_eq!(context_index(0x8f), 31, "IN 15");
        assert_eq!(context_flag(3), 1 << 3);
    }

    #[test]
    fn interrupt_intervals_round_down_to_powers_of_two_microframes() {
        use usb_hcd_xhci::context::interval_exponent;
        // 125 µs × 2^n: 1 ms is 2^3, 8 ms 2^6, 10 ms rounds down to 8 ms.
        assert_eq!(interval_exponent(Speed::Full, 1), 3);
        assert_eq!(interval_exponent(Speed::Low, 8), 6);
        assert_eq!(interval_exponent(Speed::Low, 10), 6);
        assert_eq!(interval_exponent(Speed::High, 1), 3);
        // Full and low speed span 1 to 128 frames (3 to 10).
        assert_eq!(interval_exponent(Speed::Full, 255), 10);
        assert_eq!(interval_exponent(Speed::Full, 0), 3);
        // High speed goes down to a microframe.
        assert_eq!(interval_exponent(Speed::High, 0), 0);
        assert_eq!(interval_exponent(Speed::High, 255), 10);
    }

    #[test]
    fn an_interrupt_in_context_names_its_kind_interval_and_payload() {
        use usb_hcd_xhci::context::interrupt_in_context;
        let [word0, word1, low, high, word4] = interrupt_in_context(8, 6, 0x4_0000_2000);
        assert_eq!(word0 >> 16 & 0xff, 6, "interval");
        assert_eq!(word1 >> 3 & 7, 7, "interrupt IN");
        assert_eq!(word1 >> 1 & 3, 3, "three retries");
        assert_eq!(word1 >> 16, 8, "max packet");
        assert_eq!((low, high), (0x2001, 4), "ring with cycle state 1");
        assert_eq!(word4, 8 << 16 | 8, "payload and average TRB");
        // Endpoint 3 (IN 1) as the last context.
        let [word0, ..] = slot_context(Position::root(1), Speed::Low, None, 3);
        assert_eq!(word0 >> 27, 3);
        let normal = Trb::normal(0x2800, 8);
        assert_eq!(normal.kind(), kind::NORMAL);
        assert_eq!((normal.parameter, normal.status), (0x2800, 8));
        assert_ne!(normal.control & 1 << 5, 0, "IOC");
        assert_ne!(normal.control & 1 << 2, 0, "ISP");
    }

    #[test]
    fn the_controller_writes_the_address_and_state_into_word_3() {
        let word = Slot3::new_with_raw_value(2 << 27 | 5);
        assert_eq!(word.address(), 5);
        assert_eq!(word.state().value(), 2, "addressed");
    }
}
