//! Host tests of the framebuffer property buffers.

#[cfg(test)]
mod tests {
    use {
        fb_impl::{
            ATTRIBUTE_WORDS, Allocation, Attributes, CONFIGURE_WORDS, attributes_request,
            configure_request, covered_by, physical_address, read_allocation, read_attributes,
        },
        mailbox_impl::{check_request, tag},
    };

    #[test]
    fn both_requests_pass_the_mailbox_whitelist() {
        assert_eq!(check_request(&configure_request(640, 480, 32)), Ok(()));
        assert_eq!(check_request(&attributes_request()), Ok(()));
    }

    #[test]
    fn the_configuring_request_lays_out_its_tags() {
        let request = configure_request(640, 480, 32);
        assert_eq!(request[0], 88);
        assert_eq!(request[1], 0);
        assert_eq!(request[2..7], [tag::SET_PHYSICAL_SIZE, 8, 0, 640, 480]);
        assert_eq!(request[7..12], [tag::SET_VIRTUAL_SIZE, 8, 0, 640, 480]);
        assert_eq!(request[12..16], [tag::SET_DEPTH, 4, 0, 32]);
        assert_eq!(request[16..21], [tag::ALLOCATE_BUFFER, 8, 0, 16, 0]);
        assert_eq!(request[21], 0, "the end tag");
        assert_eq!(request.len(), CONFIGURE_WORDS);
    }

    #[test]
    fn the_attributes_request_lays_out_its_tags() {
        let request = attributes_request();
        assert_eq!(request[0], 64);
        assert_eq!(request[2..6], [tag::GET_PIXEL_ORDER, 4, 0, 0]);
        assert_eq!(request[6..10], [tag::SET_ALPHA_MODE, 4, 0, 2]);
        assert_eq!(request[10..14], [tag::GET_PITCH, 4, 0, 0]);
        assert_eq!(request[14], 0, "the end tag");
        assert_eq!(request.len(), ATTRIBUTE_WORDS);
    }

    #[test]
    fn answers_are_read_from_the_tags_that_were_answered() {
        let mut answer = configure_request(640, 480, 32);
        answer[18] = 0x8000_0008;
        answer[19] = 0xc3e0_0000;
        answer[20] = 0x12_c000;
        assert_eq!(
            read_allocation(&answer),
            Ok(Allocation {
                bus_address: 0xc3e0_0000,
                bytes: 0x12_c000
            })
        );
        // An allocation the firmware ignored is reported with its code word.
        answer[18] = 8;
        assert_eq!(read_allocation(&answer), Err(8));

        let mut answer = attributes_request();
        answer[4] = 0x8000_0004;
        answer[5] = 1;
        answer[12] = 0x8000_0004;
        answer[13] = 2560;
        assert_eq!(
            read_attributes(&answer),
            Ok(Attributes {
                pixel_order: 1,
                pitch: 2560
            })
        );
        answer[12] = 4;
        assert_eq!(read_attributes(&answer), Err(4));
        answer[12] = 0x8000_0004;
        answer[4] = 0;
        assert_eq!(read_attributes(&answer), Err(0));
    }

    #[test]
    fn the_framebuffer_must_lie_in_the_video_memory_blocks() {
        // Two adjacent blocks, as on the Raspberry Pi 3: 32 MiB and 16 MiB.
        let blocks = [(0x3c00_0000, 0x3e00_0000), (0x3e00_0000, 0x3f00_0000)];
        assert!(covered_by(&blocks, 0x3c10_0000, 0x12_c000));
        // Across the boundary between the blocks.
        assert!(covered_by(&blocks, 0x3dff_f000, 0x2000));
        assert!(covered_by(&blocks, 0x3c00_0000, 0x0300_0000));
        // Below, beyond, and partly outside.
        assert!(!covered_by(&blocks, 0x3bff_f000, 0x2000));
        assert!(!covered_by(&blocks, 0x3eff_f000, 0x2000));
        assert!(!covered_by(&blocks, 0x3f00_0000, 0x1000));
        assert!(!covered_by(&blocks, u64::MAX - 8, 0x100));
        assert!(!covered_by(&[], 0x3c10_0000, 0x1000));
        assert!(
            covered_by(&blocks, 0x3c10_0000, 0),
            "an empty range is covered"
        );
    }
    #[test]
    fn bus_addresses_become_physical_addresses() {
        assert_eq!(physical_address(0xc3e0_0000, 0xc000_0000), 0x03e0_0000);
        // Without a usable offset the bus aliases are masked off.
        assert_eq!(physical_address(0xc3e0_0000, u64::MAX), 0x03e0_0000);
        assert_eq!(physical_address(0x4000_1000, u64::MAX), 0x0000_1000);
        // An address below the offset falls back to the same masking.
        assert_eq!(physical_address(0x8000_1000, 0xc000_0000), 0x0000_1000);
    }

    use fb_impl::{
        FramePlan, FrameRun, LARGE_FRAME_BITS, MAX_RETYPE_COUNT, PAGE_BYTES, PAGE_FRAME_BITS,
        page_range, skip_chunks,
    };

    const LARGE: u64 = 1 << LARGE_FRAME_BITS;

    fn plan(blocks: &[(u64, u64)], start: u64, end: u64) -> Vec<FrameRun> {
        FramePlan::new(blocks, start, end).collect()
    }

    /// Whether `runs` cover `[start, end)` exactly, in order, each run inside
    /// one of `blocks` and on the alignment of its Frames.
    fn assert_covers(runs: &[FrameRun], blocks: &[(u64, u64)], start: u64, end: u64) {
        let mut cursor = start;
        for run in runs {
            assert_eq!(run.address, cursor, "runs follow each other: {runs:?}");
            assert!(run.count >= 1 && run.count <= MAX_RETYPE_COUNT);
            assert!(run.address.is_multiple_of(1 << run.size_bits), "aligned");
            assert!(
                blocks
                    .iter()
                    .any(|&(base, block_end)| base <= run.address && run.end() <= block_end),
                "a run stays inside one block: {run:?}"
            );
            cursor = run.end();
        }
        assert_eq!(cursor, end, "the runs reach the end: {runs:?}");
    }

    #[test]
    fn the_framebuffer_occupies_whole_pages() {
        assert_eq!(
            page_range(0x3c10_0000, 0x12_c000),
            Some((0x3c10_0000, 0x3c22_c000))
        );
        // A buffer that starts and ends inside pages takes those pages whole.
        assert_eq!(
            page_range(0x3c10_0010, 100),
            Some((0x3c10_0000, 0x3c10_1000))
        );
        assert_eq!(
            page_range(0x3c10_0ff0, 0x20),
            Some((0x3c10_0000, 0x3c10_2000))
        );
        assert_eq!(page_range(0x1000, 0), None);
        assert_eq!(page_range(u64::MAX - 8, 100), None);
        assert_eq!(page_range(u64::MAX - 0x10, 0x8), None, "rounding overflows");
    }

    #[test]
    fn a_framebuffer_off_the_large_alignment_is_4k_frames() {
        // What QEMU's firmware allocates: 1 MiB into the video memory.
        let blocks = [(0x3c00_0000, 0x4000_0000)];
        let runs = plan(&blocks, 0x3c10_0000, 0x3c22_c000);
        assert_covers(&runs, &blocks, 0x3c10_0000, 0x3c22_c000);
        assert_eq!(
            runs,
            [
                FrameRun {
                    address: 0x3c10_0000,
                    size_bits: PAGE_FRAME_BITS,
                    count: 256
                },
                FrameRun {
                    address: 0x3c20_0000,
                    size_bits: PAGE_FRAME_BITS,
                    count: 44
                },
            ],
            "300 pages, 256 per Retype"
        );
        assert_eq!(
            runs.iter().map(|run| u64::from(run.count)).sum::<u64>(),
            300
        );
    }

    #[test]
    fn large_frames_where_the_range_allows() {
        let blocks = [(0, 0x1000_0000)];
        // A 4 KiB head to the alignment, three 2 MiB Frames, a 4 KiB tail.
        let runs = plan(&blocks, 0x1f_f000, 0x80_1000);
        assert_covers(&runs, &blocks, 0x1f_f000, 0x80_1000);
        assert_eq!(
            runs,
            [
                FrameRun {
                    address: 0x1f_f000,
                    size_bits: PAGE_FRAME_BITS,
                    count: 1
                },
                FrameRun {
                    address: 0x20_0000,
                    size_bits: LARGE_FRAME_BITS,
                    count: 3
                },
                FrameRun {
                    address: 0x80_0000,
                    size_bits: PAGE_FRAME_BITS,
                    count: 1
                },
            ]
        );
        // Less than a large Frame fully inside the range is all 4 KiB.
        let runs = plan(&blocks, 0x20_0000, 0x20_0000 + LARGE - PAGE_BYTES);
        assert!(runs.iter().all(|run| run.size_bits == PAGE_FRAME_BITS));
        assert_covers(&runs, &blocks, 0x20_0000, 0x20_0000 + LARGE - PAGE_BYTES);
        // Exactly one large Frame is one.
        assert_eq!(
            plan(&blocks, 0x20_0000, 0x40_0000),
            [FrameRun {
                address: 0x20_0000,
                size_bits: LARGE_FRAME_BITS,
                count: 1
            }]
        );
    }

    #[test]
    fn runs_stay_inside_their_block_and_are_capped() {
        // A block too small for a large Frame forces 4 KiB Frames, and a run
        // never crosses into the next block.
        let blocks = [(0, 0x10_0000), (0x10_0000, 0x40_0000)];
        let runs = plan(&blocks, 0xf_f000, 0x30_0000);
        assert_covers(&runs, &blocks, 0xf_f000, 0x30_0000);
        assert_eq!(runs[0].address, 0xf_f000);
        assert_eq!(runs[0].count, 1, "up to the end of the first block");
        assert_eq!(runs[1].address, 0x10_0000);
        // 256 pages fill 1 MiB: the cap, then the next run follows at once.
        assert!(runs.iter().all(|run| run.count <= MAX_RETYPE_COUNT));
        // A large Frame in the second block, from the first aligned address
        // that has room for it before the end of the range.
        let runs = plan(&blocks, 0x10_0000, 0x40_0000);
        assert_covers(&runs, &blocks, 0x10_0000, 0x40_0000);
        assert!(runs.iter().any(|run| run.size_bits == LARGE_FRAME_BITS));
        // A large Frame never straddles two blocks, even when aligned.
        let split = [(0x20_0000, 0x30_0000), (0x30_0000, 0x40_0000)];
        let runs = plan(&split, 0x20_0000, 0x40_0000);
        assert_covers(&runs, &split, 0x20_0000, 0x40_0000);
        assert!(runs.iter().all(|run| run.size_bits == PAGE_FRAME_BITS));
    }

    #[test]
    fn plans_end_at_memory_no_block_holds() {
        let blocks = [(0x1000, 0x5000)];
        let runs = plan(&blocks, 0x3000, 0x9000);
        assert_eq!(runs.iter().map(|run| run.bytes()).sum::<u64>(), 0x2000);
        assert!(plan(&blocks, 0x6000, 0x8000).is_empty());
        assert!(plan(&blocks, 0x3000, 0x3000).is_empty());
    }

    #[test]
    fn any_covered_range_is_planned_exactly() {
        let blocks = [
            (0x0, 0x40_0000),
            (0x40_0000, 0x80_0000),
            (0x80_0000, 0x100_0000),
        ];
        for start in (0..0x100_0000).step_by(0x3_f000 + 0x1000) {
            for pages in [1_u64, 2, 255, 256, 257, 511, 512, 513, 1000, 3000] {
                let end = (start + pages * PAGE_BYTES).min(0x100_0000);
                if end > start {
                    let runs = plan(&blocks, start, end);
                    assert_covers(&runs, &blocks, start, end);
                }
            }
        }
    }

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
