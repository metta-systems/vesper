//! Host tests of the BCM2836 register logic against in-memory register blocks.
//!
//! Each test plays the hardware through a *device view* of the register
//! blocks: the same layout as the driver's `register_structs!`, with every
//! register readable and writable, so a test can raise pending interrupts in
//! read-only registers and observe what the driver wrote to write-only ones.

#[cfg(test)]
mod tests {
    use {
        irqchip_bcm2836::{
            ArmctrlRegisterBlock, BASIC, Bcm2836, ConfigError, GPU_IRQS, GPU_ROUTING, IRQ_SOURCE,
            KIND_ARMCTRL, KIND_LOCAL, LINE_CNTPNS, LINE_CNTV, LINE_COUNT, LINE_MAILBOX0, LINE_PMU,
            LocalRegisterBlock, MAILBOX_CONTROL, PMU_ROUTING, TIMER_CONTROL, armctrl_line, gpu_irq,
        },
        libirqchip::{Controller, INVALID_LINE, IrqChipConfig, MmioRegion, SPURIOUS},
        tock_registers::{
            interfaces::{Readable, Writeable},
            register_structs,
            registers::ReadWrite,
        },
    };

    register_structs! {
        /// The local peripherals block as the hardware sees it.
        #[allow(non_snake_case)]
        LocalDevice {
            (0x00 => _control),
            (0x0C => GPU_INT_ROUTING: ReadWrite<u32, GPU_ROUTING::Register>),
            (0x10 => PMU_INT_ROUTING_SET: ReadWrite<u32, PMU_ROUTING::Register>),
            (0x14 => PMU_INT_ROUTING_CLEAR: ReadWrite<u32, PMU_ROUTING::Register>),
            (0x18 => _timers),
            (0x40 => CORE_TIMER_INT_CONTROL: [ReadWrite<u32, TIMER_CONTROL::Register>; 4]),
            (0x50 => CORE_MAILBOX_INT_CONTROL: [ReadWrite<u32, MAILBOX_CONTROL::Register>; 4]),
            (0x60 => CORE_IRQ_SOURCE: [ReadWrite<u32, IRQ_SOURCE::Register>; 4]),
            (0x70 => _fiq_sources),
            (0x80 => @END),
        }
    }

    register_structs! {
        /// The ARM-control block as the hardware sees it.
        #[allow(non_snake_case)]
        ArmctrlDevice {
            (0x00 => BASIC_PENDING: ReadWrite<u32, BASIC::Register>),
            (0x04 => GPU_PENDING: [ReadWrite<u32, GPU_IRQS::Register>; 2]),
            (0x0C => _fiq_control),
            (0x10 => GPU_ENABLE: [ReadWrite<u32, GPU_IRQS::Register>; 2]),
            (0x18 => BASIC_ENABLE: ReadWrite<u32, BASIC::Register>),
            (0x1C => GPU_DISABLE: [ReadWrite<u32, GPU_IRQS::Register>; 2]),
            (0x24 => BASIC_DISABLE: ReadWrite<u32, BASIC::Register>),
            (0x28 => @END),
        }
    }

    // The device views must cover exactly the driver's blocks.
    const _: () = assert!(size_of::<LocalDevice>() == size_of::<LocalRegisterBlock>());
    const _: () = assert!(size_of::<ArmctrlDevice>() == size_of::<ArmctrlRegisterBlock>());

    /// Zeroed, leaked backing memory for register block `T`, with its region.
    fn device<T>() -> (&'static T, MmioRegion) {
        let words = size_of::<T>().div_ceil(size_of::<u32>());
        let memory: &'static mut [u32] = Box::leak(vec![0_u32; words].into_boxed_slice());
        let region = MmioRegion {
            virt: memory.as_ptr() as u64,
            size: size_of::<T>() as u64,
        };
        // SAFETY: zeroed, u32-aligned memory of the block's size, leaked for
        // 'static; the registers are plain `u32` cells.
        (unsafe { &*memory.as_ptr().cast::<T>() }, region)
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
    fn chip() -> (Bcm2836, &'static LocalDevice, &'static ArmctrlDevice) {
        let (local, local_region) = device::<LocalDevice>();
        let (armctrl, armctrl_region) = device::<ArmctrlDevice>();
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

    #[test]
    fn init_masks_every_line_and_routes_the_gpu_to_core0() {
        let (_chip, local, armctrl) = chip();
        assert_eq!(local.CORE_TIMER_INT_CONTROL[0].get(), 0);
        assert_eq!(local.CORE_MAILBOX_INT_CONTROL[0].get(), 0);
        assert!(local.PMU_INT_ROUTING_CLEAR.is_set(PMU_ROUTING::CORE0_IRQ));
        assert!(
            local
                .GPU_INT_ROUTING
                .matches_all(GPU_ROUTING::IRQ_CORE::Core0 + GPU_ROUTING::FIQ_CORE::Core0)
        );
        assert_eq!(
            armctrl.BASIC_DISABLE.read(BASIC::ARM_PERIPHERALS),
            BASIC::ARM_PERIPHERALS.mask
        );
        for disable in &armctrl.GPU_DISABLE {
            assert_eq!(disable.read(GPU_IRQS::ALL), GPU_IRQS::ALL.mask);
        }
    }

    #[test]
    fn a_config_without_the_local_controller_is_rejected() {
        let (_armctrl, region) = device::<ArmctrlDevice>();
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
        let (_local, mut region) = device::<LocalDevice>();
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
        assert!(local.CORE_TIMER_INT_CONTROL[0].is_set(TIMER_CONTROL::CNTPNS_IRQ));
        chip.set_enabled(LINE_CNTV, true);
        assert!(
            local.CORE_TIMER_INT_CONTROL[0]
                .matches_all(TIMER_CONTROL::CNTPNS_IRQ::SET + TIMER_CONTROL::CNTV_IRQ::SET)
        );
        chip.set_enabled(LINE_CNTPNS, false);
        assert!(!local.CORE_TIMER_INT_CONTROL[0].is_set(TIMER_CONTROL::CNTPNS_IRQ));
        assert!(local.CORE_TIMER_INT_CONTROL[0].is_set(TIMER_CONTROL::CNTV_IRQ));
    }

    #[test]
    fn mailbox_and_pmu_lines_use_their_own_registers() {
        let (mut chip, local, _armctrl) = chip();
        chip.set_enabled(LINE_MAILBOX0 + 1, true);
        assert!(local.CORE_MAILBOX_INT_CONTROL[0].is_set(MAILBOX_CONTROL::MAILBOX1_IRQ));
        assert!(!local.CORE_MAILBOX_INT_CONTROL[0].is_set(MAILBOX_CONTROL::MAILBOX0_IRQ));
        chip.set_enabled(LINE_PMU, true);
        assert!(local.PMU_INT_ROUTING_SET.is_set(PMU_ROUTING::CORE0_IRQ));
    }

    #[test]
    fn claim_returns_the_first_direct_local_line() {
        let (chip, local, _armctrl) = chip();
        assert_eq!(chip.claim(), SPURIOUS);
        local.CORE_IRQ_SOURCE[0].write(IRQ_SOURCE::CNTPNS::SET + IRQ_SOURCE::CNTV::SET);
        assert_eq!(chip.claim(), LINE_CNTPNS);
        local.CORE_IRQ_SOURCE[0].write(IRQ_SOURCE::PMU::SET);
        assert_eq!(chip.claim(), LINE_PMU);
    }

    #[test]
    fn claim_demuxes_the_gpu_cascade_through_enabled_interrupts() {
        let (mut chip, local, armctrl) = chip();
        local.CORE_IRQ_SOURCE[0].write(IRQ_SOURCE::GPU::SET);

        // GPU interrupt 57 (PL011) is bank 2, irq 25: pending but not enabled.
        armctrl.GPU_PENDING[1].write(gpu_irq(25));
        assert_eq!(chip.claim(), SPURIOUS);

        let uart = Bcm2836::xlate(KIND_ARMCTRL, &[2, 25]);
        assert_eq!(uart, armctrl_line(2, 25));
        chip.set_enabled(uart, true);
        assert!(armctrl.GPU_ENABLE[1].matches_all(gpu_irq(25)));
        assert_eq!(chip.claim(), uart);

        // A pending, enabled bank-1 interrupt is claimed before bank 2.
        let gpu_line = armctrl_line(1, 3);
        chip.set_enabled(gpu_line, true);
        armctrl.GPU_PENDING[0].write(gpu_irq(3));
        assert_eq!(chip.claim(), gpu_line);

        // ARM peripherals (bank 0) come first.
        let arm_timer = armctrl_line(0, 0);
        chip.set_enabled(arm_timer, true);
        assert!(armctrl.BASIC_ENABLE.is_set(BASIC::ARM_TIMER));
        armctrl.BASIC_PENDING.write(BASIC::ARM_TIMER::SET);
        assert_eq!(chip.claim(), arm_timer);

        // Masking writes the disable register and stops delivery.
        chip.set_enabled(arm_timer, false);
        assert!(armctrl.BASIC_DISABLE.is_set(BASIC::ARM_TIMER));
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
