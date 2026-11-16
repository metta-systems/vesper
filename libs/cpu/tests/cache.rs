//! Host tests of the cache set/way operands (the pure part of
//! `libcpu::cache`; the maintenance instructions themselves run on the
//! boards only). `libcpu` builds its cache module for `AArch64` hosts only.
#![cfg(target_arch = "aarch64")]

#[cfg(test)]
mod tests {
    use vesper_cpu::cache::CacheGeometry;

    #[test]
    fn decodes_ccsidr_fields() {
        // A Cortex-A72 L1 data cache: 32 KiB, 64-byte lines, 2-way, 256 sets.
        assert_eq!(
            CacheGeometry::from_fields(2, 1, 255),
            CacheGeometry {
                line_shift: 6,
                ways: 2,
                sets: 256
            }
        );
    }

    #[test]
    fn places_way_set_and_level_as_the_arm_arm_lays_them_out() {
        // A Cortex-A76 L1 data cache: 64 KiB, 64-byte lines, 4-way, 256 sets:
        // the way in bits 31:30, the set from bit 6.
        let l1 = CacheGeometry::from_fields(2, 3, 255);
        assert_eq!(l1.set_way(0, 0, 0), 0);
        assert_eq!(l1.set_way(0, 1, 0), 1 << 6);
        assert_eq!(l1.set_way(0, 255, 3), (3 << 30) | (255 << 6));
        // The level less one in bits 3:1: L2 is 1.
        let l2 = CacheGeometry::from_fields(2, 7, 1023);
        assert_eq!(l2.set_way(1, 1023, 7), (7 << 29) | (1023 << 6) | (1 << 1));
    }

    #[test]
    fn rounds_an_odd_associativity_up_to_its_way_bits() {
        // A 3-way cache needs two way bits, from bit 30.
        let three_way = CacheGeometry::from_fields(2, 2, 63);
        assert_eq!(three_way.set_way(0, 0, 2), 2 << 30);
    }

    #[test]
    fn a_direct_mapped_cache_has_no_way_bits() {
        let direct = CacheGeometry::from_fields(2, 0, 511);
        assert_eq!(direct.set_way(2, 511, 0), (511 << 6) | (2 << 1));
    }
}
