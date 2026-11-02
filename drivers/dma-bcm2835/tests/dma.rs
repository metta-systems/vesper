//! Host tests of the DMA controller's control blocks, channel allocation and
//! chain bookkeeping.

#[cfg(test)]
mod tests {
    use {
        arbitrary_int::{u4, u5},
        dma_bcm2835::{
            CHAIN_BYTES, CHANNELS, CONTROL_BLOCK_BYTES, ChainError, Channels, ControlBlock,
            ControlStatus, DREQ_MEMORY, DREQ_PWM, DebugStatus, MAX_CHAIN_BLOCKS, MAX_LITE_LENGTH,
            NODE_INTERRUPTS, PWM_FIFO_BUS, Position, TransferInformation, acknowledging,
            channels_of_interrupt, clear_debug_errors, clearing_debug_errors, copy_chain,
            error_bits, finished_blocks, interrupt_index, is_known_request, peripheral_address,
            position, ring_chain, slot_address, starting,
        },
    };

    /// The device trees' `brcm,dma-channel-mask` on the Raspberry Pi 3:
    /// channels 0, 2, 4, 5 and 8 to 14.
    const RPI3_MASK: u32 = 0x7f35;

    /// Where the test puts a channel's slots: a 32-byte aligned bus address in
    /// RAM's `0xC000_0000` alias.
    const SLOTS: u32 = 0xc010_0100;

    /// The design's ring: 4 periods of 512 stereo frames, two 32-bit FIFO
    /// words each (`doc/agentic/audio-design.md`, Scheduling).
    const PERIOD_BYTES: u32 = 512 * 2 * 4;

    #[test]
    fn control_blocks_are_eight_words_aligned_to_32_bytes() {
        assert_eq!(size_of::<ControlBlock>(), 32);
        assert_eq!(align_of::<ControlBlock>(), 32);
        assert_eq!(CONTROL_BLOCK_BYTES, 32);
        let blocks = [ControlBlock::default(); 3];
        let first = core::ptr::from_ref(&blocks[0]) as usize;
        assert_eq!(first % 32, 0);
        assert_eq!(core::ptr::from_ref(&blocks[1]) as usize - first, 32);
        // Every channel's slots fit one page, each slot aligned.
        const { assert!(CHANNELS * CHAIN_BYTES <= 4096) };
        assert_eq!(CHAIN_BYTES % 32, 0);
        assert_eq!(slot_address(SLOTS, 3), SLOTS + 96);
    }

    /// §4.2.1.2: `INTEN` 0, `TDMODE` 1, `WAIT_RESP` 3, `DEST_INC` 4,
    /// `DEST_WIDTH` 5, `DEST_DREQ` 6, `DEST_IGNORE` 7, `SRC_INC` 8, `SRC_WIDTH`
    /// 9, `SRC_DREQ` 10, `SRC_IGNORE` 11, `BURST_LENGTH` 15:12, `PERMAP` 20:16,
    /// `WAITS` 25:21, `NO_WIDE_BURSTS` 26.
    #[test]
    fn transfer_information_fields_sit_at_the_datasheet_bits() {
        let field = |information: TransferInformation| information.raw_value();
        let zero = TransferInformation::ZERO;
        assert_eq!(field(zero.with_interrupt_enable(true)), 1 << 0);
        assert_eq!(field(zero.with_two_dimensional(true)), 1 << 1);
        assert_eq!(field(zero.with_wait_for_write_response(true)), 1 << 3);
        assert_eq!(field(zero.with_destination_increment(true)), 1 << 4);
        assert_eq!(field(zero.with_destination_wide(true)), 1 << 5);
        assert_eq!(field(zero.with_destination_request(true)), 1 << 6);
        assert_eq!(field(zero.with_destination_ignore(true)), 1 << 7);
        assert_eq!(field(zero.with_source_increment(true)), 1 << 8);
        assert_eq!(field(zero.with_source_wide(true)), 1 << 9);
        assert_eq!(field(zero.with_source_request(true)), 1 << 10);
        assert_eq!(field(zero.with_source_ignore(true)), 1 << 11);
        assert_eq!(field(zero.with_burst_length(u4::new(0xf))), 0xf << 12);
        assert_eq!(field(zero.with_peripheral(u5::new(5))), 5 << 16);
        assert_eq!(field(zero.with_waits(u5::new(0x1f))), 0x1f << 21);
        assert_eq!(field(zero.with_no_wide_bursts(true)), 1 << 26);
    }

    /// §4.2.1.2: `ACTIVE` 0, `END` 1, `INT` 2, `ERROR` 8, `PRIORITY` 19:16,
    /// `PANIC_PRIORITY` 23:20, `WAIT_FOR_OUTSTANDING_WRITES` 28, `ABORT` 30,
    /// `RESET` 31; `DEBUG`: the three error flags in 2:0, `LITE` 28.
    #[test]
    fn control_status_and_debug_fields_sit_at_the_datasheet_bits() {
        assert_eq!(
            starting().raw_value(),
            1 | (1 << 16) | (15 << 20) | (1 << 28),
            "active, priority 1, panic priority 15, waiting for writes"
        );
        let finished = acknowledging(false);
        assert_eq!(
            finished.raw_value(),
            (1 << 1) | (1 << 2),
            "END and INT only"
        );
        assert!(!finished.active(), "a finished channel stays idle");
        let running = acknowledging(true);
        assert_eq!(
            running.raw_value(),
            starting().raw_value() | (1 << 1) | (1 << 2),
            "a running channel keeps ACTIVE and its priorities"
        );
        assert_eq!(ControlStatus::ZERO.with_reset(true).raw_value(), 1 << 31);
        assert_eq!(ControlStatus::ZERO.with_abort(true).raw_value(), 1 << 30);
        assert!(ControlStatus::new_with_raw_value(1 << 8).error());
        assert_eq!(clear_debug_errors().raw_value(), 0b111);
        // Only the flags that are set are written back (QEMU stores DEBUG
        // writes verbatim instead of clearing).
        assert_eq!(
            clearing_debug_errors(DebugStatus::new_with_raw_value(1 << 28)),
            None,
            "nothing set, nothing written"
        );
        assert_eq!(
            clearing_debug_errors(DebugStatus::new_with_raw_value((1 << 28) | 0b010))
                .map(|clearing| clearing.raw_value()),
            Some(0b010),
            "the FIFO error alone, LITE untouched"
        );
        assert!(DebugStatus::new_with_raw_value(1 << 28).lite());
        assert_eq!(
            error_bits(
                ControlStatus::new_with_raw_value(1 << 8),
                DebugStatus::new_with_raw_value((1 << 28) | 0b101)
            ),
            0b1101,
            "the DEBUG flags, then CS.ERROR; LITE is not an error"
        );
    }

    #[test]
    fn a_copy_chain_links_its_blocks_and_ends_in_zero() {
        let chain = copy_chain(SLOTS, 0xc000_1000, 0xc000_1800, 512, 4, true).unwrap();
        assert!(!chain.is_cyclic());
        assert_eq!(chain.blocks().len(), 4);
        assert_eq!(chain.bytes(), 4 * 32);
        for (index, block) in chain.blocks().iter().enumerate() {
            let offset = u32::try_from(index).unwrap() * 512;
            assert_eq!(block.source, 0xc000_1000 + offset);
            assert_eq!(block.destination, 0xc000_1800 + offset);
            assert_eq!(block.length, 512);
            assert_eq!(block.stride, 0);
            let information = block.information();
            assert!(information.interrupt_enable(), "every block interrupts");
            assert!(information.source_increment() && information.destination_increment());
            assert!(!information.destination_request() && !information.source_request());
            assert_eq!(information.peripheral(), u5::new(0), "DREQ 0: unpaced");
            assert!(
                !information.destination_wide() && !information.source_wide(),
                "32-bit words (QEMU refuses 128-bit writes)"
            );
        }
        let next: Vec<_> = chain.blocks().iter().map(|block| block.next).collect();
        assert_eq!(next, [SLOTS + 32, SLOTS + 64, SLOTS + 96, 0]);
    }

    #[test]
    fn a_ring_feeds_the_pwm_fifo_paced_by_its_dreq_and_wraps() {
        let fifo = peripheral_address(DREQ_PWM).unwrap();
        assert_eq!(fifo, PWM_FIFO_BUS);
        assert_eq!(PWM_FIFO_BUS, 0x7e20_c000 + 0x18, "FIF1 of the PWM block");
        let ring = 0xc020_0000;
        let chain = ring_chain(SLOTS, ring, PERIOD_BYTES, 4, DREQ_PWM, fifo, true).unwrap();
        assert!(chain.is_cyclic());
        for (index, block) in chain.blocks().iter().enumerate() {
            assert_eq!(
                block.source,
                ring + u32::try_from(index).unwrap() * PERIOD_BYTES
            );
            assert_eq!(block.destination, fifo, "the FIFO's address never moves");
            assert_eq!(block.length, PERIOD_BYTES);
            let information = block.information();
            assert!(information.interrupt_enable(), "one interrupt per period");
            assert!(information.source_increment());
            assert!(!information.destination_increment());
            assert!(information.destination_request(), "writes paced by DREQ");
            assert!(information.wait_for_write_response());
            assert!(information.no_wide_bursts());
            assert_eq!(
                information.peripheral(),
                u5::new(5),
                "PERMAP is the PWM's DREQ"
            );
        }
        let next: Vec<_> = chain.blocks().iter().map(|block| block.next).collect();
        assert_eq!(
            next,
            [SLOTS + 32, SLOTS + 64, SLOTS + 96, SLOTS],
            "the last period leads back to the first"
        );
    }

    #[test]
    fn chains_refuse_what_a_channel_cannot_run() {
        let copy =
            |bytes, blocks, lite| copy_chain(SLOTS, 0xc000_0000, 0xc001_0000, bytes, blocks, lite);
        assert_eq!(copy(512, 0, false), Err(ChainError::BlockCount));
        assert_eq!(
            copy(512, u32::try_from(MAX_CHAIN_BLOCKS).unwrap() + 1, false),
            Err(ChainError::BlockCount)
        );
        let longest = copy(512, u32::try_from(MAX_CHAIN_BLOCKS).unwrap(), false).unwrap();
        assert_eq!(longest.blocks().len(), MAX_CHAIN_BLOCKS);
        assert_eq!(copy(0, 1, false), Err(ChainError::Length));
        assert_eq!(copy(510, 1, false), Err(ChainError::Length), "whole words");
        // A Lite channel's length is 16 bits (§4.5); a full channel's 30.
        assert_eq!(MAX_LITE_LENGTH, 65_532);
        assert_eq!(
            copy(MAX_LITE_LENGTH, 1, true).unwrap().blocks()[0].length,
            MAX_LITE_LENGTH
        );
        assert_eq!(copy(MAX_LITE_LENGTH + 4, 1, true), Err(ChainError::Length));
        assert_eq!(
            copy(MAX_LITE_LENGTH + 4, 1, false).unwrap().blocks()[0].length,
            MAX_LITE_LENGTH + 4
        );
        assert_eq!(
            copy_chain(SLOTS, 0xc000_0002, 0xc001_0000, 512, 1, false),
            Err(ChainError::Address),
            "word-aligned source"
        );
        assert_eq!(
            copy_chain(SLOTS, 0xc000_0000, 0xffff_ff00, 512, 1, false),
            Err(ChainError::Address),
            "a destination that wraps the bus"
        );
        assert_eq!(
            ring_chain(SLOTS, 0xc000_0000, 512, 4, 32, PWM_FIFO_BUS, false),
            Err(ChainError::Request),
            "PERMAP is five bits"
        );
    }

    #[test]
    fn only_memory_and_the_pwm_have_a_request_line() {
        assert!(is_known_request(DREQ_MEMORY));
        assert!(is_known_request(DREQ_PWM));
        assert_eq!(peripheral_address(DREQ_MEMORY), None, "memory has no FIFO");
        for dreq in [1, 2, 3, 4, 6, 7, 13, 31, 32] {
            assert!(!is_known_request(dreq), "DREQ {dreq}");
        }
    }

    #[test]
    fn channels_come_from_the_mask_lowest_first() {
        let mut channels = Channels::new(RPI3_MASK);
        assert_eq!(channels.granted(), 0x7f35);
        let mut order = Vec::new();
        while let Some(channel) = channels.claim(|_| true) {
            order.push(channel);
        }
        assert_eq!(order, [0, 2, 4, 5, 8, 9, 10, 11, 12, 13, 14]);
        assert_eq!(
            channels.claim(|_| true),
            None,
            "every granted channel taken"
        );
        assert!(channels.release(4));
        assert!(!channels.release(4), "released once");
        assert!(!channels.release(1), "never granted");
        assert_eq!(
            channels.claim(|_| true),
            Some(4),
            "a released channel comes back"
        );
        // Channel 15 and anything above it is never granted.
        assert_eq!(Channels::new(u32::MAX).granted(), 0x7fff);
        // The filter skips channels the component cannot serve.
        let mut channels = Channels::new(RPI3_MASK);
        assert_eq!(channels.claim(|channel| channel >= 11), Some(11));
        assert!(channels.is_claimed(11));
        assert!(!channels.is_claimed(0));
    }

    #[test]
    fn interrupts_map_to_channels_after_kickstart_merges_the_shared_line() {
        // dma0 to dma10 have their own lines, dma11 to dma14 share the next,
        // dma-shared-all is the last of thirteen.
        assert_eq!(NODE_INTERRUPTS, 13);
        for channel in 0..=10 {
            assert_eq!(interrupt_index(channel), channel);
            assert_eq!(channels_of_interrupt(channel), 1 << channel);
        }
        for channel in 11..=14 {
            assert_eq!(interrupt_index(channel), 11);
        }
        assert_eq!(channels_of_interrupt(11), 0b0111_1000_0000_0000);
        assert_eq!(channels_of_interrupt(12), 0, "dma-shared-all is never used");
    }

    #[test]
    fn positions_come_from_the_control_block_address() {
        assert_eq!(position(SLOTS, 4, 0), Position::Ended);
        assert_eq!(position(SLOTS, 4, SLOTS), Position::On(0));
        assert_eq!(position(SLOTS, 4, SLOTS + 96), Position::On(3));
        assert_eq!(
            position(SLOTS, 4, SLOTS + 128),
            Position::Lost,
            "past the chain"
        );
        assert_eq!(position(SLOTS, 4, SLOTS + 8), Position::Lost, "mid-block");
        assert_eq!(position(SLOTS, 4, SLOTS - 32), Position::Lost);
    }

    #[test]
    fn finished_blocks_count_one_per_period_and_survive_late_service() {
        // A finite chain QEMU ran to the end inside the starting write.
        assert_eq!(finished_blocks(0, Position::Ended, 4, false), 4);
        assert_eq!(finished_blocks(1, Position::On(3), 4, false), 2);
        // A ring: one period per interrupt, and a late interrupt counts each
        // period it slept through, across the wrap.
        assert_eq!(finished_blocks(0, Position::On(1), 4, true), 1);
        assert_eq!(finished_blocks(3, Position::On(0), 4, true), 1);
        assert_eq!(finished_blocks(2, Position::On(1), 4, true), 3);
        assert_eq!(finished_blocks(2, Position::Lost, 4, true), 0);
    }
}
