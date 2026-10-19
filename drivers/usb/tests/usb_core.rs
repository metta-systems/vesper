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

#[cfg(test)]
mod claim_tests {
    use {
        usb_core::claims::{
            ClaimError, Claims, HubRoute, MAX_CLAIMS, Offer, PipeParams, UseError, offers_of,
        },
        vesper_usb_protocol::enumeration::Speed,
    };

    const OWNER: u64 = 0x2000_0000;
    const OTHER: u64 = 0x2000_1000;

    /// A configuration with a boot keyboard (interface 0, interrupt-IN 0x81)
    /// and a boot mouse (interface 1, interrupt-IN 0x82 and an alternate
    /// setting 1 that is not offered).
    fn composite() -> [u8; 9 + 9 + 7 + 9 + 7 + 9 + 7] {
        let mut bytes = [0; 9 + 9 + 7 + 9 + 7 + 9 + 7];
        let parts: [&[u8]; 7] = [
            &[9, 2, 0, 0, 2, 1, 0, 0x80, 50],
            &[9, 4, 0, 0, 1, 3, 1, 1, 0],
            &[7, 5, 0x81, 3, 8, 0, 10],
            &[9, 4, 1, 0, 1, 3, 1, 2, 0],
            &[7, 5, 0x82, 3, 4, 0, 8],
            &[9, 4, 1, 1, 1, 3, 1, 2, 0],
            &[7, 5, 0x83, 3, 16, 0, 1],
        ];
        let mut at = 0;
        for part in parts {
            bytes[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }
        bytes
    }

    fn offers(device: u8, route: Option<HubRoute>) -> impl Iterator<Item = Offer> {
        offers_of(device, Speed::Low, route, &[]).chain(core::iter::empty())
    }

    fn filled() -> Claims {
        let mut claims = Claims::new();
        let configuration = composite();
        for offer in offers_of(
            3,
            Speed::Low,
            Some(HubRoute { hub: 1, port: 2 }),
            &configuration,
        ) {
            assert!(claims.offer(offer));
        }
        claims
    }

    #[test]
    fn the_interfaces_of_a_configuration_are_offered_with_their_interrupt_endpoint() {
        let configuration = composite();
        let found: Vec<Offer> = offers_of(3, Speed::Low, None, &configuration).collect();
        assert_eq!(found.len(), 2, "the alternate setting is not offered");
        assert_eq!((found[0].number, found[0].protocol), (0, 1));
        assert_eq!(
            found[0]
                .interrupt_in
                .map(|e| (e.address, e.max_packet, e.interval)),
            Some((0x81, 8, 10))
        );
        assert_eq!((found[1].number, found[1].protocol), (1, 2));
        assert_eq!(found[1].interrupt_in.map(|e| e.address), Some(0x82));
        assert_eq!(
            offers(3, None).count(),
            0,
            "an empty configuration offers nothing"
        );
    }

    #[test]
    fn claiming_waits_for_a_match_and_takes_each_interface_once() {
        let mut claims = Claims::new();
        assert_eq!(claims.claim(OWNER, 3, 1, 1), Err(ClaimError::NotReady));
        let mut claims = filled();
        assert_eq!(claims.claim(OWNER, 8, 6, 80), Err(ClaimError::NotReady));
        let keyboard = claims.claim(OWNER, 3, 1, 1).expect("keyboard");
        assert_eq!(claims.claim(OTHER, 3, 1, 1), Err(ClaimError::NotReady));
        let mouse = claims.claim(OTHER, 3, 1, 2).expect("mouse");
        assert_ne!(keyboard, mouse);
    }

    #[test]
    fn a_claim_is_its_owners_alone() {
        let mut claims = filled();
        let handle = claims.claim(OWNER, 3, 1, 1).expect("claim");
        assert_eq!(claims.release(OTHER, handle), Err(UseError::NoClaim));
        assert_eq!(claims.interrupt_pipe(OTHER, handle), Err(UseError::NoClaim));
        assert_eq!(claims.check(OWNER, handle), Ok(()));
        assert_eq!(claims.check(OTHER, handle), Err(UseError::NoClaim));
    }

    #[test]
    fn the_pipe_parameters_describe_the_endpoint_and_its_route() {
        let mut claims = filled();
        let handle = claims.claim(OWNER, 3, 1, 2).expect("claim");
        let params = claims.interrupt_pipe(OWNER, handle).expect("params");
        assert_eq!(
            params,
            PipeParams {
                device: 3,
                endpoint: 0x82,
                speed: Speed::Low,
                interval: 8,
                max_packet: 4,
                route: Some(HubRoute { hub: 1, port: 2 }),
            }
        );
        assert_eq!(PipeParams::from_bytes(&params.to_bytes()), Some(params));
        assert_eq!(PipeParams::from_bytes(&[0; 4]), None);
        let mut bad = params.to_bytes();
        bad[2] = 9;
        assert_eq!(PipeParams::from_bytes(&bad), None);
    }

    #[test]
    fn releasing_offers_the_interface_again_and_stales_the_handle() {
        let mut claims = filled();
        let first = claims.claim(OWNER, 3, 1, 1).expect("claim");
        assert_eq!(claims.release(OWNER, first), Ok(()));
        assert_eq!(claims.release(OWNER, first), Err(UseError::NoClaim));
        let second = claims.claim(OTHER, 3, 1, 1).expect("claimed again");
        assert_ne!(first, second, "a new generation");
        assert_eq!(claims.interrupt_pipe(OWNER, first), Err(UseError::NoClaim));
    }

    #[test]
    fn unplugging_a_device_ends_its_claims_and_offers() {
        let mut claims = filled();
        let handle = claims.claim(OWNER, 3, 1, 1).expect("claim");
        claims.withdraw(3);
        assert_eq!(
            claims.interrupt_pipe(OWNER, handle),
            Err(UseError::DeviceGone)
        );
        assert_eq!(claims.claim(OWNER, 3, 1, 2), Err(ClaimError::NotReady));
        assert_eq!(claims.release(OWNER, handle), Ok(()));
        assert_eq!(claims.interrupt_pipe(OWNER, handle), Err(UseError::NoClaim));
    }

    #[test]
    fn the_claim_table_has_a_limit() {
        let mut claims = Claims::new();
        for number in 0..=MAX_CLAIMS {
            let mut offer = offers_of(1, Speed::Full, None, &composite())
                .next()
                .expect("offer");
            offer.number = u8::try_from(number).expect("small");
            assert!(claims.offer(offer));
        }
        for _ in 0..MAX_CLAIMS {
            claims.claim(OWNER, 3, 1, 1).expect("room");
        }
        assert_eq!(claims.claim(OWNER, 3, 1, 1), Err(ClaimError::Full));
    }
}
