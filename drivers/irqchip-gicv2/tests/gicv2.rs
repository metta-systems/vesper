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
        derive_mmio::Mmio,
        irqchip_gicv2::{ConfigError, Gic400, IPI_LINE, IPI_SGI, LINE_COUNT},
        libirqchip::{
            Controller, INVALID_LINE, IrqChipConfig, MAX_CPUS, MmioRegion, SPURIOUS, TRIGGER_EDGE,
            TRIGGER_FIXED, TRIGGER_LEVEL,
        },
    };

    /// `GICD_CTLR` Group 0 enable (no Security Extensions view).
    const GICD_CTLR_ENABLE_GRP0: u32 = 1 << 0;
    /// `GICC_CTLR` enable.
    const GICC_CTLR_ENABLE: u32 = 1 << 0;
    /// The interrupt ID field of `GICC_IAR` / `GICC_EOIR`.
    const INTERRUPT_ID_MASK: u32 = 0x3ff;
    /// The priority field of `GICC_PMR`.
    const PRIORITY_MASK: u32 = 0xff;

    /// The `GICv2` distributor as the hardware sees it.
    #[derive(Mmio)]
    #[mmio(no_owned)]
    #[repr(C)]
    struct DistributorDevice {
        control: u32,
        typer: u32,
        _iidr: [u32; 30],
        group: [u32; 32],
        set_enable: [u32; 32],
        clear_enable: [u32; 32],
        _pending_and_active: [u32; 128],
        _priorities: [u32; 256],
        targets: [u32; 256],
        configuration: [u32; 64],
        _reserved: [u32; 128],
        sgir: u32,
        _padding: u32,
    }

    /// The `GICv2` CPU interface as the hardware sees it.
    #[derive(Mmio)]
    #[mmio(no_owned)]
    #[repr(C)]
    struct CpuInterfaceDevice {
        control: u32,
        priority_mask: u32,
        _bpr: u32,
        interrupt_acknowledge: u32,
        end_of_interrupt: u32,
        _status: [u32; 1019],
        _dir: u32,
        _padding: u32,
    }

    // The device views must cover exactly `arm-gic`'s blocks.
    const _: () = assert!(size_of::<DistributorDevice>() == size_of::<Gicd>());
    const _: () = assert!(size_of::<CpuInterfaceDevice>() == size_of::<Gicc>());

    /// Zeroed, leaked, 8-aligned backing memory for register block `T`.
    fn device<T>() -> (*mut T, MmioRegion) {
        let words = size_of::<T>().div_ceil(size_of::<u64>());
        let memory: &'static mut [u64] = Box::leak(vec![0_u64; words].into_boxed_slice());
        let region = MmioRegion {
            virt: memory.as_ptr() as u64,
            size: size_of::<T>() as u64,
        };
        (memory.as_mut_ptr().cast::<T>(), region)
    }

    fn distributor_device() -> (MmioDistributorDevice<'static>, MmioRegion) {
        let (block, region) = device::<DistributorDevice>();
        // SAFETY: zeroed, aligned memory of the block's size, leaked for
        // 'static.
        (unsafe { DistributorDevice::new_mmio(block) }, region)
    }

    fn cpu_interface_device() -> (MmioCpuInterfaceDevice<'static>, MmioRegion) {
        let (block, region) = device::<CpuInterfaceDevice>();
        // SAFETY: as in `distributor_device`.
        (unsafe { CpuInterfaceDevice::new_mmio(block) }, region)
    }

    /// The bit of interrupt `intid` in a one-bit-per-interrupt register bank.
    const fn bank_bit(intid: u32) -> (usize, u32) {
        ((intid / 32) as usize, 1 << (intid % 32))
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
        MmioDistributorDevice<'static>,
        MmioCpuInterfaceDevice<'static>,
    ) {
        let (distributor, distributor_region) = distributor_device();
        let (cpu_interface, cpu_interface_region) = cpu_interface_device();
        // SAFETY: the blocks are leaked, so they live for 'static.
        let gic = unsafe { Gic400::new(&config(&[distributor_region, cpu_interface_region])) }
            .expect("valid config");
        (gic, distributor, cpu_interface)
    }

    /// Every word of a register bank, by reading through `read`.
    fn bank(len: usize, read: impl Fn(usize) -> u32) -> Vec<u32> {
        (0..len).map(read).collect()
    }

    #[test]
    fn init_masks_everything_and_enables_forwarding() {
        let (_gic, distributor, cpu_interface) = gic();
        let clear_enable = bank(distributor.len_clear_enable(), |index| {
            distributor.read_clear_enable(index).unwrap()
        });
        assert!(clear_enable.iter().all(|&clear| clear == u32::MAX));
        // Without Security Extensions every interrupt is Group 0.
        let group = bank(distributor.len_group(), |index| {
            distributor.read_group(index).unwrap()
        });
        assert!(group.iter().all(|&group| group == 0));
        assert_ne!(distributor.read_control() & GICD_CTLR_ENABLE_GRP0, 0);
        assert_ne!(cpu_interface.read_control() & GICC_CTLR_ENABLE, 0);
        assert_eq!(cpu_interface.read_priority_mask() & PRIORITY_MASK, 0xff);
    }

    #[test]
    fn init_routes_every_spi_to_the_booting_cpu() {
        let (mut distributor, distributor_region) = distributor_device();
        let (_cpu_interface, cpu_interface_region) = cpu_interface_device();
        // 64 lines (ITLinesNumber 1); this core is CPU interface 2, as the
        // banked SGI/PPI targets report.
        distributor.write_typer(1);
        distributor.write_targets(0, 0x0404_0404).unwrap();
        // SAFETY: the blocks are leaked, so they live for 'static.
        let _gic = unsafe { Gic400::new(&config(&[distributor_region, cpu_interface_region])) }
            .expect("valid config");
        // INTIDs 32-63: words 8-15, one target byte each.
        assert!((8..16).all(|index| distributor.read_targets(index).unwrap() == 0x0404_0404));
        assert_eq!(distributor.read_targets(16).unwrap(), 0, "no line above 63");
    }

    #[test]
    fn a_uniprocessor_gic_gets_no_targets() {
        let (_gic, distributor, _cpu_interface) = gic();
        let targets = bank(distributor.len_targets(), |index| {
            distributor.read_targets(index).unwrap()
        });
        assert!(targets.iter().all(|&targets| targets == 0));
    }

    #[test]
    fn a_missing_cpu_interface_region_is_rejected() {
        let (_distributor, region) = distributor_device();
        // SAFETY: the block is leaked.
        let result = unsafe { Gic400::new(&config(&[region])) };
        assert!(matches!(result, Err(ConfigError::BadRegion)));
    }

    #[test]
    fn claim_acknowledges_through_iar_and_filters_special_ids() {
        let (mut gic, _distributor, mut cpu_interface) = gic();
        cpu_interface.write_interrupt_acknowledge(1023);
        assert_eq!(gic.claim(), SPURIOUS);
        cpu_interface.write_interrupt_acknowledge(1022);
        assert_eq!(gic.claim(), SPURIOUS);
        cpu_interface.write_interrupt_acknowledge(30);
        assert_eq!(gic.claim(), 30);
    }

    #[test]
    fn complete_writes_eoir() {
        let (mut gic, _distributor, cpu_interface) = gic();
        gic.complete(30);
        assert_eq!(
            cpu_interface.read_end_of_interrupt() & INTERRUPT_ID_MASK,
            30
        );
        gic.complete(LINE_COUNT);
        assert_eq!(
            cpu_interface.read_end_of_interrupt() & INTERRUPT_ID_MASK,
            30,
            "special IDs are never ended"
        );
    }

    #[test]
    fn enabling_and_masking_use_the_set_and_clear_banks() {
        let (mut gic, mut distributor, _cpu_interface) = gic();
        let timer = Gic400::xlate(&[1, 14, 0xf08]);
        gic.set_enabled(timer, true);
        let (index, bit) = bank_bit(timer);
        assert_ne!(distributor.read_set_enable(index).unwrap() & bit, 0);

        let uart = Gic400::xlate(&[0, 121, 4]);
        let (index, bit) = bank_bit(uart);
        distributor.write_clear_enable(index, 0).unwrap();
        gic.set_enabled(uart, false);
        assert_ne!(distributor.read_clear_enable(index).unwrap() & bit, 0);
    }

    #[test]
    fn trigger_configures_the_high_bit_of_the_line_field() {
        let (mut gic, distributor, _cpu_interface) = gic();
        let uart = Gic400::xlate(&[0, 121, 4]);
        // Two configuration bits per interrupt; bit 1 selects edge.
        let edge = 1_u32 << ((uart % 16) * 2 + 1);
        let index = (uart / 16) as usize;
        gic.set_trigger(uart, false).expect("SPI trigger");
        assert_ne!(distributor.read_configuration(index).unwrap() & edge, 0);
        gic.set_trigger(uart, true).expect("SPI trigger");
        assert_eq!(distributor.read_configuration(index).unwrap() & edge, 0);
        assert_eq!(gic.set_trigger(3, true), Err(ConfigError::BadLine));
    }

    /// `GICD_SGIR`: the target list filter (bits 25:24, 0 = the CPU target
    /// list), the target list (bits 23:16), `NSATT` (bit 15) and the SGI's
    /// INTID (bits 3:0).
    const fn sgir(target_list: u32, intid: u32) -> u32 {
        (target_list << 16) | (1 << 15) | intid
    }

    /// Present this core as CPU interface `interface` in the banked SGI/PPI
    /// target bytes, as a core of a multiprocessor GIC reads them.
    fn be_cpu_interface(distributor: &mut MmioDistributorDevice<'static>, interface: u8) {
        let byte = u32::from(interface);
        distributor
            .write_targets(0, byte | byte << 8 | byte << 16 | byte << 24)
            .unwrap();
    }

    #[test]
    fn init_cpu_records_the_banked_interface_and_enables_the_ipi_sgi() {
        let (mut gic, mut distributor, mut cpu_interface) = gic();
        cpu_interface.write_control(0);
        cpu_interface.write_priority_mask(0);
        be_cpu_interface(&mut distributor, 1 << 2);
        gic.init_cpu(1).expect("a CPU interface");
        let (index, bit) = bank_bit(IPI_LINE);
        assert_ne!(distributor.read_set_enable(index).unwrap() & bit, 0);
        assert_ne!(cpu_interface.read_control() & GICC_CTLR_ENABLE, 0);
        assert_eq!(cpu_interface.read_priority_mask() & PRIORITY_MASK, 0xff);
        // The recorded interface is the one an IPI to CPU 1 targets.
        gic.send_ipi(1 << 1).expect("CPU 1 is known");
        assert_eq!(distributor.read_sgir(), sgir(1 << 2, IPI_SGI));
    }

    #[test]
    fn init_cpu_without_an_interface_or_beyond_max_cpus_is_refused() {
        let (mut gic, mut distributor, _cpu_interface) = gic();
        assert_eq!(gic.init_cpu(0), Err(ConfigError::NoCpuInterface));
        be_cpu_interface(&mut distributor, 1);
        assert_eq!(
            gic.init_cpu(u32::try_from(MAX_CPUS).unwrap()),
            Err(ConfigError::BadCpu)
        );
    }

    #[test]
    fn send_ipi_targets_every_named_cpu_in_one_sgir_write() {
        let (mut gic, mut distributor, _cpu_interface) = gic();
        // Logical CPUs 0..3 on CPU interfaces 3, 2, 1, 0: the mask names logical CPUs.
        for (cpu, interface) in [(0, 3), (1, 2), (2, 1), (3, 0)] {
            be_cpu_interface(&mut distributor, 1 << interface);
            gic.init_cpu(cpu).expect("a CPU interface");
        }
        gic.send_ipi(0b0101).expect("CPUs 0 and 2 are known");
        assert_eq!(distributor.read_sgir(), sgir((1 << 3) | (1 << 1), IPI_SGI));
    }

    #[test]
    fn send_ipi_to_an_unknown_cpu_signals_nobody() {
        let (mut gic, mut distributor, _cpu_interface) = gic();
        be_cpu_interface(&mut distributor, 1);
        gic.init_cpu(0).expect("a CPU interface");
        assert_eq!(gic.send_ipi(0b11), Err(ConfigError::UnknownCpu));
        assert_eq!(gic.send_ipi(1 << MAX_CPUS), Err(ConfigError::UnknownCpu));
        assert_eq!(distributor.read_sgir(), 0);
        assert_eq!(gic.send_ipi(0), Ok(()));
        assert_eq!(distributor.read_sgir(), 0, "an empty mask writes nothing");
    }

    #[test]
    fn a_claimed_sgi_is_ended_at_once_with_its_source_cpu() {
        let (mut gic, _distributor, mut cpu_interface) = gic();
        // SGI 0 from CPU interface 2: the source CPU is GICC_IAR bits 12:10.
        let acknowledged = (2 << 10) | IPI_SGI;
        cpu_interface.write_interrupt_acknowledge(acknowledged);
        assert_eq!(gic.claim(), IPI_LINE);
        assert_eq!(cpu_interface.read_end_of_interrupt(), acknowledged);
        // Its complete writes nothing more.
        cpu_interface.write_end_of_interrupt(0);
        gic.complete(IPI_LINE);
        assert_eq!(cpu_interface.read_end_of_interrupt(), 0);
        // Any other line is ended by its complete only.
        cpu_interface.write_interrupt_acknowledge(40);
        assert_eq!(gic.claim(), 40);
        assert_eq!(cpu_interface.read_end_of_interrupt(), 0);
        gic.complete(40);
        assert_eq!(cpu_interface.read_end_of_interrupt(), 40);
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

    #[test]
    fn trigger_follows_the_specifier_flags() {
        // The RPi4 timer PPIs are level-low with a CPU mask (0xf08).
        assert_eq!(Gic400::trigger(&[1, 11, 0xf08]), TRIGGER_LEVEL);
        assert_eq!(Gic400::trigger(&[0, 121, 4]), TRIGGER_LEVEL);
        assert_eq!(Gic400::trigger(&[0, 121, 1]), TRIGGER_EDGE);
        assert_eq!(Gic400::trigger(&[0, 121, 2]), TRIGGER_EDGE);
        assert_eq!(Gic400::trigger(&[0, 121, 0]), TRIGGER_FIXED);
        assert_eq!(Gic400::trigger(&[0, 121]), TRIGGER_FIXED);
    }
}
