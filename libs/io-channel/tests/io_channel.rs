//! Host tests of IO channel FIFOs, packets and endpoints.

#[cfg(test)]
mod tests {
    use {
        core::{
            cell::Cell,
            sync::atomic::{AtomicU64, Ordering},
        },
        vesper_io_channel::{
            Channel, Corrupt, Cursor, DataArea, Event, Fifo, IoRec, MAX_PACKET_RECS, NoEvent,
            Packet, PutError, Unwaitable, WaitError,
        },
    };

    const SLOTS: usize = 16;

    fn rec(base: u64, len: u64) -> IoRec {
        IoRec { base, len }
    }

    fn packet(value: u64, recs: &[IoRec]) -> Packet {
        Packet::new(value, recs).expect("a well-formed packet")
    }

    /// The FIFO as raw words: produced, consumed, then two words per slot.
    fn words<const N: usize>(fifo: &Fifo<N>) -> &[AtomicU64] {
        // SAFETY: a `repr(C)` FIFO is nothing but `AtomicU64`s.
        unsafe {
            core::slice::from_raw_parts(
                core::ptr::from_ref(fifo).cast::<AtomicU64>(),
                size_of::<Fifo<N>>() / size_of::<AtomicU64>(),
            )
        }
    }

    #[test]
    fn a_packet_holds_one_to_max_records() {
        assert_eq!(Packet::new(1, &[]), None);
        assert_eq!(Packet::new(1, &[rec(0, 1); MAX_PACKET_RECS + 1]), None);
        let full = packet(7, &[rec(0x1000, 3); MAX_PACKET_RECS]);
        assert_eq!(full.recs().len(), MAX_PACKET_RECS);
        assert_eq!(full.bytes(), 3 * MAX_PACKET_RECS as u64);
        assert_eq!(full.value(), 7);
    }

    #[test]
    fn packets_come_out_in_order_with_their_records_and_values() {
        let fifo = Fifo::<SLOTS>::new();
        assert!(fifo.is_empty());
        let first = packet(1, &[rec(0x1000, 4)]);
        let second = packet(2, &[rec(0x2000, 1), rec(0x3000, 2), rec(0x4000, 3)]);
        fifo.put(&first).expect("room");
        fifo.put(&second).expect("room");
        assert!(!fifo.is_empty());
        assert_eq!(fifo.get(), Ok(Some(first)));
        assert_eq!(fifo.get(), Ok(Some(second)));
        assert_eq!(fifo.get(), Ok(None));
        assert!(fifo.is_empty());
    }

    #[test]
    fn a_packet_takes_a_header_slot_and_one_per_record() {
        let fifo = Fifo::<SLOTS>::new();
        // Three 4-record packets fill 15 of 16 slots.
        for value in 0..3 {
            fifo.put(&packet(value, &[rec(0, 1); 4])).expect("room");
        }
        assert!(fifo.has_room(0));
        assert!(!fifo.has_room(1));
        assert_eq!(
            fifo.put(&packet(9, &[rec(0, 1)])),
            Err(PutError::Full),
            "a full FIFO refuses a packet"
        );
        fifo.get().expect("consistent").expect("a packet");
        fifo.put(&packet(9, &[rec(0, 1)])).expect("room again");
    }

    #[test]
    fn a_fifo_wraps_around_many_times() {
        let fifo = Fifo::<SLOTS>::new();
        for value in 0..1000_u64 {
            let recs = [rec(value, value % 5 + 1); 3];
            let sent = packet(value, &recs[..=usize::try_from(value % 3).unwrap()]);
            fifo.put(&sent).expect("room");
            assert_eq!(fifo.get(), Ok(Some(sent)));
        }
        assert!(fifo.is_empty());
    }

    #[test]
    fn an_all_zero_fifo_is_empty() {
        let fifo = Fifo::<SLOTS>::new();
        assert!(
            words(&fifo)
                .iter()
                .all(|word| word.load(Ordering::Relaxed) == 0)
        );
        assert_eq!(fifo.get(), Ok(None));
    }

    #[test]
    fn a_garbled_header_is_skipped_with_everything_published() {
        let fifo = Fifo::<SLOTS>::new();
        fifo.put(&packet(1, &[rec(0, 1)])).expect("room");
        fifo.put(&packet(2, &[rec(0, 1)])).expect("room");
        // The producer claims the first packet has more records than exist.
        words(&fifo)[3].store(MAX_PACKET_RECS as u64 + 1, Ordering::Relaxed);
        assert_eq!(fifo.get(), Err(Corrupt));
        assert_eq!(fifo.get(), Ok(None), "the consumer resynchronised");
        fifo.put(&packet(3, &[rec(0, 1)])).expect("room");
        assert_eq!(fifo.get(), Ok(Some(packet(3, &[rec(0, 1)]))));
    }

    #[test]
    fn a_header_counting_beyond_what_was_published_is_corrupt() {
        let fifo = Fifo::<SLOTS>::new();
        fifo.put(&packet(1, &[rec(0, 1)])).expect("room");
        words(&fifo)[3].store(2, Ordering::Relaxed);
        assert_eq!(fifo.get(), Err(Corrupt));
    }

    #[test]
    fn impossible_counters_are_corrupt_on_both_sides() {
        let fifo = Fifo::<SLOTS>::new();
        // The producer claims more slots than the FIFO has.
        words(&fifo)[0].store(SLOTS as u64 + 1, Ordering::Relaxed);
        assert_eq!(fifo.put(&packet(1, &[rec(0, 1)])), Err(PutError::Corrupt));
        assert!(!fifo.has_room(0));
        assert_eq!(fifo.get(), Err(Corrupt));
        assert!(fifo.is_empty());
    }

    #[test]
    fn a_data_area_contains_only_spans_inside_it() {
        let area = DataArea {
            base: 0x1000,
            len: 0x2000,
        };
        assert!(area.contains(&rec(0x1000, 0x2000)));
        assert!(area.contains(&rec(0x2fff, 1)));
        assert!(area.contains(&rec(0x3000, 0)));
        assert!(!area.contains(&rec(0x0fff, 1)));
        assert!(!area.contains(&rec(0x2fff, 2)));
        assert!(!area.contains(&rec(0x1000, u64::MAX)));
        assert!(!area.contains(&rec(u64::MAX, 2)));
        assert!(area.holds(&packet(0, &[rec(0x1000, 1), rec(0x2000, 1)])));
        assert!(!area.holds(&packet(0, &[rec(0x1000, 1), rec(0x4000, 1)])));
    }

    #[test]
    fn a_cursor_walks_every_byte_of_every_record() {
        let walked = packet(0, &[rec(0x10, 2), rec(0x40, 0), rec(0x80, 1)]);
        let mut cursor = Cursor::START;
        let mut addresses = Vec::new();
        while let Some(address) = cursor.next_address(&walked) {
            addresses.push(address);
        }
        assert_eq!(addresses, [0x10, 0x11, 0x80]);
        assert!(cursor.is_at_end(&walked));
        assert_eq!(cursor.passed(&walked), 3);
        assert_eq!(cursor.next_address(&walked), None);
    }

    #[test]
    fn a_cursor_sees_trailing_empty_records_as_the_end() {
        let walked = packet(0, &[rec(0x10, 1), rec(0x40, 0)]);
        let mut cursor = Cursor::START;
        assert!(!cursor.is_at_end(&walked));
        assert_eq!(cursor.next_address(&walked), Some(0x10));
        assert!(cursor.is_at_end(&walked));
    }

    #[test]
    fn cutting_a_packet_keeps_only_the_bytes_passed() {
        let mut buffer = packet(4, &[rec(0x10, 3), rec(0x40, 5), rec(0x80, 2)]);
        let mut cursor = Cursor::START;
        for _ in 0..5 {
            cursor.next_address(&buffer).expect("a byte");
        }
        assert_eq!(cursor.passed(&buffer), 5);
        cursor.cut(&mut buffer);
        assert_eq!(
            buffer,
            packet(4, &[rec(0x10, 3), rec(0x40, 2), rec(0x80, 0)])
        );
        assert_eq!(buffer.bytes(), 5);

        let mut untouched = packet(4, &[rec(0x10, 3)]);
        Cursor::START.cut(&mut untouched);
        assert_eq!(untouched.bytes(), 0);
    }

    /// An event count playing the kernel's, single-threaded. An await of a
    /// count not yet reached plays the producer's turn: it puts the
    /// `refill` packet and advances, or fails the test if there is none,
    /// since nothing else could advance the count.
    #[derive(Default)]
    struct Counter<'fifo> {
        count: Cell<u64>,
        awaits: Cell<u32>,
        refill: Cell<Option<(&'fifo Fifo<SLOTS>, Packet)>>,
    }

    impl Event for &Counter<'_> {
        type Error = ();

        fn advance(&self) -> Result<(), ()> {
            self.count.set(self.count.get() + 1);
            Ok(())
        }

        fn await_at_least(&self, target: u64) -> Result<u64, ()> {
            self.awaits.set(self.awaits.get() + 1);
            if self.count.get() < target {
                let (fifo, packet) = self.refill.take().expect("would sleep forever");
                fifo.put(&packet).expect("room");
                self.advance()?;
            }
            Ok(self.count.get())
        }
    }

    #[test]
    fn a_producer_signals_once_per_batch() {
        let channel = Channel::<SLOTS>::new();
        let submitted = Counter::default();
        let (mut producer, _completions) = channel.client(&submitted, NoEvent);
        producer.signal().expect("advance");
        assert_eq!(submitted.count.get(), 0, "nothing put, nothing signalled");
        producer.put(&packet(1, &[rec(0, 1)])).expect("room");
        producer.put(&packet(2, &[rec(0, 1)])).expect("room");
        producer.signal().expect("advance");
        producer.signal().expect("advance");
        assert_eq!(submitted.count.get(), 1);
    }

    #[test]
    fn a_server_completes_what_its_client_submitted() {
        let channel = Channel::<SLOTS>::new();
        let (submitted, completed) = (Counter::default(), Counter::default());
        let (mut submit, mut completions) = channel.client(&submitted, &completed);
        let (mut requests, mut complete) = channel.server(&submitted, &completed);

        submit.put(&packet(5, &[rec(0x1000, 8)])).expect("room");
        submit.signal().expect("advance");
        let mut request = requests.wait().expect("the submitted packet");
        assert_eq!(request.value(), 5);
        request.recs_mut()[0].len = 3;
        complete.put(&request).expect("room");
        complete.signal().expect("advance");
        assert_eq!(
            completions.wait(),
            Ok(packet(5, &[rec(0x1000, 3)])),
            "the value comes back, with the length the server set"
        );
        assert_eq!(completions.get(), Ok(None));
    }

    #[test]
    fn a_consumer_drains_before_it_sleeps_and_survives_empty_signals() {
        let channel = Channel::<SLOTS>::new();
        let submitted = Counter::default();
        let (mut submit, _) = channel.client(&submitted, NoEvent);
        let (mut requests, _) = channel.server(&submitted, NoEvent);
        for value in 0..3 {
            submit.put(&packet(value, &[rec(0, 1)])).expect("room");
        }
        submit.signal().expect("advance");
        for value in 0..3 {
            assert_eq!(requests.wait().map(|packet| packet.value()), Ok(value));
        }
        assert_eq!(submitted.awaits.get(), 0, "queued packets need no sleep");

        // An advance with no packet behind it: the consumer looks, finds
        // nothing, and sleeps past it until the producer's next packet.
        (&submitted).advance().expect("advance");
        submitted
            .refill
            .set(Some((&channel.submit, packet(9, &[rec(0, 1)]))));
        assert_eq!(requests.wait().map(|packet| packet.value()), Ok(9));
        assert_eq!(submitted.awaits.get(), 2);
        assert_eq!(requests.get(), Ok(None));
    }

    #[test]
    fn waiting_on_no_event_fails_instead_of_sleeping() {
        let channel = Channel::<SLOTS>::new();
        let (mut requests, _) = channel.server(NoEvent, NoEvent);
        assert_eq!(requests.wait(), Err(WaitError::Event(Unwaitable)));
    }
}
