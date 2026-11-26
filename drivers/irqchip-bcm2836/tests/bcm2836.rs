//! Host tests of the BCM2836 register logic against in-memory register blocks.
//!
//! Each test plays the hardware through a *device view* of the register
//! blocks: the same layout as the driver's blocks, with every register
//! readable and writable, so a test can raise pending interrupts in read-only
//! registers and observe what the driver wrote to write-only ones.

#[cfg(test)]
mod tests {
    use {
        derive_mmio::Mmio,
        irqchip_bcm2836::{
            ArmctrlRegisterBlock, BasicInterrupts, Bcm2836, CORES, ConfigError, Core,
            GpuInterrupts, GpuRouting, IPI_LINE, IrqSource, KIND_ARMCTRL, KIND_LOCAL, LINE_CNTPNS,
            LINE_CNTV, LINE_COUNT, LINE_MAILBOX0, LINE_PMU, LocalRegisterBlock, Mailbox,
            MailboxControl, PmuRouting, TimerControl, armctrl_line,
        },
        libirqchip::{Controller, INVALID_LINE, IrqChipConfig, MAX_CPUS, MmioRegion, SPURIOUS},
    };

    /// Mailboxes per core.
    const MAILBOXES: usize = 4;

    /// The local peripherals block as the hardware sees it.
    #[derive(Mmio)]
    #[mmio(no_owned)]
    #[repr(C)]
    struct LocalDevice {
        _control: [u32; 3],
        gpu_int_routing: GpuRouting,
        pmu_int_routing_set: PmuRouting,
        pmu_int_routing_clear: PmuRouting,
        _timers: [u32; 10],
        timer_int_control: [TimerControl; CORES],
        mailbox_int_control: [MailboxControl; CORES],
        irq_source: [IrqSource; CORES],
        _fiq_sources: [u32; CORES],
        mailbox_write_set: [Mailbox; CORES * MAILBOXES],
        mailbox_read_clear: [Mailbox; CORES * MAILBOXES],
    }

    /// The ARM-control block as the hardware sees it.
    #[derive(Mmio)]
    #[mmio(no_owned)]
    #[repr(C)]
    struct ArmctrlDevice {
        basic_pending: BasicInterrupts,
        gpu_pending: [GpuInterrupts; 2],
        _fiq_control: u32,
        gpu_enable: [GpuInterrupts; 2],
        basic_enable: BasicInterrupts,
        gpu_disable: [GpuInterrupts; 2],
        basic_disable: BasicInterrupts,
    }

    // The device views must cover exactly the driver's blocks.
    const _: () = assert!(size_of::<LocalDevice>() == size_of::<LocalRegisterBlock>());
    const _: () = assert!(size_of::<ArmctrlDevice>() == size_of::<ArmctrlRegisterBlock>());

    /// Zeroed, leaked backing memory for register block `T`, with its region.
    fn device<T>() -> (*mut T, MmioRegion) {
        let words = size_of::<T>().div_ceil(size_of::<u32>());
        let memory: &'static mut [u32] = Box::leak(vec![0_u32; words].into_boxed_slice());
        let region = MmioRegion {
            virt: memory.as_ptr() as u64,
            size: size_of::<T>() as u64,
        };
        (memory.as_mut_ptr().cast::<T>(), region)
    }

    fn local_device() -> (MmioLocalDevice<'static>, MmioRegion) {
        let (block, region) = device::<LocalDevice>();
        // SAFETY: zeroed, u32-aligned memory of the block's size, leaked for
        // 'static.
        (unsafe { LocalDevice::new_mmio(block) }, region)
    }

    fn armctrl_device() -> (MmioArmctrlDevice<'static>, MmioRegion) {
        let (block, region) = device::<ArmctrlDevice>();
        // SAFETY: as in `local_device`.
        (unsafe { ArmctrlDevice::new_mmio(block) }, region)
    }

    fn controller(kind: u32, region: MmioRegion) -> Controller {
        let mut controller = Controller {
            kind,
            region_count: 1,
            ..Controller::default()
        };
        controller.regions[0] = region;
        controller
    }

    /// A chip bound to fresh local and ARM-control blocks by hardware core
    /// `boot_core`.
    fn chip_on(
        boot_core: usize,
    ) -> (
        Bcm2836,
        MmioLocalDevice<'static>,
        MmioArmctrlDevice<'static>,
    ) {
        let (local, local_region) = local_device();
        let (armctrl, armctrl_region) = armctrl_device();
        let mut config = IrqChipConfig {
            controller_count: 2,
            ..IrqChipConfig::default()
        };
        config.controllers[0] = controller(KIND_LOCAL, local_region);
        config.controllers[1] = controller(KIND_ARMCTRL, armctrl_region);
        // SAFETY: the blocks are leaked, so they live for 'static.
        let chip = unsafe { Bcm2836::new(&config, boot_core) }.expect("valid config");
        (chip, local, armctrl)
    }

    /// A chip bound by core 0.
    fn chip() -> (
        Bcm2836,
        MmioLocalDevice<'static>,
        MmioArmctrlDevice<'static>,
    ) {
        chip_on(0)
    }

    /// The interrupt sources with exactly `lines` raised.
    fn sources(lines: &[u32]) -> IrqSource {
        IrqSource::new_with_raw_value(lines.iter().fold(0, |bits, line| bits | 1 << line))
    }

    /// GPU interrupt `irq` of a bank.
    fn gpu_irq(irq: usize) -> GpuInterrupts {
        GpuInterrupts::ZERO.with_irq(irq, true)
    }

    #[test]
    fn init_masks_every_core_clears_every_mailbox_and_routes_the_gpu_to_the_boot_core() {
        let (_chip, local, armctrl) = chip_on(2);
        for core in 0..CORES {
            assert_eq!(
                local.read_timer_int_control(core).unwrap(),
                TimerControl::ZERO
            );
            assert_eq!(
                local.read_mailbox_int_control(core).unwrap(),
                MailboxControl::ZERO
            );
        }
        for mailbox in 0..CORES * MAILBOXES {
            assert_eq!(
                local.read_mailbox_read_clear(mailbox).unwrap(),
                Mailbox::ALL
            );
        }
        assert_eq!(
            local.read_pmu_int_routing_clear(),
            PmuRouting::new_with_raw_value(0xff)
        );
        assert_eq!(
            local.read_gpu_int_routing(),
            GpuRouting::ZERO
                .with_irq_core(Core::Core2)
                .with_fiq_core(Core::Core2)
        );
        assert_eq!(
            armctrl.read_basic_disable(),
            BasicInterrupts::ALL_ARM_PERIPHERALS
        );
        for bank in 0..armctrl.len_gpu_disable() {
            assert_eq!(armctrl.read_gpu_disable(bank).unwrap(), GpuInterrupts::ALL);
        }
    }

    #[test]
    fn a_boot_core_the_chip_does_not_have_is_rejected() {
        let (_local, region) = local_device();
        let mut config = IrqChipConfig {
            controller_count: 1,
            ..IrqChipConfig::default()
        };
        config.controllers[0] = controller(KIND_LOCAL, region);
        // SAFETY: the block is leaked.
        let result = unsafe { Bcm2836::new(&config, CORES) };
        assert!(matches!(result, Err(ConfigError::BadCore)));
    }

    #[test]
    fn a_config_without_the_local_controller_is_rejected() {
        let (_armctrl, region) = armctrl_device();
        let mut config = IrqChipConfig {
            controller_count: 1,
            ..IrqChipConfig::default()
        };
        config.controllers[0] = controller(KIND_ARMCTRL, region);
        // SAFETY: the block is leaked.
        let result = unsafe { Bcm2836::new(&config, 0) };
        assert!(matches!(result, Err(ConfigError::MissingLocal)));
    }

    #[test]
    fn a_region_smaller_than_its_block_is_rejected() {
        let (_local, mut region) = local_device();
        region.size -= 4;
        let mut config = IrqChipConfig {
            controller_count: 1,
            ..IrqChipConfig::default()
        };
        config.controllers[0] = controller(KIND_LOCAL, region);
        // SAFETY: the block is leaked.
        let result = unsafe { Bcm2836::new(&config, 0) };
        assert!(matches!(result, Err(ConfigError::BadRegion)));
    }

    #[test]
    fn timer_lines_toggle_the_calling_cores_enables() {
        let (mut chip, local, _armctrl) = chip();
        chip.set_enabled(LINE_CNTPNS, true, 0);
        assert!(local.read_timer_int_control(0).unwrap().irq(1));
        chip.set_enabled(LINE_CNTV, true, 0);
        assert_eq!(
            local.read_timer_int_control(0).unwrap(),
            TimerControl::ZERO.with_irq(1, true).with_irq(3, true)
        );
        chip.set_enabled(LINE_CNTPNS, false, 0);
        assert!(!local.read_timer_int_control(0).unwrap().irq(1));
        assert!(local.read_timer_int_control(0).unwrap().irq(3));

        // Core 2's timer is its own register; core 0's stays as it was.
        chip.set_enabled(LINE_CNTPNS, true, 2);
        assert_eq!(
            local.read_timer_int_control(2).unwrap(),
            TimerControl::ZERO.with_irq(1, true)
        );
        assert!(!local.read_timer_int_control(0).unwrap().irq(1));
        for core in [1, 3] {
            assert_eq!(
                local.read_timer_int_control(core).unwrap(),
                TimerControl::ZERO
            );
        }
    }

    #[test]
    fn mailbox_and_pmu_lines_use_the_calling_cores_registers() {
        let (mut chip, local, _armctrl) = chip();
        chip.set_enabled(LINE_MAILBOX0 + 1, true, 0);
        assert!(local.read_mailbox_int_control(0).unwrap().irq(1));
        assert!(!local.read_mailbox_int_control(0).unwrap().irq(0));
        chip.set_enabled(LINE_PMU, true, 0);
        assert_eq!(
            local.read_pmu_int_routing_set(),
            PmuRouting::ZERO.with_irq_core(0, true)
        );
        chip.set_enabled(LINE_MAILBOX0, true, 3);
        assert!(local.read_mailbox_int_control(3).unwrap().irq(0));
        chip.set_enabled(LINE_PMU, true, 3);
        assert_eq!(
            local.read_pmu_int_routing_set(),
            PmuRouting::ZERO.with_irq_core(3, true)
        );
    }

    #[test]
    fn claim_returns_the_first_direct_local_line_of_the_calling_core() {
        let (mut chip, mut local, _armctrl) = chip();
        assert_eq!(chip.claim(0), SPURIOUS);
        local
            .write_irq_source(0, sources(&[LINE_CNTPNS, LINE_CNTV]))
            .unwrap();
        assert_eq!(chip.claim(0), LINE_CNTPNS);
        local.write_irq_source(0, sources(&[LINE_PMU])).unwrap();
        assert_eq!(chip.claim(0), LINE_PMU);

        // Core 1 sees only its own sources.
        assert_eq!(chip.claim(1), SPURIOUS);
        local.write_irq_source(1, sources(&[LINE_CNTV])).unwrap();
        assert_eq!(chip.claim(1), LINE_CNTV);
        assert_eq!(chip.claim(CORES), SPURIOUS);
    }

    #[test]
    fn claim_returns_a_pending_mailbox_line() {
        let (mut chip, mut local, _armctrl) = chip();
        local.write_irq_source(2, sources(&[IPI_LINE])).unwrap();
        local
            .write_mailbox_read_clear(MAILBOXES * 2, Mailbox::IPI)
            .unwrap();
        assert_eq!(chip.claim(2), IPI_LINE);
        // A timer comes before a mailbox.
        local
            .write_irq_source(2, sources(&[LINE_CNTPNS, IPI_LINE]))
            .unwrap();
        assert_eq!(chip.claim(2), LINE_CNTPNS);
    }

    #[test]
    fn claim_demuxes_the_gpu_cascade_through_enabled_interrupts() {
        let (mut chip, mut local, mut armctrl) = chip();
        local.write_irq_source(0, sources(&[8])).unwrap();

        // GPU interrupt 57 (PL011) is bank 2, irq 25: pending but not enabled.
        armctrl.write_gpu_pending(1, gpu_irq(25)).unwrap();
        assert_eq!(chip.claim(0), SPURIOUS);

        let uart = Bcm2836::xlate(KIND_ARMCTRL, &[2, 25]);
        assert_eq!(uart, armctrl_line(2, 25));
        chip.set_enabled(uart, true, 0);
        assert_eq!(armctrl.read_gpu_enable(1).unwrap(), gpu_irq(25));
        assert_eq!(chip.claim(0), uart);

        // A pending, enabled bank-1 interrupt is claimed before bank 2.
        let gpu_line = armctrl_line(1, 3);
        chip.set_enabled(gpu_line, true, 0);
        armctrl.write_gpu_pending(0, gpu_irq(3)).unwrap();
        assert_eq!(chip.claim(0), gpu_line);

        // ARM peripherals (bank 0) come first.
        let arm_timer = armctrl_line(0, 0);
        chip.set_enabled(arm_timer, true, 0);
        assert!(armctrl.read_basic_enable().arm_peripheral(0));
        armctrl.write_basic_pending(BasicInterrupts::ZERO.with_arm_peripheral(0, true));
        assert_eq!(chip.claim(0), arm_timer);

        // Masking writes the disable register and stops delivery.
        chip.set_enabled(arm_timer, false, 0);
        assert!(armctrl.read_basic_disable().arm_peripheral(0));
        assert_eq!(chip.claim(0), gpu_line);
    }

    #[test]
    fn init_cpu_records_the_core_and_unmasks_its_ipi_mailbox() {
        let (mut chip, local, _armctrl) = chip();
        chip.init_cpu(1, 2).expect("core 2 exists");
        assert_eq!(
            local.read_mailbox_int_control(2).unwrap(),
            MailboxControl::ZERO.with_irq(0, true)
        );
        for core in [0, 1, 3] {
            assert_eq!(
                local.read_mailbox_int_control(core).unwrap(),
                MailboxControl::ZERO
            );
        }
        assert_eq!(chip.init_cpu(0, CORES), Err(ConfigError::BadCore));
        assert_eq!(
            chip.init_cpu(u32::try_from(MAX_CPUS).unwrap(), 0),
            Err(ConfigError::BadCpu)
        );
    }

    #[test]
    fn send_ipi_sets_mailbox_0_of_each_target_core() {
        let (mut chip, local, _armctrl) = chip();
        // Logical CPUs 0..3 on hardware cores 3, 2, 1, 0: the mask names logical CPUs.
        for (cpu, core) in [(0, 3), (1, 2), (2, 1), (3, 0)] {
            chip.init_cpu(cpu, core).expect("valid core");
        }
        chip.send_ipi(0b0101).expect("CPUs 0 and 2 are known");
        for core in 0..CORES {
            let expected = if core == 3 || core == 1 {
                Mailbox::IPI
            } else {
                Mailbox::ZERO
            };
            assert_eq!(
                local.read_mailbox_write_set(MAILBOXES * core).unwrap(),
                expected,
                "core {core}"
            );
            for mailbox in 1..MAILBOXES {
                assert_eq!(
                    local
                        .read_mailbox_write_set(MAILBOXES * core + mailbox)
                        .unwrap(),
                    Mailbox::ZERO
                );
            }
        }
    }

    #[test]
    fn send_ipi_to_an_unknown_cpu_signals_nobody() {
        let (mut chip, local, _armctrl) = chip();
        chip.init_cpu(0, 0).expect("core 0 exists");
        assert_eq!(chip.send_ipi(0b11), Err(ConfigError::UnknownCpu));
        assert_eq!(chip.send_ipi(1 << MAX_CPUS), Err(ConfigError::UnknownCpu));
        for mailbox in 0..CORES * MAILBOXES {
            assert_eq!(
                local.read_mailbox_write_set(mailbox).unwrap(),
                Mailbox::ZERO
            );
        }
        // An empty mask is no IPI and no error.
        assert_eq!(chip.send_ipi(0), Ok(()));
    }

    #[test]
    fn xlate_follows_the_device_tree_bindings() {
        // The RPi3 timer node: <0 4 1 4 3 4 2 4>; CNTPNS is the second entry.
        assert_eq!(Bcm2836::xlate(KIND_LOCAL, &[1, 4]), LINE_CNTPNS);
        // The cascade itself is not a deliverable line.
        assert_eq!(Bcm2836::xlate(KIND_LOCAL, &[8, 4]), INVALID_LINE);
        assert_eq!(Bcm2836::xlate(KIND_ARMCTRL, &[3, 0]), INVALID_LINE);
        assert_eq!(Bcm2836::xlate(KIND_ARMCTRL, &[1]), INVALID_LINE);
        assert_eq!(Bcm2836::xlate(7, &[1, 4]), INVALID_LINE);
        assert_eq!(Bcm2836::xlate(KIND_ARMCTRL, &[2, 31]), LINE_COUNT - 1);
    }
}
