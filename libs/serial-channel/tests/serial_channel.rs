//! Host tests of the serial channel rings.

#[cfg(test)]
mod tests {
    use {
        core::sync::atomic::Ordering,
        vesper_serial_channel::{RING_BYTES, Ring, SerialChannel},
    };

    #[test]
    fn a_ring_returns_bytes_in_order() {
        let ring = Ring::new();
        assert!(ring.is_empty());
        for byte in [0_u8, 1, 0xff, 7] {
            assert!(ring.push(byte));
        }
        assert_eq!(ring.len(), 4);
        assert_eq!(ring.pop(), Some(0));
        assert_eq!(ring.pop(), Some(1));
        assert_eq!(ring.pop(), Some(0xff));
        assert_eq!(ring.pop(), Some(7));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn a_full_ring_refuses_bytes_until_one_is_read() {
        let ring = Ring::new();
        for index in 0..RING_BYTES {
            assert!(ring.push(index.to_le_bytes()[0]));
        }
        assert_eq!(ring.len(), RING_BYTES);
        assert!(!ring.push(0xaa));
        assert_eq!(ring.pop(), Some(0));
        assert!(ring.push(0xaa));
        assert!(!ring.push(0xbb));
    }

    #[test]
    fn a_ring_wraps_around_many_times() {
        let ring = Ring::new();
        let mut expected = 0_u32;
        let mut written = 0_u32;
        for _ in 0..10 {
            while ring.push(written.to_le_bytes()[0]) {
                written += 1;
            }
            while let Some(byte) = ring.pop() {
                assert_eq!(byte, expected.to_le_bytes()[0]);
                expected += 1;
            }
        }
        assert_eq!(expected, written);
        assert!(written as usize >= 10 * RING_BYTES);
    }

    #[test]
    fn the_counters_wrap_around_u32() {
        let ring = Ring::new();
        // Start both counters just below the wrap.
        // SAFETY: a `Ring` is `repr(C)` with the two counters first.
        let counters = unsafe { &*(&raw const ring).cast::<[core::sync::atomic::AtomicU32; 2]>() };
        counters[0].store(u32::MAX - 1, Ordering::Relaxed);
        counters[1].store(u32::MAX - 1, Ordering::Relaxed);
        assert!(ring.is_empty());
        for byte in 0..4 {
            assert!(ring.push(byte));
        }
        assert_eq!(ring.len(), 4);
        for byte in 0..4 {
            assert_eq!(ring.pop(), Some(byte));
        }
        assert!(ring.is_empty());
    }

    #[test]
    fn a_corrupt_counter_never_overfills_the_ring() {
        let ring = Ring::new();
        // SAFETY: as in `the_counters_wrap_around_u32`.
        let counters = unsafe { &*(&raw const ring).cast::<[core::sync::atomic::AtomicU32; 2]>() };
        // A consumer claiming to have read far ahead of the producer.
        counters[1].store(5_000, Ordering::Relaxed);
        assert!(!ring.push(1));
    }

    #[test]
    fn a_zeroed_page_is_an_empty_channel() {
        // SAFETY: every field of a channel is an atomic integer, for which
        // zero is a valid value.
        let channel: SerialChannel = unsafe { core::mem::zeroed() };
        assert!(channel.transmit.is_empty());
        assert!(channel.receive.is_empty());
        assert!(!channel.has(vesper_serial_channel::LOOPBACK));
        assert_eq!(channel.dropped.load(Ordering::Relaxed), 0);
    }
}
