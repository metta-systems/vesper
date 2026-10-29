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

mod platform;

use {
    aarch64_cpu::asm::barrier,
    core::{hash::Hasher, time::Duration},
    libchainboot_protocol::{
        BEACON, BOOT_BAUD_RATE, BOOT_RATE_WAIT, FAST_RATE_WAIT, Opening, RATE_ACCEPTED,
        RATE_REFUSED, RateRefusal, SWITCH_SETTLE, Wait, decode_rate_request_body, opening,
        validate_rate,
    },
    libconsole::console::console,
    liblog::{print, println},
    libqemu::semihosting as semi,
    platform::led::Blinker,
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

    semi::println!("Initializing the console");
    let bringup = match platform::init() {
        Ok(bringup) => bringup,
        Err(x) => panic!("Error initializing the console: {x}"),
    };

    // Route print!/println! through the console logger; without this they
    // silently no-op on the default NopLogger.
    // Chainboot runs with the MMU off, where an atomic swap never completes on
    // the board: the logger is set with plain loads and stores.
    // SAFETY: one core, nothing else sets a logger.
    if unsafe { libconsole::init_logger_racy() }.is_err() {
        semi::println!("Logger already initialized");
    }

    // println! is usable from here on.

    // Transition from unsafe to safe.
    kernel_main(dtb, max_kernel_size, bringup)
}

// https://onlineasciitools.com/convert-text-to-ascii-art (FIGlet) with `cricket` font
const LOGO: &str = r"
       __          __       __                __
 .----|  |--.---.-|__.-----|  |--.-----.-----|  |_
 |  __|     |  _  |  |     |  _  |  _  |  _  |   _|
 |____|__|__|___._|__|__|__|_____|_____|_____|____|
";

/// How often the LED flips while chainboot waits for the host.
const BLINK_PERIOD: Duration = Duration::from_millis(250);

/// How often the LED flips while the kernel arrives: ten times as fast as while
/// waiting, so a load is told from a wait at a glance, at any rate.
const TRANSFER_BLINK_PERIOD: Duration = Duration::from_millis(25);

/// After how many bytes of the kernel the LED's period is looked at. Reading the
/// time for every byte would slow the receive loop; 64 bytes take 5.6 ms at
/// 115200 baud and 0.32 ms at 2 Mbaud, well inside the period. A flip never
/// waits for the firmware: while it has not answered the flip before, the flip
/// is skipped (`led::Blinker::toggle`).
const TRANSFER_BLINK_CHECK_BYTES: u64 = 64;

/// How long it is since `start`, in uptime.
fn since(start: Duration) -> Duration {
    libtime::time::time_manager()
        .uptime()
        .checked_sub(start)
        .expect("uptime must not go backwards")
}

/// The next `N` bytes from the host; the first is `first` when it has been read
/// already.
fn read_bytes<const N: usize>(first: Option<u8>) -> [u8; N] {
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = match (index, first) {
            (0, Some(first)) => first,
            _ => console().read_byte(),
        };
    }
    bytes
}

/// Beacon and wait for the host as `wait` says, blinking the LED. Returns the
/// host's first byte, or `None` when `wait` gives up.
fn wait_for_host(wait: Wait, led: &mut Blinker) -> Option<u8> {
    let started = libtime::time::time_manager().uptime();
    let mut beacon_count: u64 = 0;
    loop {
        if wait.gives_up(since(started)) {
            return None;
        }
        beacon_count = beacon_count.saturating_add(1);
        let uptime = libtime::time::time_manager().uptime();
        #[cfg(not(feature = "qemu"))]
        let _: Duration = uptime;
        semi::println!(
            "⏪ Beacon #{beacon_count} at {}.{:03}s (sending ^C^C^C)",
            uptime.as_secs(),
            uptime.subsec_millis()
        );

        for byte in BEACON {
            console().write_byte(byte);
        }
        console().flush();

        let beaconed = libtime::time::time_manager().uptime();
        while since(beaconed) < wait.beacon_period {
            if let Some(b) = console().read_byte_nonblocking() {
                let now = libtime::time::time_manager().uptime();
                #[cfg(not(feature = "qemu"))]
                let _: Duration = now;
                semi::println!(
                    "⏪ Host byte 0x{b:02x} received at {}.{:03}s",
                    now.as_secs(),
                    now.subsec_millis()
                );
                return Some(b);
            }
            led.tick(BLINK_PERIOD);
            libtime::time::time_manager().spin_for(Duration::from_millis(10));
        }

        let waited = since(beaconed);
        #[cfg(not(feature = "qemu"))]
        let _: Duration = waited;
        semi::println!(
            "⏪ No host data after {}.{:03}s, retrying beacon",
            waited.as_secs(),
            waited.subsec_millis()
        );
    }
}

/// Read the rest of a rate request, whose magic has been read, and answer it at
/// the rate the UART is at; on accepting, switch the UART, running at `clock`
/// Hz, and give the host [`SWITCH_SETTLE`] to switch too. Returns the new rate
/// when it switched.
fn answer_rate_request(clock: u32) -> Option<u32> {
    let request = decode_rate_request_body(read_bytes(None))
        .ok_or(RateRefusal::Garbled)
        .and_then(|rate| validate_rate(clock, rate).map(|_divisors| rate));
    let rate = match request {
        Ok(rate) => rate,
        Err(refusal) => {
            println!("{RATE_REFUSED}\n⏪ Staying at the rate: {refusal}");
            return None;
        }
    };
    print!("{RATE_ACCEPTED}");
    // The answer leaves the line at the old rate before the switch.
    match platform::set_uart_rate(clock, rate) {
        Ok(()) => {
            libtime::time::time_manager().spin_for(SWITCH_SETTLE);
            semi::println!("⏪ Switched to {rate} baud");
            Some(rate)
        }
        Err(why) => {
            // Only the transmitter not draining fails here: nobody hears it.
            #[cfg(not(feature = "qemu"))]
            let _: &str = why;
            semi::println!("⏪ Accepted {rate} baud but stayed: {why}");
            None
        }
    }
}

/// The main function running after the early init.
#[inline(always)]
fn kernel_main(dtb: u32, max_kernel_size: u64, bringup: platform::Bringup) -> ! {
    #[cfg(test)]
    test_main();

    print!("{}", LOGO);
    println!("{:>51}\n", platform::model_name(bringup.board_revision));
    println!("Preserving DTB at {:8x}", dtb);
    println!(
        "UART clock {} Hz ({})",
        bringup.uart_clock.rate,
        if bringup.uart_clock.reported {
            "from the firmware"
        } else {
            "assumed, the firmware did not say"
        }
    );
    println!(
        "UART clock running: {}",
        match bringup.clock_running {
            Some(true) => "yes",
            Some(false) => "no",
            None => "the firmware did not say",
        }
    );
    println!("Counter frequency {} Hz", platform::counter_frequency());
    println!("MMU {}", platform::mmu_state());
    match bringup.board_revision {
        Some(revision) => println!("Board revision {revision:08x}"),
        None => println!("Board revision unknown, the firmware did not say"),
    }
    println!("⏪ Requesting kernel image...");

    let kernel_addr: *mut u8 = platform::KERNEL_LOAD_ADDRESS as *mut u8;

    // The LED shows what chainboot is doing: it blinks slowly while waiting for
    // the host, fast while the kernel arrives, and goes off before the kernel runs.
    let mut led = Blinker::new();

    let clock = bringup.uart_clock.rate;
    // The rate the UART is at, and whether the host has just been told it may
    // switch to it (see the protocol crate's documentation).
    let mut uart_rate = BOOT_BAUD_RATE;
    let mut rate_just_accepted = false;

    loop {
        console().flush();

        // Every wait that does not follow an accepted rate request is at the boot
        // rate: after a failed load, or when the host did not show up at the new
        // rate. The host goes back on the answer it just read, or has gone back.
        if !rate_just_accepted && uart_rate != BOOT_BAUD_RATE {
            match platform::set_uart_rate(clock, BOOT_BAUD_RATE) {
                Ok(()) => {
                    uart_rate = BOOT_BAUD_RATE;
                    libtime::time::time_manager().spin_for(SWITCH_SETTLE);
                }
                Err(why) => {
                    // Only the transmitter not draining fails here: nobody hears it.
                    #[cfg(not(feature = "qemu"))]
                    let _: &str = why;
                    semi::println!("⏪ Could not go back to {BOOT_BAUD_RATE} baud: {why}");
                }
            }
        }
        let wait = if rate_just_accepted {
            FAST_RATE_WAIT
        } else {
            BOOT_RATE_WAIT
        };
        rate_just_accepted = false;

        // Read the binary's size, or a rate request.
        // While waiting, periodically emit sync beacons so a late-attaching host can still catch us.
        console().clear_rx();
        let Some(first) = wait_for_host(wait, &mut led) else {
            semi::println!("⏪ No host at {uart_rate} baud, back to {BOOT_BAUD_RATE}");
            continue;
        };

        let size = match opening(read_bytes(Some(first))) {
            Opening::Size(size) => size,
            Opening::RateRequest => {
                if let Some(rate) = answer_rate_request(clock) {
                    uart_rate = rate;
                    rate_just_accepted = true;
                }
                continue;
            }
        };

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
            if i % TRANSFER_BLINK_CHECK_BYTES == 0 {
                led.tick(TRANSFER_BLINK_PERIOD);
            }
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
        let checksum = u64::from_le_bytes(read_bytes(None));

        let valid = hasher.finish() == checksum;
        if !valid {
            println!("ERR ❌ Kernel image checksum mismatch");
            continue;
        }
        semi::println!("Read kernel checksum {checksum:016x}");

        print!("OK");
        break;
    }

    // The kernel stays at the rate the boot loader set: the UART is left at
    // `uart_rate`, the rate the load ran at, and Vesper's UART driver keeps the
    // divisors it finds. chainofcommand stays at that rate for the console
    // (`send_kernel` there).
    semi::println!("⏪ Handing over at {uart_rate} baud");

    if let Err(why) = platform::led::set(false) {
        println!("The LED stays as it is: {why}");
    }

    println!(
        "\n⏪ Loaded! Executing the payload now from {:p}\n",
        kernel_addr
    );
    console().flush();

    // The kernel starts with the MMU off, as the firmware starts chainboot.
    #[cfg(feature = "mmu")]
    platform::mmu::disable();

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
