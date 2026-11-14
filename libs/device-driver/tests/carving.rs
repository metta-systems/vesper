//! Host tests of carving a Untyped's watermark forward.

#[cfg(test)]
mod tests {
    use vesper_device_driver::skip_chunks;

    #[test]
    fn skipping_carves_aligned_chunks() {
        assert_eq!(skip_chunks(0, 0x10_0000).collect::<Vec<_>>(), [20]);
        assert_eq!(skip_chunks(0, 0).count(), 0);
        assert_eq!(skip_chunks(0x1000, 0x3000).collect::<Vec<_>>(), [12, 12]);
        assert_eq!(skip_chunks(0, 0x3000).collect::<Vec<_>>(), [13, 12]);
        for (from, to) in [
            (0, 0x12c_000),
            (0x1000, 0x10_0000),
            (0x3000, 0x20_0000),
            (0x7_0000, 0x7_1000),
            (0x1_0010, 0x4_0000),
            (0, 0x3fff_f000),
        ] {
            let mut cursor = from;
            let mut chunks = 0;
            for bits in skip_chunks(from, to) {
                assert!(bits >= 4, "a chunk is at least an Untyped of 16 bytes");
                assert!(
                    cursor.is_multiple_of(1 << bits),
                    "{cursor:#x} aligned to 2^{bits}"
                );
                cursor += 1 << bits;
                chunks += 1;
            }
            assert_eq!(cursor, to, "the chunks reach the target");
            assert!(chunks <= 2 * 30, "a few chunks: {chunks}");
        }
    }
}
