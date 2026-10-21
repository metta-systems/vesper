//! Host tests of the block channel's layout and the driver's request checks.

#[cfg(test)]
mod tests {
    use {
        libio_channel::{DataArea, IoRec, NoEvent, Packet},
        vesper_block_channel::{
            BLOCK_BYTES, BlockChannel, HEADER_BYTES, Header, MAX_BUFFERS, Op, Refusal, Status,
            admit, request, shape,
        },
    };

    const AREA: DataArea = DataArea {
        base: 0x10_0000,
        len: 16 * 4096,
    };

    fn buffer(offset: u64, blocks: u64) -> IoRec {
        IoRec {
            base: AREA.base + offset,
            len: blocks * BLOCK_BYTES,
        }
    }

    #[test]
    fn the_header_is_sixteen_bytes() {
        assert_eq!(HEADER_BYTES, 16);
        assert_eq!(core::mem::offset_of!(Header, status), 4);
        assert_eq!(core::mem::offset_of!(Header, block), 8);
    }

    #[test]
    fn an_all_zero_page_is_an_idle_channel() {
        let page = vec![0_u64; 4096 / 8];
        // SAFETY: a zeroed, aligned page that outlives the channel's use.
        let channel = unsafe { BlockChannel::at(page.as_ptr() as u64) };
        assert!(channel.requests.submit.is_empty());
        assert!(channel.requests.complete.is_empty());
    }

    #[test]
    fn a_request_puts_its_header_first() {
        let packet = request(7, AREA.base, &[buffer(512, 2), buffer(4096, 1)]).unwrap();
        assert_eq!(packet.value(), 7);
        assert_eq!(packet.recs().len(), 3);
        assert_eq!(packet.recs()[0].len, HEADER_BYTES);
        assert_eq!(shape(&packet, &AREA), Ok((AREA.base, 3)));
    }

    #[test]
    fn a_request_needs_one_to_seven_buffers() {
        assert!(request(0, AREA.base, &[]).is_none());
        let buffers = [buffer(512, 1); MAX_BUFFERS + 1];
        assert!(request(0, AREA.base, &buffers).is_none());
        assert!(request(0, AREA.base, &buffers[..MAX_BUFFERS]).is_some());
    }

    #[test]
    fn a_record_outside_the_area_leaves_the_packet_untouched() {
        let outside = IoRec {
            base: AREA.base + AREA.len - 256,
            len: BLOCK_BYTES,
        };
        let packet = request(0, AREA.base, &[outside]).unwrap();
        assert_eq!(shape(&packet, &AREA), Err(Refusal::Untouched));
        let header_outside = request(0, AREA.base - 16, &[buffer(512, 1)]).unwrap();
        assert_eq!(shape(&header_outside, &AREA), Err(Refusal::Untouched));
    }

    #[test]
    fn a_misshapen_header_leaves_the_packet_untouched() {
        for header in [
            IoRec {
                base: AREA.base,
                len: 8,
            },
            IoRec {
                base: AREA.base + 4,
                len: HEADER_BYTES,
            },
        ] {
            let packet = Packet::new(0, &[header, buffer(512, 1)]).unwrap();
            assert_eq!(shape(&packet, &AREA), Err(Refusal::Untouched));
        }
    }

    #[test]
    fn misshapen_buffers_are_invalid() {
        let invalid = Err(Refusal::Status(Status::Invalid));
        // Only a header.
        let alone = Packet::new(
            0,
            &[IoRec {
                base: AREA.base,
                len: HEADER_BYTES,
            }],
        )
        .unwrap();
        assert_eq!(shape(&alone, &AREA), invalid);
        // A buffer that is not whole blocks, or not word aligned.
        for bad in [
            IoRec {
                base: AREA.base + 512,
                len: 100,
            },
            IoRec {
                base: AREA.base + 512,
                len: 0,
            },
            IoRec {
                base: AREA.base + 514,
                len: BLOCK_BYTES,
            },
        ] {
            let packet = request(0, AREA.base, &[bad]).unwrap();
            assert_eq!(shape(&packet, &AREA), invalid);
        }
    }

    #[test]
    fn admit_checks_the_op_and_the_range() {
        let read = |block| Header {
            op: Op::Read as u32,
            status: 0,
            block,
        };
        assert_eq!(admit(&read(0), 8, 8), Ok(Op::Read));
        assert_eq!(admit(&read(1), 8, 8), Err(Status::OutOfRange));
        assert_eq!(admit(&read(u64::MAX), 2, 8), Err(Status::OutOfRange));
        let write = Header {
            op: Op::Write as u32,
            ..read(3)
        };
        assert_eq!(admit(&write, 1, 8), Ok(Op::Write));
        let unknown = Header { op: 9, ..read(0) };
        assert_eq!(admit(&unknown, 1, 8), Err(Status::Invalid));
    }

    #[test]
    fn the_driver_reads_the_header_and_writes_the_status() {
        let mut words = [0_u64; 2];
        let address = words.as_mut_ptr() as u64;
        let header = Header {
            op: Op::Write as u32,
            status: 0,
            block: 42,
        };
        // SAFETY: `words` is 16 aligned bytes that outlive both calls.
        unsafe {
            core::ptr::write(address as *mut Header, header);
            assert_eq!(Header::read(address), header);
            Header::write_status(address, Status::Done);
            assert_eq!(Header::read(address).status, Status::Done as u32);
        }
        assert_eq!(words[1], 42, "the status write leaves the block alone");
    }

    #[test]
    fn statuses_round_trip() {
        for status in [
            Status::Pending,
            Status::Done,
            Status::OutOfRange,
            Status::Invalid,
            Status::DeviceFailed,
            Status::NoMedium,
        ] {
            assert_eq!(Status::from_raw(status as u32), Some(status));
        }
        assert_eq!(Status::from_raw(6), None);
    }

    #[test]
    fn requests_complete_back_unchanged() {
        let channel = BlockChannel::new();
        let (mut submit, mut completed) = channel.requests.client(NoEvent, NoEvent);
        let (mut requests, mut completions) = channel.requests.server(NoEvent, NoEvent);
        let packet = request(3, AREA.base, &[buffer(512, 1)]).unwrap();
        submit.put(&packet).unwrap();
        let served = requests.get().unwrap().unwrap();
        completions.put(&served).unwrap();
        assert_eq!(completed.get().unwrap(), Some(packet));
    }
}
