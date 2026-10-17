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
        usb_hcd_dwc2::{control, regs::*},
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
}
