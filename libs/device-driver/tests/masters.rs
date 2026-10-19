//! Host tests of the exporter's master-key bookkeeping. The key operations
//! themselves are system calls, exercised end to end by `test-usb`.

#[cfg(test)]
mod tests {
    use {
        libobject::{AddressSpaceKey, KeyTableKey, RawKey},
        vesper_device_driver::{ExportedArea, InstallError, MAX_AREAS, Masters},
    };

    fn masters() -> Masters<4> {
        Masters::new(
            AddressSpaceKey::from_key(RawKey::from_wire(0x102)),
            KeyTableKey::from_key(RawKey::from_wire(0x104)),
            40,
            [0x1000, 0x2000, 0x3000, 0x4000],
        )
    }

    #[test]
    fn masters_take_one_slot_per_client_and_function() {
        assert_eq!(Masters::<4>::SLOTS, u32::try_from(MAX_AREAS * 4).unwrap());
        let masters = masters();
        assert_eq!(masters.master_slot(0, 0), 40);
        assert_eq!(masters.master_slot(0, 3), 43);
        assert_eq!(masters.master_slot(1, 0), 44);
        let last = masters.master_slot(MAX_AREAS - 1, 3);
        assert_eq!(last, 40 + Masters::<4>::SLOTS - 1);
    }

    #[test]
    fn install_refuses_clients_and_slots_out_of_range_before_any_key_operation() {
        let mut masters = masters();
        let area = ExportedArea::default();
        let table = KeyTableKey::from_key(RawKey::from_wire(0x106));
        assert!(matches!(
            masters.install(MAX_AREAS, &area, &table, 8, 1),
            Err(InstallError::Unknown)
        ));
        assert!(matches!(
            masters.install(0, &area, &table, u32::MAX - 2, 1),
            Err(InstallError::Unknown)
        ));
    }
}
