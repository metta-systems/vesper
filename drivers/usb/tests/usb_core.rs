//! Host tests of the USB core's device bookkeeping.

#[cfg(test)]
mod tests {
    use {
        usb_core::devices::{AddressPool, Attachment, Device, DeviceTable, MAX_DEVICES},
        vesper_usb_protocol::enumeration::{FIRST_ADDRESS, LAST_ADDRESS, Speed},
    };

    fn device(address: u8, attachment: Attachment) -> Device {
        Device {
            address,
            speed: Speed::Full,
            attachment,
            class_code: 0,
            vendor: 0x0424,
            product: 0x9514,
        }
    }

    #[test]
    fn addresses_are_handed_out_lowest_first_and_come_back() {
        let mut pool = AddressPool::new();
        assert_eq!(pool.take(), Some(FIRST_ADDRESS));
        assert_eq!(pool.take(), Some(2));
        assert_eq!(pool.take(), Some(3));
        pool.release(2);
        assert_eq!(pool.take(), Some(2), "the lowest free address again");
        assert_eq!(pool.take(), Some(4));
    }

    #[test]
    fn the_pool_runs_out_after_the_last_address_and_ignores_strays() {
        let mut pool = AddressPool::new();
        for expected in FIRST_ADDRESS..=LAST_ADDRESS {
            assert_eq!(pool.take(), Some(expected));
        }
        assert_eq!(pool.take(), None);
        pool.release(0);
        pool.release(128);
        pool.release(255);
        assert_eq!(pool.take(), None, "addresses outside the range are ignored");
        pool.release(100);
        assert_eq!(pool.take(), Some(100));
        // Releasing an address twice frees it once.
        pool.release(7);
        pool.release(7);
        assert_eq!(pool.take(), Some(7));
        assert_eq!(pool.take(), None);
    }

    #[test]
    fn devices_are_found_by_address_and_by_port() {
        let mut table = DeviceTable::new();
        assert!(table.is_empty());
        assert!(table.add(device(1, Attachment::Root)));
        assert!(table.add(device(2, Attachment::Hub { hub: 1, port: 3 })));
        assert_eq!(table.len(), 2);
        assert_eq!(table.find(2).map(|d| d.address), Some(2));
        assert_eq!(table.on_port(1, 3).map(|d| d.address), Some(2));
        assert!(table.on_port(1, 2).is_none());
        assert!(table.find(9).is_none());
    }

    #[test]
    fn an_address_is_held_by_one_device_and_the_table_has_a_limit() {
        let mut table = DeviceTable::new();
        assert!(table.add(device(1, Attachment::Root)));
        assert!(!table.add(device(1, Attachment::Hub { hub: 1, port: 1 })));
        for address in 2..=u8::try_from(MAX_DEVICES).unwrap() {
            assert!(table.add(device(
                address,
                Attachment::Hub {
                    hub: 1,
                    port: address
                }
            )));
        }
        assert!(!table.add(device(100, Attachment::Hub { hub: 1, port: 99 })));
        assert_eq!(table.len(), MAX_DEVICES);
    }

    #[test]
    fn removing_a_port_forgets_the_device_and_everything_behind_it() {
        let mut table = DeviceTable::new();
        table.add(device(1, Attachment::Root));
        table.add(device(2, Attachment::Hub { hub: 1, port: 1 }));
        table.add(device(3, Attachment::Hub { hub: 1, port: 2 }));
        // Device 3 is a hub with devices on its ports.
        table.add(device(4, Attachment::Hub { hub: 3, port: 1 }));
        table.add(device(5, Attachment::Hub { hub: 3, port: 2 }));
        let removed = table.remove_port(1, 2);
        let mut addresses = removed.addresses().to_vec();
        addresses.sort_unstable();
        assert_eq!(addresses, [3, 4, 5]);
        assert_eq!(table.len(), 2);
        assert!(table.find(2).is_some() && table.find(1).is_some());
        // An empty port removes nothing.
        assert!(table.remove_port(1, 2).addresses().is_empty());
        assert!(table.remove_port(7, 1).addresses().is_empty());
    }
}
