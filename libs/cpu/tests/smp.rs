//! Host tests of the core affinity read from `MPIDR_EL1` (the pure part of `libcpu::smp`).
//! `libcpu` builds its smp module for `AArch64` hosts only.
#![cfg(target_arch = "aarch64")]

#[cfg(test)]
mod tests {
    use vesper_cpu::smp::{AFFINITY_MASK, CoreAffinity};

    #[test]
    fn the_mask_covers_the_four_affinity_levels_only() {
        // Aff3 in bits 39:32, Aff2..Aff0 in bits 23:0.
        assert_eq!(AFFINITY_MASK, 0x0000_00ff_00ff_ffff);
    }

    #[test]
    fn a_cortex_a53_core_is_named_by_aff0() {
        // Raspberry Pi 3 core 2: RES1 bit 31 set, Aff0 = 2.
        assert_eq!(CoreAffinity::from_mpidr(0x8000_0002).bits(), 0x2);
    }

    #[test]
    fn a_cortex_a76_core_is_named_by_aff1() {
        // Raspberry Pi 5 core 3: RES1 and MT set, Aff1 = 3, Aff0 = 0.
        let core_three = CoreAffinity::from_mpidr(0x8100_0300);
        assert_eq!(core_three.bits(), 0x300);
        assert_ne!(core_three, CoreAffinity::from_mpidr(0x8100_0000));
    }

    #[test]
    fn the_boot_cores_of_all_boards_have_affinity_zero() {
        for boot_core_mpidr in [0x8000_0000, 0x8100_0000] {
            assert_eq!(CoreAffinity::from_mpidr(boot_core_mpidr).bits(), 0);
        }
    }

    #[test]
    fn aff3_is_kept() {
        assert_eq!(
            CoreAffinity::from_mpidr(0x0000_0001_8000_0000).bits(),
            0x0000_0001_0000_0000
        );
    }
}
