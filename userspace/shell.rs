fn print_mmu_state_and_features() {
    // use machine::memory::mmu::interface::MMU;
    libmemory::arch::features::print_features();
}

// TODO: AFTER INIT_THREAD, one of the userspace processes
//
//------------------------------------------------------------
// Start a command prompt
//------------------------------------------------------------
fn command_prompt() {
    'cmd_loop: loop {
        let mut buf = [0_u8; 64];

        match libconsole::console::command_prompt(&mut buf) {
            // b"mmu" => init_mmu(),
            b"feats" => print_mmu_state_and_features(),
            // b"disp" => check_display_init(),
            // b"trap" => check_data_abort_trap(),
            // b"map" => machine::platform::memory::mmu::virt_mem_layout().print_layout(),
            // b"led on" => set_led(true),
            // b"led off" => set_led(false),
            b"help" => print_help(),
            b"end" => break 'cmd_loop,
            x => warn!("[!] Unknown command {x:?}, try 'help'"),
        }
    }
}

fn print_help() {
    println!("Supported console commands:");
    println!("  mmu  - initialize MMU");
    println!("  feats - print MMU state and supported features");
    #[cfg(not(feature = "noserial"))]
    println!("  uart - try to reinitialize UART serial");
    // println!("  disp - try to init VC framebuffer and draw some text");
    println!("  trap - trigger and recover from a data abort exception");
    println!("  map  - show kernel memory layout");
    // println!("  led [on|off]  - change RPi LED status");
    println!("  end  - leave console and reset board");
}

// fn set_led(enable: bool) {
//     let mut mbox = Mailbox::<8>::default();
//     let index = mbox.request();
//     let index = mbox.set_led_on(index, enable);
//     let mbox = mbox.end(index);
//
//     mbox.call(channel::PropertyTagsArmToVc)
//         .map_err(|e| {
//             warn!("Mailbox call returned error {}", e);
//             warn!("Mailbox contents: {:?}", mbox);
//         })
//         .ok();
// }

fn reboot() -> ! {
    cfg_if! {
        if #[cfg(feature = "qemu")] {
            info!("Bye, shutting down QEMU");
            libqemu::semihosting::exit_success()
        } else {
            // use machine::platform::raspberrypi::power::Power;

            info!("Bye, going to reset now");
            // Power::default().reset()
            libcpu::endless_sleep()
        }
    }
}
