//! Host tests of the `GICv2` component against in-memory register blocks.
//!
//! Each test plays the hardware through a *device view* of the distributor and
//! CPU interface: the `GICv2` register layout, every register readable and
//! writable, so a test can present a pending interrupt in `GICC_IAR` and observe
//! what the component wrote to the set/clear registers.

#[cfg(test)]
mod tests {
    use {
        arm_gic::gicv2::registers::{Gicc, Gicd},
        irqchip_gicv2::{ConfigError, Gic400, LINE_COUNT},
        libirqchip::{Controller, INVALID_LINE, IrqChipConfig, MmioRegion, SPURIOUS},
        tock_registers::{
            fields::FieldValue,
            interfaces::{Readable, Writeable},
            register_bitfields, register_structs,
            registers::ReadWrite,
        },
    };

    register_bitfields! {
        u32,

        /// Distributor control (no Security Extensions view).
        GICD_CTLR [
            ENABLE_GRP0 OFFSET(0) NUMBITS(1) [],
            ENABLE_GRP1 OFFSET(1) NUMBITS(1) [],
        ],

        /// CPU interface control.
        GICC_CTLR [
            ENABLE OFFSET(0) NUMBITS(1) [],
        ],

        /// Interrupt ID of `GICC_IAR`/`GICC_EOIR`.
        INTERRUPT_ID [
            ID OFFSET(0) NUMBITS(10) [],
        ],

        /// Priority mask.
        GICC_PMR [
            PRIORITY OFFSET(0) NUMBITS(8) [],
        ],
    }

    register_structs! {
        /// The `GICv2` distributor as the hardware sees it.
        #[allow(non_snake_case)]
        DistributorDevice {
            (0x000 => CTLR: ReadWrite<u32, GICD_CTLR::Register>),
            (0x004 => TYPER: ReadWrite<u32>),
            (0x008 => _iidr),
            (0x080 => IGROUPR: [ReadWrite<u32>; 32]),
            (0x100 => ISENABLER: [ReadWrite<u32>; 32]),
            (0x180 => ICENABLER: [ReadWrite<u32>; 32]),
            (0x200 => _pending_and_active),
            (0x400 => _priorities_and_targets),
            (0xC00 => ICFGR: [ReadWrite<u32>; 64]),
            (0xD00 => _reserved),
            (0xF00 => _sgir),
            (0xF04 => _padding),
            (0xF08 => @END),
        }
    }

    register_structs! {
        /// The `GICv2` CPU interface as the hardware sees it.
        #[allow(non_snake_case)]
        CpuInterfaceDevice {
            (0x0000 => CTLR: ReadWrite<u32, GICC_CTLR::Register>),
            (0x0004 => PMR: ReadWrite<u32, GICC_PMR::Register>),
            (0x0008 => _bpr),
            (0x000C => IAR: ReadWrite<u32, INTERRUPT_ID::Register>),
            (0x0010 => EOIR: ReadWrite<u32, INTERRUPT_ID::Register>),
            (0x0014 => _status),
            (0x1000 => _dir),
            (0x1004 => _padding),
            (0x1008 => @END),
        }
    }

    // The device views must cover exactly `arm-gic`'s blocks.
    const _: () = assert!(size_of::<DistributorDevice>() == size_of::<Gicd>());
    const _: () = assert!(size_of::<CpuInterfaceDevice>() == size_of::<Gicc>());

    /// Zeroed, leaked, 8-aligned backing memory for register block `T`.
    fn device<T>() -> (&'static T, MmioRegion) {
        let words = size_of::<T>().div_ceil(size_of::<u64>());
        let memory: &'static mut [u64] = Box::leak(vec![0_u64; words].into_boxed_slice());
        let region = MmioRegion {
            virt: memory.as_ptr() as u64,
            size: size_of::<T>() as u64,
        };
        // SAFETY: zeroed, aligned memory of the block's size, leaked for
        // 'static; the registers are plain `u32` cells.
        (unsafe { &*memory.as_ptr().cast::<T>() }, region)
    }

    /// The bit of interrupt `intid` in a one-bit-per-interrupt register bank.
    fn bank_bit(intid: u32) -> (usize, FieldValue<u32, ()>) {
        (
            (intid / 32) as usize,
            FieldValue::<u32, ()>::new(1, (intid % 32) as usize, 1),
        )
    }

    fn config(regions: &[MmioRegion]) -> IrqChipConfig {
        let mut controller = Controller {
            kind: 0,
            region_count: u32::try_from(regions.len()).expect("at most MAX_REGIONS"),
            ..Controller::default()
        };
        controller.regions[..regions.len()].copy_from_slice(regions);
        let mut config = IrqChipConfig {
            controller_count: 1,
            ..IrqChipConfig::default()
        };
        config.controllers[0] = controller;
        config
    }

    /// A component bound to fresh distributor and CPU interface blocks.
    fn gic() -> (
        Gic400,
        &'static DistributorDevice,
        &'static CpuInterfaceDevice,
    ) {
        let (distributor, distributor_region) = device::<DistributorDevice>();
        let (cpu_interface, cpu_interface_region) = device::<CpuInterfaceDevice>();
        // SAFETY: the blocks are leaked, so they live for 'static.
        let gic = unsafe { Gic400::new(&config(&[distributor_region, cpu_interface_region])) }
            .expect("valid config");
        (gic, distributor, cpu_interface)
    }

    #[test]
    fn init_masks_everything_and_enables_forwarding() {
        let (_gic, distributor, cpu_interface) = gic();
        assert!(
            distributor
                .ICENABLER
                .iter()
                .all(|clear| clear.get() == u32::MAX)
        );
        // Without Security Extensions every interrupt is Group 0.
        assert!(distributor.IGROUPR.iter().all(|group| group.get() == 0));
        assert!(distributor.CTLR.is_set(GICD_CTLR::ENABLE_GRP0));
        assert!(cpu_interface.CTLR.is_set(GICC_CTLR::ENABLE));
        assert_eq!(cpu_interface.PMR.read(GICC_PMR::PRIORITY), 0xff);
    }

    #[test]
    fn a_missing_cpu_interface_region_is_rejected() {
        let (_distributor, region) = device::<DistributorDevice>();
        // SAFETY: the block is leaked.
        let result = unsafe { Gic400::new(&config(&[region])) };
        assert!(matches!(result, Err(ConfigError::BadRegion)));
    }

    #[test]
    fn claim_acknowledges_through_iar_and_filters_special_ids() {
        let (mut gic, _distributor, cpu_interface) = gic();
        cpu_interface.IAR.write(INTERRUPT_ID::ID.val(1023));
        assert_eq!(gic.claim(), SPURIOUS);
        cpu_interface.IAR.write(INTERRUPT_ID::ID.val(1022));
        assert_eq!(gic.claim(), SPURIOUS);
        cpu_interface.IAR.write(INTERRUPT_ID::ID.val(30));
        assert_eq!(gic.claim(), 30);
    }

    #[test]
    fn complete_writes_eoir() {
        let (mut gic, _distributor, cpu_interface) = gic();
        gic.complete(30);
        assert_eq!(cpu_interface.EOIR.read(INTERRUPT_ID::ID), 30);
        gic.complete(LINE_COUNT);
        assert_eq!(
            cpu_interface.EOIR.read(INTERRUPT_ID::ID),
            30,
            "special IDs are never ended"
        );
    }

    #[test]
    fn enabling_and_masking_use_the_set_and_clear_banks() {
        let (mut gic, distributor, _cpu_interface) = gic();
        let timer = Gic400::xlate(&[1, 14, 0xf08]);
        gic.set_enabled(timer, true);
        let (bank, bit) = bank_bit(timer);
        assert!(distributor.ISENABLER[bank].matches_all(bit));

        let uart = Gic400::xlate(&[0, 121, 4]);
        distributor.ICENABLER[(uart / 32) as usize].set(0);
        gic.set_enabled(uart, false);
        let (bank, bit) = bank_bit(uart);
        assert!(distributor.ICENABLER[bank].matches_all(bit));
    }

    #[test]
    fn trigger_configures_the_high_bit_of_the_line_field() {
        let (mut gic, distributor, _cpu_interface) = gic();
        let uart = Gic400::xlate(&[0, 121, 4]);
        // Two configuration bits per interrupt; bit 1 selects edge.
        let edge = FieldValue::<u32, ()>::new(1, ((uart % 16) * 2 + 1) as usize, 1);
        gic.set_trigger(uart, false).expect("SPI trigger");
        assert!(distributor.ICFGR[(uart / 16) as usize].matches_all(edge));
        gic.set_trigger(uart, true).expect("SPI trigger");
        assert!(!distributor.ICFGR[(uart / 16) as usize].matches_any(&[edge]));
        assert_eq!(gic.set_trigger(3, true), Err(ConfigError::BadLine));
    }

    #[test]
    fn xlate_follows_the_gic_binding() {
        // The RPi4 timer node: non-secure EL1 physical timer is PPI 14.
        assert_eq!(Gic400::xlate(&[1, 14, 0xf08]), 30);
        // The RPi4 PL011 is SPI 121.
        assert_eq!(Gic400::xlate(&[0, 121, 4]), 153);
        assert_eq!(Gic400::xlate(&[1, 16, 4]), INVALID_LINE);
        assert_eq!(Gic400::xlate(&[0, 988, 4]), INVALID_LINE);
        assert_eq!(Gic400::xlate(&[2, 0, 4]), INVALID_LINE);
        assert_eq!(Gic400::xlate(&[0]), INVALID_LINE);
    }
}
