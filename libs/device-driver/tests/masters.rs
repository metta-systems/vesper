//! Host tests of the exporter's master-key bookkeeping. The key operations
//! themselves are system calls, exercised end to end by `test-usb`.

#[cfg(test)]
mod tests {
    use {
        libobject::{AddressSpaceKey, KeyTableKey, RawKey, StackPoolHandle},
        vesper_device_driver::{InstallError, Masters},
    };

    fn masters() -> Masters<4> {
        Masters::new(
            AddressSpaceKey::from_key(RawKey::from_wire(0x102)),
            KeyTableKey::from_key(RawKey::from_wire(0x104)),
            StackPoolHandle::new(0, 1),
            40,
            [0x1000, 0x2000, 0x3000, 0x4000],
        )
    }

    #[test]
    fn masters_take_one_slot_per_function() {
        assert_eq!(Masters::<4>::SLOTS, 4);
        let masters = masters();
        assert_eq!(masters.master_slot(0), 40);
        assert_eq!(masters.master_slot(3), 43);
    }

    #[test]
    fn install_refuses_slots_out_of_range_before_any_key_operation() {
        let mut masters = masters();
        let table = KeyTableKey::from_key(RawKey::from_wire(0x106));
        assert!(matches!(
            masters.install(&table, u32::MAX - 2, 1),
            Err(InstallError::Unknown)
        ));
    }
}
