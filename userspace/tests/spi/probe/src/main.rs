#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! spi-test's probe driver, for the panel node on SPI0 (`ilitek,ili9341`, the
//! MSP2202 panel of `targets/spi-panel-test.dtb`). Kickstart loads it for a
//! node that sits on a bus, not in the memory map, and links its `spi_v1`
//! through its parent node, the SPI controller.
//!
//! It checks its node's path and reads its chip select (`reg`) and
//! `spi-max-frequency` from its device tree copy, then calls the spi
//! component: an empty write answers the rate without touching the block
//! (QEMU has no SPI0), a chip select the controller does not name and bytes
//! outside the probe's data area are refused. Then it reports on the key
//! spi-test installed. Any failure panics, which ends the QEMU run.

use {
    driver_protocol::report_key,
    libdevice_driver::driver_init,
    libdevicetree::{DeviceTree, INDEX_BUFFER_BYTES, PropReader, property},
    libobject::{NotificationKey, RawKey},
    libuser::semihosting as semi,
    spi_client::spi_v1::{Bindings, Client, Error, Exception, Mode},
};

libuser::entry!(main);

libdevice_driver::driver_compatible!("ilitek,ili9341");

/// The device tree index (`libdevicetree`).
static mut INDEX: [u8; INDEX_BUFFER_BYTES] = [0; INDEX_BUFFER_BYTES];

/// The panel's node in the test tree.
const PANEL_PATH: &str = "/soc/spi@7e204000/panel@0";

fn main(init_va: u64) -> ! {
    // SAFETY: Kickstart passes the init page's address in x0.
    let init = unsafe { driver_init(init_va) };
    assert_eq!(init.node_path(), Some(PANEL_PATH), "the probe's node path");
    assert!(
        init.regions().is_empty(),
        "a node on the bus has no registers"
    );

    // Its chip select and the fastest clock it takes, from its own node.
    let blob = init
        .device_tree()
        .expect("spi-test-probe: a device tree copy");
    // SAFETY: Kickstart mapped the copy read-only for good; the index buffer
    // is used here once.
    let tree = unsafe {
        DeviceTree::blob_from_raw(blob.as_ptr())
            .and_then(|raw| DeviceTree::new(raw, &mut *core::ptr::addr_of_mut!(INDEX)))
    }
    .unwrap_or_else(|error| panic!("spi-test-probe: the device tree copy: {error:?}"));
    let node = tree.node(PANEL_PATH).expect("the probe's node in its copy");
    let number = |name: &str| {
        property(&node, name)
            .and_then(|prop| prop.u32(0).ok())
            .unwrap_or_else(|| panic!("spi-test-probe: {name}"))
    };
    let chip_select = number("reg");
    let max_hz = number("spi-max-frequency");
    assert_eq!((chip_select, max_hz), (0, 32_000_000));

    let spi = Client::new(Bindings::from_imports(init.imports()));
    let area = spi.data_area().expect("spi_v1 has a data area");
    let empty = area.reference(0, 0).expect("an empty reference");
    let hz = spi
        .write(chip_select, max_hz, Mode::Mode0, empty)
        .unwrap_or_else(|error| panic!("spi-test-probe: an empty write: {error:?}"));
    assert!(hz != 0 && hz <= max_hz, "{hz} Hz for at most {max_hz} Hz");

    assert!(matches!(
        spi.write(3, max_hz, Mode::Mode0, empty),
        Err(Error::Exception(Exception::InvalidChipSelect {
            chip_select: 3
        }))
    ));
    let outside = libmeddle_runtime::IoRec {
        base: area.base + area.len,
        len: 16,
    };
    assert!(matches!(
        spi.write(chip_select, max_hz, Mode::Mode0, outside),
        Err(Error::Exception(Exception::InvalidData))
    ));
    semi::println!(
        "spi-test-probe: {PANEL_PATH} on chip select {chip_select}: {hz} Hz for at most {max_hz} Hz; refused chip select 3 and bytes outside its area"
    );

    NotificationKey::from_key(report_key(init.table))
        .signal(1)
        .unwrap_or_else(|error| panic!("spi-test-probe: report failed: {:?}", error.code()));
    let park = NotificationKey::from_key(RawKey::from_wire(init.notification));
    loop {
        let _bits = park.await_bits(NotificationKey::WAIT_INFINITE);
    }
}
