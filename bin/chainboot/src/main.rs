// Based on miniload by @andre-richter
#![feature(format_args_nl)]
#![feature(custom_test_frameworks)]
#![test_runner(machine::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![no_main]
#![no_std]
#![no_builtins]

use {
    aarch64_cpu::asm::barrier,
    core::hash::Hasher,
    machine::{console::console, platform::raspberrypi::BcmHost, print, println},
    seahash::SeaHasher,
};

mod boot;

/// Early init code.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order.
unsafe fn kernel_init(max_kernel_size: u64) -> ! {
    #[cfg(feature = "jtag")]
    machine::debug::jtag::wait_debugger();

    if let Err(x) = machine::platform::drivers::init() {
        panic!("Error initializing platform drivers: {}", x);
    }

    // Initialize all device drivers.
    machine::drivers::driver_manager().init_drivers_and_irqs();

    // println! is usable from here on.

    // Transition from unsafe to safe.
    kernel_main(max_kernel_size)
}

// https://onlineasciitools.com/convert-text-to-ascii-art (FIGlet) with `cricket` font
const LOGO: &str = r#"
       __          __       __                __   
 .----|  |--.---.-|__.-----|  |--.-----.-----|  |_ 
 |  __|     |  _  |  |     |  _  |  _  |  _  |   _|
 |____|__|__|___._|__|__|__|_____|_____|_____|____|
"#;

fn read_u64() -> u64 {
    let mut val: u64 = u64::from(console().read_byte());
    val |= u64::from(console().read_byte()) << 8;
    val |= u64::from(console().read_byte()) << 16;
    val |= u64::from(console().read_byte()) << 24;
    val |= u64::from(console().read_byte()) << 32;
    val |= u64::from(console().read_byte()) << 40;
    val |= u64::from(console().read_byte()) << 48;
    val |= u64::from(console().read_byte()) << 56;
    val
}

/// The main function running after the early init.
#[inline(always)]
fn kernel_main(max_kernel_size: u64) -> ! {
    #[cfg(test)]
    test_main();

    print!("{}", LOGO);
    println!("{:>51}\n", BcmHost::board_name());
    println!("⏪ Requesting kernel image...");

    let kernel_addr: *mut u8 = BcmHost::kernel_load_address() as *mut u8;

    loop {
        console().flush();

        // Discard any spurious received characters before starting with the loader protocol.
        console().clear_rx();

        // Notify `chainofcommand` to send the binary.
        for _ in 0..3 {
            console().write_byte(3u8);
        }

        // Read the binary's size.
        let size = read_u64();

        // Check the size to fit RAM
        if size > max_kernel_size {
            println!(
                "ERR ❌ Kernel image too big (over {} bytes)",
                max_kernel_size
            );
            continue;
        }

        print!("OK");

        // We use seahash, simple and with no_std implementation.
        let mut hasher = SeaHasher::new();

        // Read the kernel byte by byte.
        for i in 0..size {
            let val = console().read_byte();
            unsafe {
                core::ptr::write_volatile(kernel_addr.offset(i as isize), val);
            }
            let written = unsafe { core::ptr::read_volatile(kernel_addr.offset(i as isize)) };
            hasher.write_u8(written);
        }

        // Read the binary's checksum.
        let checksum = read_u64();

        let valid = hasher.finish() == checksum;
        if !valid {
            println!("ERR ❌ Kernel image checksum mismatch");
            continue;
        }

        print!("OK");
        break;
    }

    println!(
        "⏪ Loaded! Executing the payload now from {:p}\n",
        kernel_addr
    );
    console().flush();

    // Use black magic to create a function pointer.
    let kernel: fn() -> ! = unsafe { core::mem::transmute(kernel_addr) };

    // Force everything to complete before we jump.
    barrier::isb(barrier::SY);

    // Jump to loaded kernel!
    kernel()
}

#[cfg(not(test))]
#[panic_handler]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    machine::panic::handler(info)
}

#[panic_handler]
#[cfg(test)]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    machine::panic::handler_for_tests(info)
}

#[cfg(test)]
mod chainboot_tests {
    #[test_case]
    fn nothing() {
        assert_eq!(2 + 2, 4);
    }
}
