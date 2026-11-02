//! Host tests of the serial channel's layout and agreements.

#[cfg(test)]
mod tests {
    use {
        core::sync::atomic::Ordering,
        libio_channel::{IoRec, NoEvent, Packet},
        vesper_serial_channel::{
            CONTROL_FRAME_SLOT, CONTROL_VA, DATA_AREA, DATA_AREA_VA, DATA_FRAME_SLOT, DATA_PAGES,
            LOOPBACK, RECEIVE_COMPLETED_SLOT, SerialChannel, TRANSMIT_COMPLETED_SLOT,
            TRANSMIT_SUBMITTED_SLOT,
        },
    };

    #[test]
    fn an_all_zero_page_is_an_idle_channel() {
        let page = vec![0_u64; 4096 / 8];
        // SAFETY: a zeroed, aligned page that outlives the channel's use.
        let channel = unsafe { SerialChannel::at(page.as_ptr() as u64) };
        assert!(!channel.has(LOOPBACK));
        assert_eq!(channel.dropped.load(Ordering::Relaxed), 0);
        for fifo in [
            &channel.transmit.submit,
            &channel.transmit.complete,
            &channel.receive.submit,
            &channel.receive.complete,
        ] {
            assert!(fifo.is_empty());
        }
    }

    #[test]
    fn the_client_sets_options_before_the_driver_starts() {
        let channel = SerialChannel::new();
        channel.config.store(LOOPBACK, Ordering::Release);
        assert!(channel.has(LOOPBACK));
    }

    #[test]
    fn the_data_area_follows_the_control_page() {
        assert_eq!(DATA_AREA_VA, CONTROL_VA + 4096);
        assert_eq!(DATA_AREA.base, DATA_AREA_VA);
        assert_eq!(DATA_AREA.len, u64::from(DATA_PAGES) * 4096);
        assert!(DATA_AREA.contains(&IoRec {
            base: DATA_AREA_VA + u64::from(DATA_PAGES) * 4096 - 1,
            len: 1,
        }));
        assert!(!DATA_AREA.contains(&IoRec {
            base: CONTROL_VA,
            len: 1,
        }));
    }

    #[test]
    fn the_client_keys_take_distinct_slots() {
        let data_slots = DATA_FRAME_SLOT..DATA_FRAME_SLOT + DATA_PAGES;
        let others = [
            CONTROL_FRAME_SLOT,
            TRANSMIT_SUBMITTED_SLOT,
            TRANSMIT_COMPLETED_SLOT,
            RECEIVE_COMPLETED_SLOT,
        ];
        for (index, slot) in others.iter().enumerate() {
            assert!(!data_slots.contains(slot));
            assert!(!others[index + 1..].contains(slot));
            assert!(*slot < 256, "a driver table has 256 slots");
        }
    }

    #[test]
    fn both_channels_carry_packets_independently() {
        let channel = SerialChannel::new();
        let bytes = Packet::new(
            1,
            &[IoRec {
                base: DATA_AREA_VA,
                len: 5,
            }],
        )
        .expect("a packet");
        let (mut transmit, _) = channel.transmit.client(NoEvent, NoEvent);
        transmit.put(&bytes).expect("room");
        assert!(channel.receive.submit.is_empty());
        assert_eq!(channel.transmit.submit.get(), Ok(Some(bytes)));
    }
}
