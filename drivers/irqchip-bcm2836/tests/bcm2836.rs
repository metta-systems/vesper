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
            ArmctrlRegisterBlock, BasicInterrupts, Bcm2836, ConfigError, Core, GpuInterrupts,
            GpuRouting, IrqSource, KIND_ARMCTRL, KIND_LOCAL, LINE_CNTPNS, LINE_CNTV, LINE_COUNT,
            LINE_MAILBOX0, LINE_PMU, LocalRegisterBlock, MailboxControl, PmuRouting, TimerControl,
            armctrl_line,
        },
        libirqchip::{Controller, INVALID_LINE, IrqChipConfig, MmioRegion, SPURIOUS},
    };

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
        core0_timer_int_control: TimerControl,
        _core1_3_timer_int_control: [u32; 3],
        core0_mailbox_int_control: MailboxControl,
        _core1_3_mailbox_int_control: [u32; 3],
        core0_irq_source: IrqSource,
        _core1_3_irq_source: [u32; 3],
        _fiq_sources: [u32; 4],
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

    /// A chip bound to fresh local and ARM-control blocks.
    fn chip() -> (
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
        let chip = unsafe { Bcm2836::new(&config) }.expect("valid config");
        (chip, local, armctrl)
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
    fn init_masks_every_line_and_routes_the_gpu_to_core0() {
        let (_chip, local, armctrl) = chip();
        assert_eq!(local.read_core0_timer_int_control(), TimerControl::ZERO);
        assert_eq!(local.read_core0_mailbox_int_control(), MailboxControl::ZERO);
        assert!(local.read_pmu_int_routing_clear().irq_core(0));
        assert_eq!(
            local.read_gpu_int_routing(),
            GpuRouting::ZERO
                .with_irq_core(Core::Core0)
                .with_fiq_core(Core::Core0)
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
    fn a_config_without_the_local_controller_is_rejected() {
        let (_armctrl, region) = armctrl_device();
        let mut config = IrqChipConfig {
            controller_count: 1,
            ..IrqChipConfig::default()
        };
        config.controllers[0] = controller(KIND_ARMCTRL, region);
        // SAFETY: the block is leaked.
        let result = unsafe { Bcm2836::new(&config) };
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
        let result = unsafe { Bcm2836::new(&config) };
        assert!(matches!(result, Err(ConfigError::BadRegion)));
    }

    #[test]
    fn timer_lines_toggle_their_core0_enables() {
        let (mut chip, local, _armctrl) = chip();
        chip.set_enabled(LINE_CNTPNS, true);
        assert!(local.read_core0_timer_int_control().irq(1));
        chip.set_enabled(LINE_CNTV, true);
        assert_eq!(
            local.read_core0_timer_int_control(),
            TimerControl::ZERO.with_irq(1, true).with_irq(3, true)
        );
        chip.set_enabled(LINE_CNTPNS, false);
        assert!(!local.read_core0_timer_int_control().irq(1));
        assert!(local.read_core0_timer_int_control().irq(3));
    }

    #[test]
    fn mailbox_and_pmu_lines_use_their_own_registers() {
        let (mut chip, local, _armctrl) = chip();
        chip.set_enabled(LINE_MAILBOX0 + 1, true);
        assert!(local.read_core0_mailbox_int_control().irq(1));
        assert!(!local.read_core0_mailbox_int_control().irq(0));
        chip.set_enabled(LINE_PMU, true);
        assert!(local.read_pmu_int_routing_set().irq_core(0));
    }

    #[test]
    fn claim_returns_the_first_direct_local_line() {
        let (chip, mut local, _armctrl) = chip();
        assert_eq!(chip.claim(), SPURIOUS);
        local.write_core0_irq_source(sources(&[LINE_CNTPNS, LINE_CNTV]));
        assert_eq!(chip.claim(), LINE_CNTPNS);
        local.write_core0_irq_source(sources(&[LINE_PMU]));
        assert_eq!(chip.claim(), LINE_PMU);
    }

    #[test]
    fn claim_demuxes_the_gpu_cascade_through_enabled_interrupts() {
        let (mut chip, mut local, mut armctrl) = chip();
        local.write_core0_irq_source(sources(&[8]));

        // GPU interrupt 57 (PL011) is bank 2, irq 25: pending but not enabled.
        armctrl.write_gpu_pending(1, gpu_irq(25)).unwrap();
        assert_eq!(chip.claim(), SPURIOUS);

        let uart = Bcm2836::xlate(KIND_ARMCTRL, &[2, 25]);
        assert_eq!(uart, armctrl_line(2, 25));
        chip.set_enabled(uart, true);
        assert_eq!(armctrl.read_gpu_enable(1).unwrap(), gpu_irq(25));
        assert_eq!(chip.claim(), uart);

        // A pending, enabled bank-1 interrupt is claimed before bank 2.
        let gpu_line = armctrl_line(1, 3);
        chip.set_enabled(gpu_line, true);
        armctrl.write_gpu_pending(0, gpu_irq(3)).unwrap();
        assert_eq!(chip.claim(), gpu_line);

        // ARM peripherals (bank 0) come first.
        let arm_timer = armctrl_line(0, 0);
        chip.set_enabled(arm_timer, true);
        assert!(armctrl.read_basic_enable().arm_peripheral(0));
        armctrl.write_basic_pending(BasicInterrupts::ZERO.with_arm_peripheral(0, true));
        assert_eq!(chip.claim(), arm_timer);

        // Masking writes the disable register and stops delivery.
        chip.set_enabled(arm_timer, false);
        assert!(armctrl.read_basic_disable().arm_peripheral(0));
        assert_eq!(chip.claim(), gpu_line);
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
