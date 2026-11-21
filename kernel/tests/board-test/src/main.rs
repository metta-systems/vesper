#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! board-test: the drivers being brought up on new boards, with their lines on
//! the UART.
//!
//! Kickstart loads the PL011 driver, the log server (holding the PL011's
//! serial channel), the mailbox, the SD card controller, the PCIe root
//! complex, what lies behind it (the Raspberry Pi 4's VL805 xHCI controller,
//! the Raspberry Pi 5's RP1 through `pci-probe`), the USB core, HID driver
//! and `input-trace` on the xHCI controller (the Raspberry Pi 4's VIA hub and
//! a keyboard or mouse behind it, whose events `input-trace` logs) and the
//! root dispatcher. The
//! SD controller logs its bring-up (base clock, ADMA2 or the data port, the
//! card's size) and what block 0 holds, the root complex its link, functions,
//! BARs and MSI self-test, and the functions behind it theirs, all through the
//! log server, so a board shows them on its UART: the Raspberry Pi 4's GPIO 14
//! and 15, the Raspberry Pi 5's debug connector. Nothing writes to the card.
//! Built with `--cfg test_boot_panic` (`just extra_rustflags='--cfg
//! test_boot_panic' boot-board-test debug_kernel`), Kickstart on the boards
//! also loads log-test's probe, which panics on purpose and overflows a stack,
//! to show a component's last words reach the UART.
//! Kickstart prints on semihosting only once it drops to EL1, so this Thread
//! then only waits.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    kickstart::{image::BootObjects, kickstart_init_el2},
    // `libkicktest` supplies the panic handler.
    libkicktest as _,
    libobject::NotificationKey,
    libqemu::semihosting as semi,
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C03;

#[cfg_attr(
    not(feature = "qemu"),
    expect(
        unused_variables,
        reason = "the boot report prints over semihosting only"
    )
)]
pub fn run() -> ! {
    semi::println!("board-test: enabled MMU and dropped to EL1");
    let mut boot = BootObjects::bootstrap();
    // The builder's own address space, as in the other driver test kernels.
    let mut image = boot.builder_address_space(BUILDER_ARCHIVE_GUARD);

    let started = image.start_drivers(
        &[
            &components::UART_DRIVER,
            &components::LOG_SERVER_DRIVER,
            &components::MAILBOX_DRIVER,
            &components::SD_DRIVER,
            &components::PCIE_DRIVER,
            &components::XHCI_DRIVER,
            &components::USB_CORE_DRIVER,
            &components::USB_HID_DRIVER,
            &components::INPUT_TRACE_DRIVER,
            &components::PCI_PROBE_DRIVER,
            // On the boards only, by request (`--cfg test_boot_panic`): its
            // panic on purpose would end a QEMU run.
            #[cfg(all(not(feature = "qemu"), test_boot_panic))]
            &components::LOG_PROBE_DRIVER,
            &components::DISPATCHER_DRIVER,
        ],
        |loaded, _builder| {
            semi::println!(
                "board-test: {} on {}: {} pages, {} interrupts, {} imports",
                loaded.driver.component.name,
                loaded.node,
                loaded.init.pages().len(),
                loaded.init.interrupts().len(),
                loaded.init.imports().len()
            );
        },
    );
    semi::println!("board-test: {started} components started");

    // The drivers log on their own; wait for good.
    let park = image.notification();
    loop {
        let _woken = NotificationKey::from_key(park).await_bits(NotificationKey::WAIT_INFINITE);
    }
}
