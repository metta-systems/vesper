#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! sd-test's program. It calls the SDHOST component through the `block_v1`
//! import Kickstart wired along the controller's compatible string, and
//! drives the request queue on the io-channel: it reads marked blocks of the
//! card image `just test-sd` attaches, writes a block and reads it back, and
//! checks the requests the driver must refuse. It then reports on the key
//! sd-test installed (`driver_protocol::REPORT_SLOT`).

use {
    block_client::block_v1::{Bindings, Client},
    core::{ptr, slice},
    driver_protocol::report_key,
    libblock_channel::{BLOCK_BYTES, BlockChannel, HEADER_BYTES, Header, Op, Status, request},
    libdevice_driver::{driver_init, driver_threads},
    libio_channel::{IoRec, NoEvent, Packet},
    libobject::{EventCountKey, NotificationKey, RawKey},
    libuser::semihosting as semi,
};

libuser::entry!(main);

driver_threads!(background);

/// Blocks of the card image `just test-sd` attaches: 4 MiB.
const IMAGE_BLOCKS: u64 = 8192;

/// A block `just test-sd` writes the marker `vesper sd block <n>` at the start of.
const MARKED: [u64; 4] = [0, 5, 6, IMAGE_BLOCKS - 1];

/// The block the program writes and reads back.
const SCRATCH_BLOCK: u64 = 100;

/// The program's second Thread: it reports in on the component's own
/// `EventCount`, which the first Thread waits for before it starts calling.
fn background(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .advance(1)
        .unwrap_or_else(|error| panic!("background advance failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}

/// The channel's data pages, as the program lays them out: request headers in
/// the first page, block buffers after it.
struct Area {
    base: u64,
    len: u64,
}

impl Area {
    /// Header `index`, zeroed and filled in for `op` from `block`.
    fn header(&self, index: u64, op: u32, block: u64) -> u64 {
        let address = self.base + index * HEADER_BYTES;
        assert!(address + HEADER_BYTES <= self.base + 4096);
        // SAFETY: the header lies in the first data page, which the driver
        // does not hold until the request is submitted.
        unsafe {
            ptr::write_volatile(
                address as *mut Header,
                Header {
                    op,
                    status: Status::Pending as u32,
                    block,
                },
            );
        }
        address
    }

    /// The status the driver wrote in the header at `address`.
    fn status(address: u64) -> Option<Status> {
        // SAFETY: a header this program laid out, back from the driver.
        Status::from_raw(unsafe { Header::read(address) }.status)
    }

    /// Buffer `index` of `blocks` blocks, block-sized slots after the first page.
    fn buffer(&self, index: u64, blocks: u64) -> IoRec {
        let rec = IoRec {
            base: self.base + 4096 + index * BLOCK_BYTES,
            len: blocks * BLOCK_BYTES,
        };
        assert!(rec.base + rec.len <= self.base + self.len);
        rec
    }

    /// The bytes of `rec`.
    fn bytes(rec: &IoRec) -> &'static mut [u8] {
        // SAFETY: `buffer` keeps every record inside the data pages, which are
        // mapped read/write for good; the program only looks at a buffer
        // while the driver does not hold it.
        unsafe {
            slice::from_raw_parts_mut(
                rec.base as *mut u8,
                usize::try_from(rec.len).unwrap_or_default(),
            )
        }
    }
}

/// The marker `just test-sd` wrote at the start of `block`.
fn marker(block: u64, text: &mut [u8; 32]) -> &[u8] {
    use core::fmt::Write;
    struct Cursor<'text>(&'text mut [u8], usize);
    impl Write for Cursor<'_> {
        fn write_str(&mut self, part: &str) -> core::fmt::Result {
            let end = self.1 + part.len();
            self.0
                .get_mut(self.1..end)
                .ok_or(core::fmt::Error)?
                .copy_from_slice(part.as_bytes());
            self.1 = end;
            Ok(())
        }
    }
    let mut cursor = Cursor(text, 0);
    write!(cursor, "vesper sd block {block}").expect("the marker fits");
    let length = cursor.1;
    &text[..length]
}

/// Submit `requests`, ring the driver once, and check each comes back in
/// order and unchanged.
fn run(client: &Client, channel: &BlockChannel, completed: u64, requests: &[Packet]) {
    let (mut submit, mut completions) = channel.requests.client(
        NoEvent,
        EventCountKey::from_key(RawKey::from_wire(completed)),
    );
    for packet in requests {
        submit.put(packet).expect("the FIFO has room");
    }
    client.submitted().expect("submitted");
    for packet in requests {
        let back = completions
            .wait()
            .unwrap_or_else(|_| panic!("waiting for a completion failed"));
        assert_eq!(&back, packet, "requests come back in order, unchanged");
    }
}

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    EventCountKey::from_key(RawKey::from_wire(init.event_count))
        .await_ge(1, EventCountKey::WAIT_INFINITE)
        .unwrap_or_else(|error| {
            panic!(
                "waiting for the background Thread failed: {:?}",
                error.code()
            )
        });
    let client = Client::new(Bindings::from_imports(init.imports()));
    let blocks = client.blocks().expect("the card is up");
    assert_eq!(blocks, IMAGE_BLOCKS, "the size of the card image");

    let channel_area = client
        .channel()
        .expect("the client shares an io-channel with the driver");
    assert_ne!(channel_area.event, 0, "the channel has an EventCount");
    // SAFETY: Kickstart mapped the channel's control page read/write.
    let channel = unsafe { BlockChannel::at(channel_area.control) };
    let completed = channel_area.event;
    let area = Area {
        base: channel_area.data.base,
        len: channel_area.data.len,
    };
    let read = Op::Read as u32;
    let write = Op::Write as u32;

    // Every marked block: the first alone, the next two as two buffers of one
    // request, the last one alone.
    let first = area.buffer(0, 1);
    let pair = [area.buffer(1, 1), area.buffer(2, 1)];
    let last = area.buffer(3, 1);
    let headers = [
        area.header(0, read, MARKED[0]),
        area.header(1, read, MARKED[1]),
        area.header(2, read, MARKED[3]),
    ];
    run(
        &client,
        channel,
        completed,
        &[
            request(1, headers[0], &[first]).unwrap(),
            request(2, headers[1], &pair).unwrap(),
            request(3, headers[2], &[last]).unwrap(),
        ],
    );
    for header in headers {
        assert_eq!(Area::status(header), Some(Status::Done));
    }
    for (rec, block) in [
        (first, MARKED[0]),
        (pair[0], MARKED[1]),
        (pair[1], MARKED[2]),
        (last, MARKED[3]),
    ] {
        let mut text = [0; 32];
        let expected = marker(block, &mut text);
        assert_eq!(
            &Area::bytes(&rec)[..expected.len()],
            expected,
            "block {block}"
        );
    }
    semi::println!("sd-test: read the marked blocks");

    // Write a pattern, read it back into another buffer.
    let outgoing = area.buffer(4, 1);
    let incoming = area.buffer(5, 1);
    for (index, byte) in Area::bytes(&outgoing).iter_mut().enumerate() {
        *byte = index.to_le_bytes()[0] ^ 0x5a;
    }
    Area::bytes(&incoming).fill(0);
    let headers = [
        area.header(3, write, SCRATCH_BLOCK),
        area.header(4, read, SCRATCH_BLOCK),
    ];
    run(
        &client,
        channel,
        completed,
        &[
            request(4, headers[0], &[outgoing]).unwrap(),
            request(5, headers[1], &[incoming]).unwrap(),
        ],
    );
    for header in headers {
        assert_eq!(Area::status(header), Some(Status::Done));
    }
    assert_eq!(
        Area::bytes(&incoming),
        Area::bytes(&outgoing),
        "the block written reads back"
    );
    semi::println!("sd-test: wrote block {SCRATCH_BLOCK} and read it back");

    // The requests the driver refuses.
    let past_end = area.header(5, read, IMAGE_BLOCKS - 1);
    let unknown_op = area.header(6, 9, 0);
    let short_buffer = area.header(7, read, 0);
    let outside = area.header(8, read, 0);
    let ragged = IoRec {
        len: 100,
        ..area.buffer(6, 1)
    };
    let beyond = IoRec {
        base: area.base + area.len,
        len: BLOCK_BYTES,
    };
    run(
        &client,
        channel,
        completed,
        &[
            request(6, past_end, &[area.buffer(6, 2)]).unwrap(),
            request(7, unknown_op, &[area.buffer(6, 1)]).unwrap(),
            request(8, short_buffer, &[ragged]).unwrap(),
            request(9, outside, &[beyond]).unwrap(),
        ],
    );
    assert_eq!(Area::status(past_end), Some(Status::OutOfRange));
    assert_eq!(Area::status(unknown_op), Some(Status::Invalid));
    assert_eq!(Area::status(short_buffer), Some(Status::Invalid));
    assert_eq!(
        Area::status(outside),
        Some(Status::Pending),
        "a buffer outside the data area leaves the request untouched"
    );
    semi::println!("sd-test: the driver refused the malformed requests");

    NotificationKey::from_key(report_key(init.table))
        .signal(0)
        .unwrap_or_else(|error| panic!("report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}
