//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Host tests of the DWC2 register fields, the channel helpers and the
//! control transfer stages.

#[cfg(test)]
mod tests {
    use {
        arbitrary_int::u4,
        usb_hcd_dwc2::{
            control,
            pipes::{MAX_PIPES, Pipe, PipeError, PipeTable, Route},
            regs::*,
        },
        vesper_usb_protocol::{descriptor::TransferKind, enumeration::Speed, setup::SetupPacket},
    };

    fn control_target(device_address: u8, max_packet: u16) -> Target {
        Target {
            device_address,
            endpoint: 0,
            input: false,
            low_speed: false,
            kind: TransferKind::Control,
            max_packet,
        }
    }

    #[test]
    fn channel_registers_are_strided_from_the_base() {
        assert_eq!(channel_offset(0), 0x500);
        assert_eq!(channel_offset(1), 0x520);
    }

    #[test]
    fn characteristics_of_an_out_control_endpoint_on_a_high_speed_device() {
        let value = characteristics(control_target(1, 8), true);
        assert_eq!(value.max_packet().value(), 8);
        assert_eq!(value.endpoint().value(), 0);
        assert!(!value.input());
        assert!(!value.low_speed());
        assert_eq!(value.kind(), EndpointKind::Control);
        assert_eq!(value.transactions().value(), 1, "one per microframe");
        assert_eq!(value.device_address().value(), 1);
        assert!(value.enable());
        assert!(!value.disable());
    }

    #[test]
    fn characteristics_of_an_in_interrupt_endpoint_on_a_low_speed_device() {
        let target = Target {
            device_address: 2,
            endpoint: 1,
            input: true,
            low_speed: true,
            kind: TransferKind::Interrupt,
            max_packet: 8,
        };
        let value = characteristics(target, false);
        assert_eq!(value.max_packet().value(), 8);
        assert_eq!(value.endpoint().value(), 1);
        assert!(value.input());
        assert!(value.low_speed());
        assert_eq!(value.kind(), EndpointKind::Interrupt);
        assert_eq!(value.device_address().value(), 2);
        assert!(!value.enable(), "left disabled");
    }

    #[test]
    fn characteristics_of_a_bulk_endpoint_is_type_two() {
        let target = Target {
            kind: TransferKind::Bulk,
            ..control_target(3, 512)
        };
        assert_eq!(characteristics(target, true).kind(), EndpointKind::Bulk);
    }

    #[test]
    fn characteristics_wire_form_places_the_fields_where_the_manual_does() {
        // MPS 8 | MC 1 << 20 | address 1 << 22 | enable (bit 31).
        let value = characteristics(control_target(1, 8), true);
        assert_eq!(value.raw_value(), 0x8 | (1 << 20) | (1 << 22) | (1 << 31));
    }

    #[test]
    fn transfer_size_carries_bytes_packets_and_pid() {
        let setup = transfer_size(8, 1, Pid::Setup);
        assert_eq!(setup.bytes().value(), 8);
        assert_eq!(setup.packets().value(), 1);
        assert_eq!(setup.pid(), Pid::Setup);

        assert_eq!(transfer_size(0, 1, Pid::Data1).pid(), Pid::Data1);
        assert_eq!(transfer_size(0, 1, Pid::Data0).pid(), Pid::Data0);
    }

    #[test]
    fn transfer_size_truncates_to_its_fields() {
        let value = transfer_size(0x8_0000, 0x400, Pid::Data1);
        assert_eq!(value.bytes().value(), 0, "bytes wrap at 19 bits");
        assert_eq!(value.packets().value(), 0, "packets wrap at 10 bits");
    }

    #[test]
    fn port_speed_decodes_the_three_known_values() {
        // Speed is bits 17 to 18 of the root port register; 3 is reserved.
        let at_speed = |speed: u32| PortControl::new_with_raw_value(speed << 17);
        assert_eq!(port_speed(at_speed(0)), Some(Speed::High));
        assert_eq!(port_speed(at_speed(1)), Some(Speed::Full));
        assert_eq!(port_speed(at_speed(2)), Some(Speed::Low));
        assert_eq!(port_speed(at_speed(3)), None);
    }

    #[test]
    fn port_write_sets_reset_and_keeps_power() {
        // Bit 0 is connected status (read-only), bit 12 is power.
        let current = PortControl::new_with_raw_value((1 << 12) | 1);
        let written = port_with_reset_and_power(current, true, true);
        assert!(written.reset());
        assert!(written.power());
    }

    #[test]
    fn port_write_does_not_acknowledge_events_by_accident() {
        // Writing back a read value would clear every event bit set in it.
        let current = PortControl::ZERO
            .with_enabled(true)
            .with_enable_changed(true)
            .with_connect_detected(true);
        let written = current.without_events();
        assert!(!written.enabled());
        assert!(!written.enable_changed());
        assert!(!written.connect_detected());
    }

    #[test]
    fn a_stall_outranks_a_transaction_error() {
        let interrupts = ChannelInterrupts::ZERO
            .with_stall(true)
            .with_transaction_error(true)
            .with_halted(true);
        assert_eq!(channel_outcome(interrupts), Some(ChannelOutcome::Stalled));
    }

    #[test]
    fn nak_and_transaction_errors_retry_and_babble_fails() {
        let nak = ChannelInterrupts::ZERO.with_nak(true).with_halted(true);
        assert_eq!(channel_outcome(nak), Some(ChannelOutcome::Retry));
        let transaction = ChannelInterrupts::ZERO.with_transaction_error(true);
        assert_eq!(channel_outcome(transaction), Some(ChannelOutcome::Retry));
        let babble = ChannelInterrupts::ZERO.with_babble(true);
        assert_eq!(channel_outcome(babble), Some(ChannelOutcome::Failed));
        let ahb = ChannelInterrupts::ZERO.with_ahb_error(true);
        assert_eq!(channel_outcome(ahb), Some(ChannelOutcome::Failed));
    }

    #[test]
    fn completion_and_halts_are_outcomes_and_nothing_pending_is_none() {
        let complete = ChannelInterrupts::ZERO.with_transfer_complete(true);
        assert_eq!(channel_outcome(complete), Some(ChannelOutcome::Complete));
        let halted = ChannelInterrupts::ZERO.with_halted(true);
        assert_eq!(channel_outcome(halted), Some(ChannelOutcome::Halted));
        assert_eq!(channel_outcome(ChannelInterrupts::ZERO), None);
    }

    #[test]
    fn receive_status_decodes_channel_bytes_and_packet_status() {
        // Channel 2, 18 bytes, packet status 2 (IN data), as the controller
        // packs them: channel bits 0 to 3, bytes 4 to 14, status 17 to 20.
        let status = ReceiveStatus::new_with_raw_value(2 | (18 << 4) | (2 << 17));
        assert_eq!(status.channel(), u4::new(2));
        assert_eq!(status.byte_count().value(), 18);
        assert_eq!(status.packet_status(), Ok(PacketStatus::InData));
    }

    #[test]
    fn receive_status_names_the_end_of_an_in_transfer_and_leaves_unknown_values_unnamed() {
        let complete = ReceiveStatus::new_with_raw_value(3 << 17);
        assert_eq!(complete.packet_status(), Ok(PacketStatus::InComplete));
        let halted = ReceiveStatus::new_with_raw_value(7 << 17);
        assert_eq!(halted.packet_status(), Ok(PacketStatus::ChannelHalted));
        let unused = ReceiveStatus::new_with_raw_value(6 << 17);
        assert!(unused.packet_status().is_err(), "the raw value is kept");
    }

    #[test]
    fn control_plan_and_packet_count_are_the_stage_rules() {
        let get = control::plan(SetupPacket::get_descriptor(1, 0, 18));
        assert!(get.has_data && get.data_input && !get.status_input);
        assert_eq!(control::packet_count(18, 8), 3);
        assert_eq!(control::packet_count(0, 64), 1);
    }

    #[test]
    fn port_status_bits_follow_the_interface() {
        assert_eq!(port_status_bits(PortControl::ZERO), 0);
        // Connected and enabled, with a connect change and an enable change
        // pending; the power and speed bits are not reported.
        let port =
            PortControl::new_with_raw_value(1 | 1 << 1 | 1 << 2 | 1 << 3 | 1 << 12 | 2 << 17);
        assert_eq!(port_status_bits(port), 1 | 1 << 1 | 1 << 16 | 1 << 17);
        let only_connected = PortControl::new_with_raw_value(1);
        assert_eq!(port_status_bits(only_connected), 1);
    }

    #[test]
    fn stage_length_needs_the_setup_packet_and_the_announced_data() {
        let get = SetupPacket::get_descriptor(1, 0, 18);
        assert_eq!(control::data_length(26, get, 4032), Ok(18));
        assert_eq!(control::data_length(4096, get, 4032), Ok(18));
        assert_eq!(
            control::data_length(25, get, 4032),
            Err(control::StageError::TooShort)
        );
        assert_eq!(
            control::data_length(7, get, 4032),
            Err(control::StageError::TooShort)
        );
        assert_eq!(
            control::data_length(0, get, 4032),
            Err(control::StageError::TooShort)
        );
        // No data stage: the setup packet alone is a whole stage.
        let set = SetupPacket::set_address(1);
        assert_eq!(control::data_length(8, set, 4032), Ok(0));
        // A data stage the controller's page cannot hold.
        let big = SetupPacket::get_descriptor(2, 0, 4033);
        assert_eq!(
            control::data_length(8 + 4033, big, 4032),
            Err(control::StageError::TooLong)
        );
        assert_eq!(
            control::data_length(8 + 4032, SetupPacket::get_descriptor(2, 0, 4032), 4032),
            Ok(4032)
        );
    }

    fn pipe(owner: u64) -> Pipe {
        Pipe {
            device_address: 0,
            endpoint: 0,
            speed: Speed::Full,
            kind: TransferKind::Control,
            max_packet: 8,
            owner,
            route: None,
        }
    }

    #[test]
    fn a_pipe_handle_is_never_zero_and_belongs_to_its_owner() {
        let mut table = PipeTable::new();
        let handle = table.open(pipe(7)).unwrap();
        assert_ne!(handle, 0);
        assert_eq!(table.get(handle, 7).unwrap().max_packet, 8);
        assert_eq!(table.get(handle, 8), Err(PipeError::Invalid));
        assert_eq!(table.get_mut(handle, 8).err(), Some(PipeError::Invalid));
        assert_eq!(table.close(handle, 8), Err(PipeError::Invalid));
        assert!(
            table.get(handle, 7).is_ok(),
            "a foreign close changed nothing"
        );
    }

    #[test]
    fn a_closed_pipes_handle_stays_dead_when_its_slot_is_reused() {
        let mut table = PipeTable::new();
        let old = table.open(pipe(1)).unwrap();
        table.close(old, 1).unwrap();
        assert_eq!(table.get(old, 1), Err(PipeError::Invalid));
        assert_eq!(table.close(old, 1), Err(PipeError::Invalid));
        let new = table.open(pipe(1)).unwrap();
        assert_ne!(new, old, "the generation moved on");
        assert_eq!(table.get(old, 1), Err(PipeError::Invalid));
        table.get(new, 1).unwrap();
    }

    #[test]
    fn a_full_table_refuses_and_a_closed_slot_is_reusable() {
        let mut table = PipeTable::new();
        let handles: Vec<u32> = (0..MAX_PIPES)
            .map(|_| table.open(pipe(1)).unwrap())
            .collect();
        assert_eq!(table.open(pipe(1)), Err(PipeError::Full));
        table.close(handles[3], 1).unwrap();
        table.open(pipe(2)).unwrap();
    }

    #[test]
    fn handles_outside_the_table_are_invalid() {
        let first_outside = u32::try_from(MAX_PIPES).unwrap();
        let mut table = PipeTable::new();
        for handle in [0, 1, u32::MAX, first_outside, 0x0001_0000 | first_outside] {
            assert_eq!(table.get(handle, 0), Err(PipeError::Invalid), "{handle:#x}");
            assert_eq!(
                table.close(handle, 0),
                Err(PipeError::Invalid),
                "{handle:#x}"
            );
        }
    }

    #[test]
    fn a_route_is_kept_on_its_pipe() {
        let mut table = PipeTable::new();
        let handle = table.open(pipe(5)).unwrap();
        table.get_mut(handle, 5).unwrap().route = Some(Route {
            hub_address: 1,
            hub_port: 3,
        });
        assert_eq!(
            table.get(handle, 5).unwrap().route,
            Some(Route {
                hub_address: 1,
                hub_port: 3
            })
        );
    }
}
