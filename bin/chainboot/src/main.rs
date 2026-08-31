// Based on miniload by @andre-richter
#![no_main]
#![no_std]
#![no_builtins]
#![feature(format_args_nl)]

core::arch::global_asm!(
    core::include_str!("boot.s"),
    CONST_BOOT_CORE_ID = const 0,
    CONST_CORE_ID_MASK = const 0b11,
);

use {
    aarch64_cpu::asm::barrier,
    core::{hash::Hasher, time::Duration},
    libconsole::console::console,
    liblog::{print, println},
    libplatform::raspberrypi::BcmHost,
    libqemu::semihosting as semi,
    seahash::SeaHasher,
};

/// Early init code.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order.
#[unsafe(no_mangle)]
unsafe extern "C" fn kernel_init(dtb: u32, max_kernel_size: u64) -> ! {
    #[cfg(feature = "jtag")]
    libmachine::debug::jtag::wait_debugger();

    semi::println!("Initializing drivers");
    // SAFETY: VERY SAFE
    if let Err(x) = unsafe { libplatform::drivers::init() } {
        panic!("Error initializing platform drivers: {}", x);
    }

    semi::println!("Initializing devices and IRQs");
    // Initialize all device drivers.
    // SAFETY: Relatively safe.
    unsafe {
        libplatform::drivers::driver_manager().init_drivers_and_irqs();
    }

    // Route print!/println! through the console logger; without this they
    // silently no-op on the default NopLogger.
    if libconsole::init_logger().is_err() {
        semi::println!("Logger already initialized");
    }

    // println! is usable from here on.

    // Transition from unsafe to safe.
    kernel_main(dtb, max_kernel_size)
}

// https://onlineasciitools.com/convert-text-to-ascii-art (FIGlet) with `cricket` font
const LOGO: &str = r"
       __          __       __                __
 .----|  |--.---.-|__.-----|  |--.-----.-----|  |_
 |  __|     |  _  |  |     |  _  |  _  |  _  |   _|
 |____|__|__|___._|__|__|__|_____|_____|_____|____|
";

fn read_u64(first: Option<u8>) -> u64 {
    let mut val: u64 = u64::from(first.or_else(|| Some(console().read_byte())).unwrap());
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
fn kernel_main(dtb: u32, max_kernel_size: u64) -> ! {
    #[cfg(test)]
    test_main();

    print!("{}", LOGO);
    println!("{:>51}\n", BcmHost::board_name());
    println!("Preserving DTB at {:8x}", dtb);
    println!("⏪ Requesting kernel image...");

    let kernel_addr: *mut u8 = BcmHost::kernel_load_address() as *mut u8;

    loop {
        console().flush();

        // Read the binary's size.
        // While waiting, periodically emit sync beacons so a late-attaching host can still catch us.
        console().clear_rx();

        let mut beacon_count: u64 = 0;
        let first = 'wait_for_first: loop {
            beacon_count = beacon_count.saturating_add(1);
            let uptime = libtime::time::time_manager().uptime();
            semi::println!(
                "⏪ Beacon #{beacon_count} at {}.{:03}s (sending ^C^C^C)",
                uptime.as_secs(),
                uptime.subsec_millis()
            );

            for _ in 0..3 {
                console().write_byte(3_u8);
            }
            console().flush();

            let start = libtime::time::time_manager().uptime();
            while libtime::time::time_manager().uptime() - start < Duration::from_secs(5) {
                if let Some(b) = console().read_byte_nonblocking() {
                    let now = libtime::time::time_manager().uptime();
                    semi::println!(
                        "⏪ Host byte 0x{b:02x} received at {}.{:03}s",
                        now.as_secs(),
                        now.subsec_millis()
                    );
                    break 'wait_for_first b;
                }
                libtime::time::time_manager().spin_for(Duration::from_millis(10));
            }

            let waited = libtime::time::time_manager().uptime() - start;
            semi::println!(
                "⏪ No host data after {}.{:03}s, retrying beacon",
                waited.as_secs(),
                waited.subsec_millis()
            );
        };

        let size: u64 = read_u64(Some(first));

        // Check the size to fit RAM
        if size > max_kernel_size {
            println!(
                "ERR ❌ Kernel image too big (over {} bytes)",
                max_kernel_size
            );
            continue;
        }

        print!("OK");
        semi::println!("Read kernel size {size} bytes");

        // We use seahash, it's simple and has no_std implementation.
        let mut hasher = SeaHasher::new();

        // Read the kernel byte by byte.
        for i in 0..size {
            let val = console().read_byte();
            // SAFETY: Writing things can be unsafe.
            unsafe {
                core::ptr::write_volatile(
                    kernel_addr.offset(i.cast_signed().try_into().unwrap()),
                    val,
                );
            }
            // SAFETY: Could be unsafe.
            let written = unsafe {
                core::ptr::read_volatile(kernel_addr.offset(i.cast_signed().try_into().unwrap()))
            };
            // Hash what is actually in memory, this helps catch writing over memory holes or device memory.
            hasher.write_u8(written);
        }

        // Read the binary's checksum.
        let checksum = read_u64(None);

        let valid = hasher.finish() == checksum;
        if !valid {
            println!("ERR ❌ Kernel image checksum mismatch");
            continue;
        }
        semi::println!("Read kernel checksum {checksum:016x}");

        print!("OK");
        break;
    }

    println!(
        "⏪ Loaded! Executing the payload now from {:p}\n",
        kernel_addr
    );
    console().flush();

    // Use black magic to create a function pointer.
    // SAFETY: We're getting to safety soon!
    let kernel: fn(u32) -> ! = unsafe { core::mem::transmute(kernel_addr) };

    // Force everything to complete before we jump.
    barrier::isb(barrier::SY);

    // Jump to loaded kernel!
    kernel(dtb)
}

#[panic_handler]
fn panicked(info: &core::panic::PanicInfo) -> ! {
    libmachine::panic::handler(info)
}
