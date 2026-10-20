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
            pipes::{Config, ConfigError, MAX_PIPES, Pipe, PipeError, PipeTable, Route},
            regs::*,
            split::{
                Event, MAX_NYET, MAX_RESTARTS, Phase, STATUS_COMPLETE, STATUS_SPLIT, Sequence,
                Step, event_of, failure_status, next_pid, packet_bytes,
            },
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
            split: None,
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
            split: None,
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
    fn the_channel_event_mask_is_exactly_the_bits_the_outcome_reads() {
        let mask = channel_event_mask();
        // Complete 0, halted 1, AHB error 2, stall 3, NAK 4, transaction error 7,
        // babble 8, data toggle error 10.
        assert_eq!(mask.raw_value(), 0x59F);
        for bit in 0..32 {
            let single = ChannelInterrupts::new_with_raw_value(1 << bit);
            let in_mask = mask.raw_value() & (1 << bit) != 0;
            assert_eq!(
                channel_outcome(single).is_some(),
                in_mask,
                "bit {bit}: the outcome reads it exactly when the mask lets it through"
            );
        }
    }

    #[test]
    fn host_channel_summary_bits_are_one_per_channel() {
        assert_eq!(host_channel_bit(0), 1);
        assert_eq!(host_channel_bit(7), 0x80);
        assert_eq!(host_channel_bit(15), 0x8000);
        assert_eq!(GLOBAL_HOST_CHANNEL, 0x0200_0000);
        // The summary bit `InterruptStatus` reads is the one the mask enables.
        assert!(InterruptStatus::new_with_raw_value(GLOBAL_HOST_CHANNEL).host_channel());
        assert!(!InterruptStatus::new_with_raw_value(GLOBAL_HOST_CHANNEL).host_port());
    }

    #[test]
    fn the_global_interrupt_enable_is_ahb_config_bit_zero() {
        assert_eq!(
            AhbConfig::ZERO.with_global_interrupt_mask(true).raw_value(),
            1
        );
        assert_eq!(
            AhbConfig::ZERO
                .with_global_interrupt_mask(true)
                .with_dma_enable(true)
                .raw_value(),
            0x21
        );
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

    fn pipe() -> Pipe {
        Pipe {
            device_address: 0,
            endpoint: 0,
            input: false,
            speed: Speed::Full,
            config: None,
            route: None,
            owner: 0,
        }
    }

    #[test]
    fn a_pipe_handle_is_never_zero() {
        let mut table = PipeTable::new();
        let handle = table.open(pipe()).unwrap();
        assert_ne!(handle, 0);
        assert_eq!(
            table.get(handle).unwrap().config,
            None,
            "a new pipe is unconfigured"
        );
    }

    #[test]
    fn a_pipe_is_configured_and_can_be_configured_again() {
        let mut table = PipeTable::new();
        let handle = table.open(pipe()).unwrap();
        for max_packet in [8, 64] {
            let config = Config::new(TransferKind::Control, max_packet, 0).unwrap();
            table.get_mut(handle).unwrap().config = Some(config);
            assert_eq!(table.get(handle).unwrap().config, Some(config));
        }
    }

    #[test]
    fn only_control_configurations_with_a_real_packet_size_are_served() {
        for max_packet in [8, 16, 32, 64] {
            Config::new(TransferKind::Control, max_packet, 0).unwrap();
        }
        for max_packet in [0, 7, 9, 128] {
            assert_eq!(
                Config::new(TransferKind::Control, max_packet, 0),
                Err(ConfigError::Invalid)
            );
        }
        assert_eq!(
            Config::new(TransferKind::Bulk, 64, 0),
            Err(ConfigError::NotServed)
        );
    }

    #[test]
    fn interrupt_configurations_need_a_packet_size_and_an_interval_in_range() {
        for (max_packet, interval) in [(1, 1), (8, 10), (64, 255), (1024, 8)] {
            let config = Config::new(TransferKind::Interrupt, max_packet, interval).unwrap();
            assert_eq!(config.interval_ms, interval);
        }
        for (max_packet, interval) in [(0, 10), (1025, 10), (8, 0), (8, 256)] {
            assert_eq!(
                Config::new(TransferKind::Interrupt, max_packet, interval),
                Err(ConfigError::Invalid)
            );
        }
    }

    #[test]
    fn a_nak_is_an_answer_to_a_poll() {
        let nak = ChannelInterrupts::ZERO.with_nak(true).with_halted(true);
        assert_eq!(poll_outcome(nak), Some(PollOutcome::Nothing));
        let data = ChannelInterrupts::ZERO
            .with_transfer_complete(true)
            .with_halted(true);
        assert_eq!(poll_outcome(data), Some(PollOutcome::Data));
        let stall = ChannelInterrupts::ZERO.with_stall(true).with_halted(true);
        assert_eq!(poll_outcome(stall), Some(PollOutcome::Stalled));
        for failure in [
            ChannelInterrupts::ZERO.with_transaction_error(true),
            ChannelInterrupts::ZERO.with_babble(true),
            ChannelInterrupts::ZERO.with_data_toggle_error(true),
            ChannelInterrupts::ZERO.with_frame_overrun(true),
            ChannelInterrupts::ZERO.with_halted(true),
        ] {
            assert_eq!(poll_outcome(failure), Some(PollOutcome::Failed));
        }
        assert_eq!(poll_outcome(ChannelInterrupts::ZERO), None);
        // The same NAK makes a control transfer try again.
        assert_eq!(channel_outcome(nak), Some(ChannelOutcome::Retry));
    }

    #[test]
    fn a_poll_that_is_nakked_has_no_data_and_is_not_started_over() {
        let mut poll = Sequence::poll();
        assert_eq!(poll.next(Event::Accepted), Step::Issue(Phase::Complete));
        assert_eq!(poll.next(Event::NotYet), Step::Issue(Phase::Complete));
        assert_eq!(poll.next(Event::Nak), Step::NoData);

        let mut early = Sequence::poll();
        assert_eq!(early.next(Event::Nak), Step::NoData);

        // A transfer that is not a poll starts over on a NAK.
        let mut transfer = Sequence::new();
        assert_eq!(transfer.next(Event::Nak), Step::Issue(Phase::Start));

        let mut data = Sequence::poll();
        data.next(Event::Accepted);
        assert_eq!(data.next(Event::Done), Step::Done);
    }

    mod periodic {
        use {
            usb_hcd_dwc2::periodic::{
                MAX_INTERVAL_MS, PERIODIC_BUDGET_US, Polled, Poller, ScheduleError, is_pollable,
                transaction_us,
            },
            vesper_usb_protocol::{descriptor::TransferKind, enumeration::Speed},
        };

        #[test]
        fn transactions_take_longer_on_slower_buses() {
            let low = transaction_us(Speed::Low, 8);
            let full = transaction_us(Speed::Full, 8);
            let high = transaction_us(Speed::High, 8);
            assert!(low > full && full > high, "{low} {full} {high}");
            // 21 bytes of 8 bits at 1.5 Mbit/s: 112 µs.
            assert_eq!(low, 112);
            assert_eq!(full, 14);
            assert_eq!(high, 1);
        }

        #[test]
        fn a_pipe_is_due_from_its_scheduling_and_every_interval_after() {
            let mut poller = Poller::new();
            poller.schedule(7, 8, 14, 100).unwrap();
            assert_eq!(poller.due(99), None);
            assert_eq!(poller.due(100), Some((7, false)));
            assert_eq!(poller.wait_ms(90), Some(10));
            assert_eq!(poller.wait_ms(100), Some(0));
            poller.polled(7, 100, Polled::Nothing);
            assert_eq!(poller.due(107), None);
            assert_eq!(poller.due(108), Some((7, false)), "no data, no toggle");
            assert_eq!(poller.wait_ms(101), Some(7));
        }

        #[test]
        fn data_flips_the_expected_pid_and_halt_clearing_resets_it() {
            let mut poller = Poller::new();
            poller.schedule(1, 4, 14, 0).unwrap();
            poller.polled(1, 0, Polled::Data);
            assert_eq!(poller.due(4), Some((1, true)));
            poller.polled(1, 4, Polled::Data);
            assert_eq!(poller.due(8), Some((1, false)));
            poller.polled(1, 8, Polled::Data);
            poller.reset_toggle(1);
            assert_eq!(poller.due(12), Some((1, false)));
        }

        #[test]
        fn a_pipe_that_fell_behind_is_polled_once_not_for_every_missed_interval() {
            let mut poller = Poller::new();
            poller.schedule(3, 2, 14, 0).unwrap();
            poller.polled(3, 1000, Polled::Nothing);
            assert_eq!(poller.due(1001), None);
            assert_eq!(poller.due(1002), Some((3, false)));
        }

        #[test]
        fn the_most_overdue_pipe_goes_first() {
            let mut poller = Poller::new();
            poller.schedule(1, 10, 14, 50).unwrap();
            poller.schedule(2, 10, 14, 20).unwrap();
            poller.schedule(3, 10, 14, 80).unwrap();
            assert_eq!(poller.due(100).map(|due| due.0), Some(2));
            poller.polled(2, 100, Polled::Nothing);
            assert_eq!(poller.due(100).map(|due| due.0), Some(1));
        }

        #[test]
        fn removing_a_pipe_stops_its_polls() {
            let mut poller = Poller::new();
            poller.schedule(5, 8, 14, 0).unwrap();
            assert!(poller.contains(5));
            poller.remove(5);
            assert!(!poller.contains(5));
            assert_eq!(poller.due(1000), None);
            assert_eq!(poller.wait_ms(0), None);
        }

        #[test]
        fn intervals_outside_one_to_the_maximum_are_refused() {
            let mut poller = Poller::new();
            assert_eq!(poller.schedule(1, 0, 14, 0), Err(ScheduleError::Invalid));
            assert_eq!(
                poller.schedule(1, MAX_INTERVAL_MS + 1, 14, 0),
                Err(ScheduleError::Invalid)
            );
            poller.schedule(1, MAX_INTERVAL_MS, 14, 0).unwrap();
        }

        #[test]
        fn the_schedule_refuses_a_pipe_that_does_not_fit_and_keeps_what_it_has() {
            let mut poller = Poller::new();
            // Low-speed 8-byte pipes at 112 µs every millisecond: seven fit
            // (784 µs of 800), the eighth does not.
            for handle in 0..7 {
                poller.schedule(handle, 1, 112, 0).unwrap();
            }
            assert_eq!(poller.used_us(), 784);
            assert_eq!(
                poller.schedule(7, 1, 112, 0),
                Err(ScheduleError::NoBandwidth)
            );
            // At a slower interval the same pipe costs less and fits.
            poller.schedule(7, 10, 112, 0).unwrap();
            assert_eq!(poller.used_us(), 784 + 12);
            // Scheduling a pipe again counts it once.
            poller.schedule(7, 10, 112, 0).unwrap();
            assert_eq!(poller.used_us(), 784 + 12);
            assert!(PERIODIC_BUDGET_US >= poller.used_us());
            // A refused change leaves the earlier schedule.
            assert_eq!(
                poller.schedule(7, 1, 112, 0),
                Err(ScheduleError::NoBandwidth)
            );
            assert_eq!(poller.used_us(), 784 + 12);
        }

        #[test]
        fn the_table_has_a_limit() {
            let mut poller = Poller::new();
            for handle in 0..16 {
                poller.schedule(handle, 100, 1, 0).unwrap();
            }
            assert_eq!(poller.schedule(16, 100, 1, 0), Err(ScheduleError::Full));
        }

        #[test]
        fn only_interrupt_in_endpoints_are_polled() {
            assert!(is_pollable(TransferKind::Interrupt, true));
            assert!(!is_pollable(TransferKind::Interrupt, false));
            assert!(!is_pollable(TransferKind::Bulk, true));
            assert!(!is_pollable(TransferKind::Control, true));
        }
    }

    #[test]
    fn a_closed_pipes_handle_stays_dead_when_its_slot_is_reused() {
        let mut table = PipeTable::new();
        let old = table.open(pipe()).unwrap();
        table.close(old).unwrap();
        assert_eq!(table.get(old), Err(PipeError::Invalid));
        assert_eq!(table.close(old), Err(PipeError::Invalid));
        let new = table.open(pipe()).unwrap();
        assert_ne!(new, old, "the generation moved on");
        assert_eq!(table.get(old), Err(PipeError::Invalid));
        table.get(new).unwrap();
    }

    #[test]
    fn a_full_table_refuses_and_a_closed_slot_is_reusable() {
        let mut table = PipeTable::new();
        let handles: Vec<u32> = (0..MAX_PIPES)
            .map(|_| table.open(pipe()).unwrap())
            .collect();
        assert_eq!(table.open(pipe()), Err(PipeError::Full));
        table.close(handles[3]).unwrap();
        table.open(pipe()).unwrap();
    }

    #[test]
    fn handles_outside_the_table_are_invalid() {
        let first_outside = u32::try_from(MAX_PIPES).unwrap();
        let mut table = PipeTable::new();
        for handle in [0, 1, u32::MAX, first_outside, 0x0001_0000 | first_outside] {
            assert_eq!(table.get(handle), Err(PipeError::Invalid), "{handle:#x}");
            assert_eq!(table.close(handle), Err(PipeError::Invalid), "{handle:#x}");
        }
    }

    #[test]
    fn a_route_is_kept_on_its_pipe() {
        let mut table = PipeTable::new();
        let handle = table.open(pipe()).unwrap();
        table.get_mut(handle).unwrap().route = Some(Route {
            hub_address: 1,
            hub_port: 3,
        });
        assert_eq!(
            table.get(handle).unwrap().route,
            Some(Route {
                hub_address: 1,
                hub_port: 3
            })
        );
    }

    fn behind_a_hub(hub_address: u8, hub_port: u8) -> Target {
        Target {
            split: Some(SplitRoute {
                hub_address,
                hub_port,
            }),
            low_speed: true,
            ..control_target(5, 8)
        }
    }

    #[test]
    fn split_control_wire_form_places_the_fields_where_the_manual_does() {
        // HCSPLT: port 3 | hub 9 << 7 | enable (bit 31). The position field stays
        // zero: USB 2.0 §8.4.2.2 has the E field of a control or interrupt
        // start split zero (the position is for isochronous OUT only).
        let start = split_control(behind_a_hub(9, 3), false);
        assert_eq!(start.raw_value(), 3 | (9 << 7) | (1 << 31));
        // A complete split also sets bit 16.
        let complete = split_control(behind_a_hub(9, 3), true);
        assert_eq!(complete.raw_value(), start.raw_value() | (1 << 16));
        // A device on the root port runs no splits.
        assert_eq!(split_control(control_target(1, 8), false), Split::ZERO);
        assert_eq!(split_control(control_target(1, 8), true), Split::ZERO);
    }

    #[test]
    fn the_split_event_mask_adds_exactly_the_split_events() {
        let plain = channel_event_mask();
        let split = split_event_mask();
        assert_eq!(
            split.raw_value() & !plain.raw_value(),
            (1 << 5) | (1 << 6) | (1 << 9),
            "ACK, NYET and frame overrun"
        );
        assert_eq!(split.raw_value() & plain.raw_value(), plain.raw_value());
    }

    fn events(configure: impl FnOnce(ChannelInterrupts) -> ChannelInterrupts) -> ChannelInterrupts {
        configure(ChannelInterrupts::ZERO)
    }

    #[test]
    fn channel_events_read_as_split_events_by_phase() {
        let start = |e: ChannelInterrupts| event_of(Phase::Start, e);
        let complete = |e: ChannelInterrupts| event_of(Phase::Complete, e);
        assert_eq!(start(ChannelInterrupts::ZERO), None);
        assert_eq!(start(events(|e| e.with_ack(true))), Some(Event::Accepted));
        assert_eq!(
            start(events(|e| e.with_ack(true).with_halted(true))),
            Some(Event::Accepted)
        );
        // An ACK of a complete split alone is not a result.
        assert_eq!(complete(events(|e| e.with_ack(true))), None);
        assert_eq!(
            complete(events(|e| e.with_transfer_complete(true).with_ack(true))),
            Some(Event::Done)
        );
        assert_eq!(complete(events(|e| e.with_nyet(true))), Some(Event::NotYet));
        assert_eq!(start(events(|e| e.with_nak(true))), Some(Event::Nak));
        assert_eq!(complete(events(|e| e.with_nak(true))), Some(Event::Nak));
        for error in [
            events(|e| e.with_transaction_error(true)),
            events(|e| e.with_frame_overrun(true)),
            events(|e| e.with_data_toggle_error(true)),
        ] {
            assert_eq!(start(error), Some(Event::Error));
            assert_eq!(complete(error), Some(Event::Error));
        }
        // A STALL outranks a result, a bus error outranks a result.
        let stalled = events(|e| e.with_stall(true).with_transfer_complete(true));
        assert_eq!(complete(stalled), Some(Event::Stalled));
        let broken = events(|e| e.with_ahb_error(true).with_transfer_complete(true));
        assert_eq!(complete(broken), Some(Event::Failed));
        assert_eq!(
            complete(events(|e| e.with_babble(true))),
            Some(Event::Failed)
        );
        // A completed transfer of a start split is not a result.
        assert_eq!(start(events(|e| e.with_transfer_complete(true))), None);
    }

    #[test]
    fn a_split_transaction_runs_start_then_complete_until_done() {
        let mut sequence = Sequence::new();
        assert_eq!(sequence.phase(), Phase::Start);
        assert_eq!(sequence.next(Event::Accepted), Step::Issue(Phase::Complete));
        assert_eq!(sequence.phase(), Phase::Complete);
        assert_eq!(sequence.next(Event::Done), Step::Done);
    }

    #[test]
    fn nyet_repeats_the_complete_split_then_starts_over() {
        let mut sequence = Sequence::new();
        sequence.next(Event::Accepted);
        for _ in 0..MAX_NYET {
            assert_eq!(sequence.next(Event::NotYet), Step::Issue(Phase::Complete));
        }
        assert_eq!(
            sequence.next(Event::NotYet),
            Step::Issue(Phase::Start),
            "the hub never had the result"
        );
        // The count starts over with the new complete split.
        sequence.next(Event::Accepted);
        assert_eq!(sequence.next(Event::NotYet), Step::Issue(Phase::Complete));
        assert_eq!(sequence.next(Event::Done), Step::Done);
    }

    #[test]
    fn an_error_starts_the_packet_over_until_given_up_and_a_nak_never_counts() {
        let mut sequence = Sequence::new();
        for _ in 0..MAX_RESTARTS {
            assert_eq!(sequence.next(Event::Error), Step::Issue(Phase::Start));
        }
        assert_eq!(sequence.next(Event::Error), Step::Failed);
        // USB 2.0 §11.17.1: a NAK leaves the error count alone.
        let mut sequence = Sequence::new();
        for _ in 0..=MAX_RESTARTS * 2 {
            assert_eq!(sequence.next(Event::Nak), Step::Issue(Phase::Start));
        }
        assert_eq!(sequence.restarts(), 0);
        // The same from the complete split.
        let mut sequence = Sequence::new();
        sequence.next(Event::Accepted);
        assert_eq!(sequence.next(Event::Nak), Step::Issue(Phase::Start));
        assert_eq!(sequence.phase(), Phase::Start);
    }

    #[test]
    fn a_stall_or_a_bus_error_ends_the_transaction() {
        for phase_events in [&[][..], &[Event::Accepted][..]] {
            let mut stalled = Sequence::new();
            let mut failed = Sequence::new();
            for event in phase_events {
                stalled.next(*event);
                failed.next(*event);
            }
            assert_eq!(stalled.next(Event::Stalled), Step::Stalled);
            assert_eq!(failed.next(Event::Failed), Step::Failed);
        }
    }

    #[test]
    fn events_a_phase_cannot_produce_start_the_packet_over() {
        let mut sequence = Sequence::new();
        assert_eq!(sequence.next(Event::Done), Step::Issue(Phase::Start));
        assert_eq!(sequence.next(Event::NotYet), Step::Issue(Phase::Start));
        sequence.next(Event::Accepted);
        assert_eq!(sequence.next(Event::Accepted), Step::Issue(Phase::Start));
    }

    #[test]
    fn packets_are_at_most_max_packet_bytes_and_pids_alternate() {
        assert_eq!(packet_bytes(0, 8), 0);
        assert_eq!(packet_bytes(5, 8), 5);
        assert_eq!(packet_bytes(8, 8), 8);
        assert_eq!(packet_bytes(4032, 64), 64);
        assert_eq!(next_pid(Pid::Setup), Pid::Data1);
        assert_eq!(next_pid(Pid::Data1), Pid::Data0);
        assert_eq!(next_pid(Pid::Data0), Pid::Data1);
    }

    #[test]
    fn a_failed_split_transaction_reports_its_phase_above_the_channel_events() {
        let events = events(|e| e.with_nak(true).with_halted(true)).raw_value();
        let start = failure_status(events, Phase::Start);
        let complete = failure_status(events, Phase::Complete);
        assert_eq!(start & !STATUS_SPLIT, events);
        assert_eq!(start & STATUS_COMPLETE, 0);
        assert_eq!(complete, events | STATUS_SPLIT | STATUS_COMPLETE);
        // The marker bits are above every channel event bit.
        assert!(events < STATUS_COMPLETE);
    }
}
