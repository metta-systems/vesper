//! Host tests against the real Raspberry Pi device trees in `targets/`.

#[cfg(test)]
mod tests {
    use vesper_devicetree::{
        BrcmPin, Cells, DevTreeIndex, DeviceTree, GpioSpecifier, PropReader, Region,
        brcm_dma_channel_mask, brcm_pins, bus_cells, dma_address, dump, find_compatible,
        generic_function_code, gpio_pins, gpio_specifier, interrupt_cells, interrupt_parent,
        interrupt_specifier, is_compatible, is_enabled, is_interrupt_controller, linked_phandle,
        node_by_phandle, phandle, property, reg, reg_cells, regions, translate, write_path,
    };

    /// A DTB blob copied into u32-aligned storage, as `DevTree` requires.
    struct AlignedBlob {
        words: Vec<u32>,
        length: usize,
    }

    impl AlignedBlob {
        fn new(bytes: &[u8]) -> Self {
            let mut words = vec![0_u32; bytes.len().div_ceil(size_of::<u32>())];
            // SAFETY: `words` spans at least `bytes.len()` bytes.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    words.as_mut_ptr().cast(),
                    bytes.len(),
                );
            }
            Self {
                words,
                length: bytes.len(),
            }
        }

        /// Exactly the DTB bytes: `DevTree::new` rejects a buffer longer than `totalsize`.
        fn bytes(&self) -> &[u8] {
            // SAFETY: reinterpreting initialized u32 storage as bytes, within its length.
            unsafe { core::slice::from_raw_parts(self.words.as_ptr().cast(), self.length) }
        }
    }

    /// Index `bytes` the way kickstart does and run `check` on the tree.
    fn with_tree(bytes: &[u8], check: impl FnOnce(&DeviceTree<'_, '_>)) {
        let blob = AlignedBlob::new(bytes);
        // SAFETY: the blob is u32-aligned, complete, and outlives the tree.
        let raw = unsafe { DeviceTree::blob_from_raw(blob.bytes().as_ptr()) }.expect("valid DTB");
        let layout = DeviceTree::index_layout(&raw).expect("index layout");
        let mut buffer = vec![0_u8; layout.size() + layout.align()];
        let tree = DeviceTree::new(raw, &mut buffer).expect("index");
        assert_eq!(tree.total_size(), bytes.len());
        check(&tree);
    }

    /// Like [`with_tree`], for the node-level helpers.
    fn with_index(bytes: &[u8], check: impl FnOnce(&DevTreeIndex<'_, '_>)) {
        with_tree(bytes, |tree| check(tree.index()));
    }

    const RPI3: &[u8] = include_bytes!("../../../targets/bcm2710-rpi-3-b-plus.dtb");
    const RPI4: &[u8] = include_bytes!("../../../targets/bcm2711-rpi-4-b.dtb");
    /// The Pi 4 tree the SD image carries for the firmware.
    const RPI4_FIRMWARE: &[u8] = include_bytes!("../../../tools/makesd/boot/bcm2711-rpi-4-b.dtb");
    const RPI5: &[u8] = include_bytes!("../../../targets/bcm2712-rpi-5-b.dtb");
    /// The Pi 4 tree with the MSP2202 SPI panel on SPI0 CE0 (`just spi-panel-dtb`).
    const SPI_PANEL: &[u8] = include_bytes!("../../../targets/spi-panel-test.dtb");

    mod pci {
        use {
            super::{RPI4, RPI4_FIRMWARE, RPI5, with_index},
            vesper_devicetree::{
                dma_address, find_compatible, has_path,
                pci::{
                    PciRange, PciSpace, Reset, brcm_msi_offset, host_bridge_of, is_host_bridge,
                    msi_parent, msi_range, pci_dma_ranges, pci_function, pci_ranges, resets,
                },
                phandle, primecell_periphid,
            },
        };

        #[test]
        fn rpi4_bridge_windows_and_its_own_msi_controller() {
            with_index(RPI4, |index| {
                let (bridge, _) = find_compatible(index, &["brcm,bcm2711-pcie"]).expect("pcie0");
                // `/scb` maps 0x6_0000_0000 one to one.
                assert_eq!(
                    pci_ranges(&bridge).collect::<Vec<_>>(),
                    [PciRange {
                        space: PciSpace::Memory32,
                        prefetchable: false,
                        pci: 0xc000_0000,
                        cpu: 0x6_0000_0000,
                        size: 0x4000_0000,
                    }]
                );
                // The first 3 GiB of RAM, through `/scb`'s one-to-one
                // `dma-ranges`.
                assert_eq!(
                    pci_dma_ranges(&bridge).collect::<Vec<_>>(),
                    [PciRange {
                        space: PciSpace::Memory32,
                        prefetchable: false,
                        pci: 0,
                        cpu: 0,
                        size: 0xc000_0000,
                    }]
                );
                assert!(is_host_bridge(&bridge));
                // The root port below it is a PCI node, not a host bridge.
                let root_port = index
                    .nodes()
                    .find(|node| node.name() == Ok("pci@1,0"))
                    .expect("root port");
                assert!(!is_host_bridge(&root_port));
                let parent = msi_parent(index, &bridge).expect("msi-parent");
                assert_eq!(phandle(&parent), phandle(&bridge));
                assert_eq!(msi_range(index, &parent), None);
                assert_eq!(resets(index, &bridge).count(), 0);
            });
        }

        #[test]
        fn rpi4_firmware_vl805_node_is_function_one_zero_zero() {
            // Its `reg` is all zero: the bus comes from the bridge above it.
            with_index(RPI4_FIRMWARE, |index| {
                let vl805 = index
                    .nodes()
                    .find(|node| has_path(node, "/scb/pcie@7d500000/pci@0,0/usb@0,0"))
                    .expect("usb@0,0");
                assert_eq!(pci_function(&vl805), Some(0x100));
                let root_port = vl805.parent().expect("pci@0,0");
                assert_eq!(pci_function(&root_port), Some(0));
            });
        }

        #[test]
        fn rpi4_vl805_node_by_path_and_its_function() {
            with_index(RPI4, |index| {
                const PATH: &str = "/scb/pcie@7d500000/pci@1,0/usb@1,0";
                let matching: Vec<_> = index.nodes().filter(|node| has_path(node, PATH)).collect();
                assert_eq!(matching.len(), 1, "one node has the path");
                let vl805 = &matching[0];
                // The same name elsewhere, or a path missing a step, is no match.
                assert!(!has_path(vl805, "/pcie@7d500000/pci@1,0/usb@1,0"));
                assert!(!has_path(vl805, "usb@1,0"));
                // `reg` 0x10000: bus 1, device 0, function 0.
                assert_eq!(pci_function(vl805), Some(0x100));
                let bridge = host_bridge_of(vl805).expect("below a host bridge");
                assert_eq!(bridge.name(), Ok("pcie@7d500000"));
                // The bridge's `dma-ranges` (three-cell PCI addresses) let the
                // function reach the first 3 GiB of RAM one to one, and no more.
                assert_eq!(dma_address(vl805, 0x1000_0000, 0x1000), Some(0x1000_0000));
                assert_eq!(dma_address(vl805, 0xbfff_f000, 0x1000), Some(0xbfff_f000));
                assert_eq!(dma_address(vl805, 0xbfff_f000, 0x2000), None);
                // A node outside any PCI bus has no function.
                let (uart, _) = find_compatible(index, &["arm,pl011"]).expect("pl011");
                assert_eq!(pci_function(&uart), None);
                assert!(host_bridge_of(&uart).is_none());
            });
        }

        #[test]
        fn rpi5_rp1_stands_for_the_function_behind_the_root_port() {
            with_index(RPI5, |index| {
                let rp1 = index
                    .nodes()
                    .find(|node| has_path(node, "/axi/pcie@1000120000/rp1"))
                    .expect("rp1");
                assert_eq!(pci_function(&rp1), Some(0x100));
                assert_eq!(
                    host_bridge_of(&rp1).map(|bridge| bridge.name()),
                    Some(Ok("pcie@1000120000"))
                );
            });
        }

        #[test]
        fn rpi5_console_uart_states_its_peripheral_id() {
            // uart10's 0x200-byte block ends before the ID registers at 0xFE0.
            with_index(RPI5, |index| {
                let uart = index
                    .nodes()
                    .find(|node| has_path(node, "/soc@107c000000/serial@7d001000"))
                    .expect("uart10");
                assert_eq!(primecell_periphid(&uart), Some(0x0034_1011));
            });
        }

        #[test]
        fn rpi5_wifi_controller_lists_its_device_and_the_card_slot_none() {
            // Kickstart leaves an SD host controller with a child node
            // undriven: the child is an SDIO device, not a card.
            with_index(RPI5, |index| {
                let children = |path: &str| {
                    let node = index
                        .nodes()
                        .find(|node| has_path(node, path))
                        .unwrap_or_else(|| panic!("{path}"));
                    node.children()
                        .map(|child| child.name().unwrap_or("?").to_owned())
                        .collect::<Vec<_>>()
                };
                assert_eq!(children("/axi/mmc@1100000"), ["wifi@1"]);
                assert!(children("/soc@107c000000/mmc@fff000").is_empty());
            });
        }

        #[test]
        fn rpi5_rp1_bridge_windows_mip_and_resets() {
            with_index(RPI5, |index| {
                let bridge = index
                    .nodes()
                    .find(|node| node.name() == Ok("pcie@1000120000"))
                    .expect("pcie2");
                assert_eq!(
                    pci_ranges(&bridge).collect::<Vec<_>>(),
                    [
                        PciRange {
                            space: PciSpace::Memory32,
                            prefetchable: false,
                            pci: 0,
                            cpu: 0x1f_0000_0000,
                            size: 0xffff_fffc,
                        },
                        PciRange {
                            space: PciSpace::Memory64,
                            prefetchable: true,
                            pci: 0x4_0000_0000,
                            cpu: 0x1c_0000_0000,
                            size: 0x3_0000_0000,
                        },
                    ]
                );
                assert_eq!(
                    pci_dma_ranges(&bridge).collect::<Vec<_>>(),
                    [
                        PciRange {
                            space: PciSpace::Memory32,
                            prefetchable: false,
                            pci: 0,
                            cpu: 0x1f_0000_0000,
                            size: 0x40_0000,
                        },
                        // All of RAM, at 64 GiB on the PCI side.
                        PciRange {
                            space: PciSpace::Memory64,
                            prefetchable: true,
                            pci: 0x10_0000_0000,
                            cpu: 0,
                            size: 0x10_0000_0000,
                        },
                        // The MIP's doorbell page.
                        PciRange {
                            space: PciSpace::Memory64,
                            prefetchable: false,
                            pci: 0xff_ffff_f000,
                            cpu: 0x10_0013_0000,
                            size: 0x1000,
                        },
                    ]
                );
                let mip = msi_parent(index, &bridge).expect("msi-parent");
                assert_eq!(mip.name(), Ok("msi-controller@1000130000"));
                let range = msi_range(index, &mip).expect("msi-ranges");
                // GIC SPI 128 up, rising edge.
                assert_eq!(range.cells(), [0, 0x80, 1]);
                assert_eq!(range.count, 64);
                assert_eq!(brcm_msi_offset(&mip), 0);
                let (rescal, _) =
                    find_compatible(index, &["brcm,bcm7216-pcie-sata-rescal"]).expect("rescal");
                let (bridge_resets, _) =
                    find_compatible(index, &["brcm,brcmstb-reset"]).expect("brcmstb-reset");
                assert_eq!(
                    resets(index, &bridge).collect::<Vec<_>>(),
                    [
                        Reset {
                            controller: phandle(&rescal).expect("phandle"),
                            id: 0,
                            name: "rescal",
                        },
                        Reset {
                            controller: phandle(&bridge_resets).expect("phandle"),
                            id: 0x2c,
                            name: "bridge",
                        },
                    ]
                );
            });
        }
    }

    #[test]
    fn rpi3_reg_uses_parent_cells_and_translates_through_soc_ranges() {
        with_index(RPI3, |index| {
            let (armctrl, _) =
                find_compatible(index, &["brcm,bcm2836-armctrl-ic"]).expect("armctrl node");
            assert_eq!(
                reg_cells(&armctrl),
                Cells {
                    address: 1,
                    size: 1
                }
            );
            assert_eq!(
                reg(&armctrl).collect::<Vec<_>>(),
                [Region {
                    start: 0x7e00_b200,
                    size: 0x200
                }]
            );
            assert_eq!(
                regions(&armctrl).collect::<Vec<_>>(),
                [Region {
                    start: 0x3f00_b200,
                    size: 0x200
                }]
            );

            let (local, _) = find_compatible(index, &["brcm,bcm2836-l1-intc"]).expect("local intc");
            assert_eq!(
                regions(&local).collect::<Vec<_>>(),
                [Region {
                    start: 0x4000_0000,
                    size: 0x100
                }]
            );
        });
    }

    #[test]
    fn soc_devices_reach_ram_through_dma_ranges() {
        for (blob, ram_size) in [(RPI3, 0x3f00_0000), (RPI4, 0x4000_0000)] {
            with_index(blob, |index| {
                let (mailbox, _) =
                    find_compatible(index, &["brcm,bcm2835-mbox"]).expect("mailbox node");
                // `/soc` maps the bus window at 0xC000_0000 onto RAM from 0.
                assert_eq!(dma_address(&mailbox, 0, 0x1000), Some(0xc000_0000));
                assert_eq!(dma_address(&mailbox, 0x10_0000, 0x1000), Some(0xc010_0000));
                assert_eq!(
                    dma_address(&mailbox, ram_size - 0x1000, 0x1000),
                    Some(0xc000_0000 + ram_size - 0x1000)
                );
                // A range running past the entry is not reachable.
                assert_eq!(dma_address(&mailbox, ram_size - 0x1000, 0x2000), None);
            });
        }
    }

    #[test]
    fn dma_channel_masks_come_from_the_dma_node() {
        with_index(RPI3, |index| {
            let (dma, _) = find_compatible(index, &["brcm,bcm2835-dma"]).expect("DMA node");
            assert_eq!(brcm_dma_channel_mask(&dma), Some(0x7f35));
            let (uart, _) = find_compatible(index, &["arm,pl011"]).expect("PL011");
            assert_eq!(brcm_dma_channel_mask(&uart), None);
        });
        with_index(RPI4, |index| {
            let (dma, _) = find_compatible(index, &["brcm,bcm2835-dma"]).expect("DMA node");
            assert_eq!(brcm_dma_channel_mask(&dma), Some(0x07f5));
        });
    }

    #[test]
    fn rpi3_card_pins_route_to_the_enabled_controller() {
        with_index(RPI3, |index| {
            let pins_of = |path_compatible: &str| {
                index
                    .nodes()
                    .filter(|node| is_compatible(node, path_compatible))
                    .map(|node| {
                        (
                            is_enabled(&node),
                            brcm_pins(index, &node).collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let card_pins = |function| {
                (48..54)
                    .map(|pin| BrcmPin {
                        pin,
                        function,
                        pull: None,
                    })
                    .collect::<Vec<_>>()
            };
            // SDHOST is enabled with the card's pins on ALT0, pulls left as
            // they are.
            assert_eq!(pins_of("brcm,bcm2835-sdhost"), [(true, card_pins(4))]);
            // The Arasan node for the card is disabled (pins on ALT3); the
            // enabled one drives the Wi-Fi chip on GPIO 34 to 39, its clock
            // without a pull and the other lines pulled up.
            let arasan = pins_of("brcm,bcm2835-sdhci");
            assert!(arasan.contains(&(false, card_pins(7))));
            let wifi = (34..40)
                .map(|pin| BrcmPin {
                    pin,
                    function: 7,
                    pull: Some(if pin == 34 { 0 } else { 2 }),
                })
                .collect();
            assert!(arasan.contains(&(true, wifi)));
        });
    }

    #[test]
    fn every_pin_group_of_a_node_is_read() {
        with_index(RPI3, |index| {
            // The PL011 links two groups: `uart0_pins` (GPIO 32 and 33 on
            // ALT3, one pull each) and `bt_pins` (GPIO 43 on ALT0, one pull).
            let (uart, _) = find_compatible(index, &["arm,pl011"]).expect("PL011 node");
            assert_eq!(
                brcm_pins(index, &uart).collect::<Vec<_>>(),
                [
                    BrcmPin {
                        pin: 32,
                        function: 7,
                        pull: Some(0),
                    },
                    BrcmPin {
                        pin: 33,
                        function: 7,
                        pull: Some(2),
                    },
                    BrcmPin {
                        pin: 43,
                        function: 4,
                        pull: Some(0),
                    },
                ]
            );
        });
    }

    #[test]
    fn gpio_specifiers_read_in_order_with_their_flags() {
        with_index(SPI_PANEL, |index| {
            let (gpio, _) = find_compatible(index, &["brcm,bcm2711-gpio"]).expect("gpio");
            let controller = phandle(&gpio).expect("the gpio node's phandle");
            let node = |name: &str| {
                index
                    .nodes()
                    .find(|node| node.name() == Ok(name))
                    .unwrap_or_else(|| panic!("{name}"))
            };
            // SPI0's `cs-gpios = <&gpio 8 1 &gpio 7 1>`: chip select 0 is
            // GPIO 8, 1 is GPIO 7, both active low.
            let spi = node("spi@7e204000");
            let chip_select = |cs| gpio_specifier(index, &spi, "cs-gpios", cs);
            assert_eq!(
                chip_select(0),
                Some(GpioSpecifier {
                    controller,
                    pin: 8,
                    flags: 1
                })
            );
            assert!(chip_select(0).is_some_and(|specifier| specifier.active_low()));
            assert_eq!(chip_select(1).map(|specifier| specifier.pin), Some(7));
            assert_eq!(chip_select(2), None);
            // The panel's D/C and reset pins, active high.
            let panel = node("panel@0");
            let dc = gpio_specifier(index, &panel, "dc-gpios", 0).expect("dc-gpios");
            assert_eq!((dc.pin, dc.active_low()), (25, false));
            assert_eq!(
                gpio_specifier(index, &panel, "reset-gpios", 0).map(|reset| reset.pin),
                Some(24)
            );
            assert_eq!(gpio_specifier(index, &panel, "missing-gpios", 0), None);
        });
    }

    #[test]
    fn a_node_path_names_every_level_from_the_root() {
        with_index(SPI_PANEL, |index| {
            let path_of = |name: &str, room: usize| {
                let node = index
                    .nodes()
                    .find(|node| node.name() == Ok(name))
                    .unwrap_or_else(|| panic!("{name}"));
                let mut out = vec![0; room];
                write_path(&node, &mut out).map(|len| String::from_utf8(out[..len].to_vec()))
            };
            assert_eq!(
                path_of("panel@0", 96),
                Some(Ok("/soc/spi@7e204000/panel@0".to_string()))
            );
            assert_eq!(
                path_of("spi@7e204000", 96),
                Some(Ok("/soc/spi@7e204000".to_string()))
            );
            assert_eq!(path_of("", 96), Some(Ok("/".to_string())), "the root");
            // A path that does not fit is refused, not cut.
            assert_eq!(path_of("panel@0", 20), None);
        });
    }

    /// The pins of `pins`, as a `gpio_pins` mask.
    fn mask_of(pins: &[u32]) -> u64 {
        pins.iter().fold(0, |mask, pin| mask | (1 << pin))
    }

    #[test]
    fn a_node_owns_the_pins_it_names_on_its_controller() {
        with_index(SPI_PANEL, |index| {
            let (gpio, _) = find_compatible(index, &["brcm,bcm2711-gpio"]).expect("gpio");
            let controller = phandle(&gpio).expect("the gpio node's phandle");
            let named = |name: &str| {
                let node = index
                    .nodes()
                    .find(|node| node.name() == Ok(name))
                    .unwrap_or_else(|| panic!("{name}"));
                gpio_pins(index, &node, controller)
            };
            // The panel: `reset-gpios` 24, `dc-gpios` 25, and GPIO 23 of the
            // `gpio-backlight` node its `backlight` links to.
            assert_eq!(named("panel@0"), mask_of(&[23, 24, 25]));
            // SPI0: its pin groups (MISO, MOSI, SCLK on ALT0; CE0 and CE1 as
            // outputs) and its `cs-gpios` 8 and 7.
            assert_eq!(named("spi@7e204000"), mask_of(&[7, 8, 9, 10, 11]));
            // PWM1: its two generic pin groups.
            assert_eq!(named("pwm@7e20c800"), mask_of(&[40, 41]));
            // Bluetooth's `shutdown-gpios` names the firmware's expander, not
            // this controller.
            assert_eq!(named("bluetooth"), 0);
        });
    }

    #[test]
    fn rpi4_generic_pin_groups_route_the_jack_to_pwm1() {
        for tree in [RPI4, RPI4_FIRMWARE] {
            with_index(tree, |index| {
                // `pwm1` links `pwm1_0_gpio40` and `pwm1_1_gpio41`, each with a
                // `pin-pwm` child: `pins = "gpio40"; function = "alt0";
                // bias-disable;` (BCM2711 ARM Peripherals, section 5.3: GPIO 40
                // and 41 ALT0 are PWM1_0 and PWM1_1).
                let pwm1 = index
                    .nodes()
                    .find(|node| node.name() == Ok("pwm@7e20c800"))
                    .expect("pwm1");
                assert_eq!(
                    brcm_pins(index, &pwm1).collect::<Vec<_>>(),
                    [
                        BrcmPin {
                            pin: 40,
                            function: 4,
                            pull: Some(0),
                        },
                        BrcmPin {
                            pin: 41,
                            function: 4,
                            pull: Some(0),
                        },
                    ]
                );
            });
        }
    }

    #[test]
    fn generic_function_names_are_the_select_codes() {
        let names = [
            "gpio_in", "gpio_out", "alt5", "alt4", "alt0", "alt1", "alt2", "alt3",
        ];
        for (code, name) in names.into_iter().enumerate() {
            assert_eq!(
                generic_function_code(name),
                u32::try_from(code).ok(),
                "{name}"
            );
        }
        assert_eq!(generic_function_code("alt6"), None);
    }

    #[test]
    fn nodes_without_a_pin_group_set_up_no_pins() {
        with_index(RPI3, |index| {
            let (mailbox, _) =
                find_compatible(index, &["brcm,bcm2835-mbox"]).expect("mailbox node");
            assert_eq!(brcm_pins(index, &mailbox).count(), 0);
        });
    }

    #[test]
    fn firmware_services_link_to_the_mailbox() {
        for blob in [RPI3, RPI4] {
            with_index(blob, |index| {
                let (power, _) =
                    find_compatible(index, &["raspberrypi,bcm2835-power"]).expect("power node");
                let firmware = node_by_phandle(
                    index,
                    linked_phandle(&power, "firmware").expect("firmware link"),
                )
                .expect("firmware node");
                assert!(is_compatible(&firmware, "raspberrypi,bcm2835-firmware"));
                let (mailbox, _) =
                    find_compatible(index, &["brcm,bcm2835-mbox"]).expect("mailbox node");
                assert_eq!(linked_phandle(&firmware, "mboxes"), phandle(&mailbox));
            });
        }
    }
    #[test]
    fn rpi3_untranslatable_address_is_rejected() {
        with_index(RPI3, |index| {
            let (armctrl, _) =
                find_compatible(index, &["brcm,bcm2836-armctrl-ic"]).expect("armctrl node");
            // Outside every /soc `ranges` entry.
            assert_eq!(translate(&armctrl, 0x1000_0000), None);
        });
    }

    #[test]
    fn rpi3_interrupt_parent_chain_reaches_the_local_controller() {
        with_index(RPI3, |index| {
            let (armctrl, _) =
                find_compatible(index, &["brcm,bcm2836-armctrl-ic"]).expect("armctrl node");
            let parent = property(&armctrl, "interrupt-parent")
                .and_then(|prop| prop.u32(0).ok())
                .expect("interrupt-parent");
            let parent = node_by_phandle(index, parent).expect("parent node");
            assert!(is_compatible(&parent, "brcm,bcm2836-l1-intc"));
        });
    }

    /// The controller the CPU timer is wired to: the root interrupt controller.
    const TIMERS: &[&str] = &["arm,armv7-timer", "arm,armv8-timer"];

    #[test]
    fn rpi3_timer_interrupt_parent_is_the_local_controller() {
        with_index(RPI3, |index| {
            let (timer, _) = find_compatible(index, TIMERS).expect("timer node");
            let controller = interrupt_parent(index, &timer).expect("timer interrupt-parent");
            assert!(is_compatible(&controller, "brcm,bcm2836-l1-intc"));
            assert!(is_interrupt_controller(&controller));
        });
    }

    #[test]
    fn rpi3_physical_timer_specifier_is_local_line_one() {
        with_index(RPI3, |index| {
            let (timer, _) = find_compatible(index, TIMERS).expect("timer node");
            let controller = interrupt_parent(index, &timer).expect("timer interrupt-parent");
            let cell_count = interrupt_cells(&controller).expect("#interrupt-cells");
            assert_eq!(cell_count, 2);
            let mut cells = [0_u32; 4];
            // Entry 1 is the non-secure physical timer (CNTPNSIRQ).
            let specifier = interrupt_specifier(&timer, 1, cell_count as usize, &mut cells);
            assert_eq!(specifier, Some(&[1_u32, 4][..]));
            assert!(interrupt_specifier(&timer, 4, 2, &mut cells).is_none());
            assert!(is_enabled(&controller));
        });
    }

    #[test]
    fn rpi4_physical_timer_specifier_is_ppi_fourteen() {
        with_index(RPI4, |index| {
            let (timer, _) = find_compatible(index, TIMERS).expect("timer node");
            let controller = interrupt_parent(index, &timer).expect("timer interrupt-parent");
            let cell_count = interrupt_cells(&controller).expect("#interrupt-cells");
            let mut cells = [0_u32; 4];
            let specifier = interrupt_specifier(&timer, 1, cell_count as usize, &mut cells);
            // <GIC_PPI 14 flags>: PPI 14 is INTID 30.
            assert_eq!(specifier, Some(&[1_u32, 14, 0xf08][..]));
        });
    }

    #[test]
    fn rpi4_timer_inherits_the_gic_from_the_root() {
        with_index(RPI4, |index| {
            // RPi4 also describes the BCM2836 local controller block, but not as an
            // interrupt controller: only the timer's (inherited) parent is the root.
            let (local, _) =
                find_compatible(index, &["brcm,bcm2836-l1-intc"]).expect("local block");
            assert!(!is_interrupt_controller(&local));

            let (timer, _) = find_compatible(index, TIMERS).expect("timer node");
            let controller = interrupt_parent(index, &timer).expect("timer interrupt-parent");
            assert!(is_compatible(&controller, "arm,gic-400"));
        });
    }

    #[test]
    fn rpi4_gic_translates_with_two_cell_parent_addresses() {
        with_index(RPI4, |index| {
            let root_cells = bus_cells(&index.root());
            assert_eq!(root_cells.address, 2);

            let (gic, matched) = find_compatible(index, &["arm,gic-400"]).expect("gic node");
            assert_eq!(matched, "arm,gic-400");
            let gic_regions = regions(&gic).collect::<Vec<_>>();
            assert_eq!(gic_regions.len(), 4);
            assert_eq!(
                gic_regions[0],
                Region {
                    start: 0xff84_1000,
                    size: 0x1000
                }
            );
            assert_eq!(
                gic_regions[1],
                Region {
                    start: 0xff84_2000,
                    size: 0x2000
                }
            );
        });
    }

    #[test]
    fn compatible_matches_any_entry_of_the_string_list() {
        with_index(RPI3, |index| {
            let (mmc, _) = find_compatible(index, &["brcm,bcm2835-sdhci"]).expect("sdhci node");
            assert!(is_compatible(&mmc, "brcm,bcm2835-mmc"));
            assert!(is_compatible(&mmc, "brcm,bcm2835-sdhci"));
            assert!(!is_compatible(&mmc, "brcm,bcm2835"));
        });
    }

    #[test]
    fn paths_find_nodes_with_or_without_unit_addresses() {
        with_tree(RPI3, |tree| {
            assert_eq!(tree.model(), Some("Raspberry Pi 3 Model B+"));
            assert_eq!(
                tree.property("/model").and_then(|prop| prop.str().ok()),
                Some("Raspberry Pi 3 Model B+")
            );
            let local = tree.node("/soc/local_intc@40000000").expect("full path");
            assert!(is_compatible(&local, "brcm,bcm2836-l1-intc"));
            let by_base_name = tree
                .node("/soc/local_intc")
                .expect("path without unit address");
            assert_eq!(phandle_of(&by_base_name), phandle_of(&local));
            assert_eq!(
                tree.property("/soc/#address-cells")
                    .and_then(|prop| prop.u32(0).ok()),
                Some(1)
            );
            assert!(tree.node("/soc/no-such-node").is_none());
            assert!(tree.property("/soc/no-such-property").is_none());
            assert!(tree.node("/").is_some());
        });
    }

    fn phandle_of(node: &vesper_devicetree::Node<'_, '_, '_>) -> Option<u32> {
        property(node, "phandle").and_then(|prop| prop.u32(0).ok())
    }

    #[test]
    fn memory_skips_the_unpatched_empty_memory_node() {
        // The firmware (or QEMU) fills in the memory size; the stock DTBs
        // carry `reg = <0 0>`.
        with_tree(RPI3, |tree| {
            assert!(tree.node("/memory@0").is_some());
            assert_eq!(tree.memory().count(), 0);
        });
    }

    #[test]
    fn reserved_memory_lists_the_memreserve_entries() {
        // Both boards reserve the first page (the firmware's spin tables).
        for dtb in [RPI3, RPI4] {
            with_tree(dtb, |tree| {
                assert_eq!(
                    tree.reserved_memory().collect::<Vec<_>>(),
                    [Region {
                        start: 0,
                        size: 0x1000
                    }]
                );
            });
        }
    }

    #[test]
    fn no_map_reserved_memory_lists_resident_firmware_only() {
        // The Pi 5 keeps TF-A in its low 512 KiB; its disabled, empty bootloader
        // nvram nodes and the run-time CMA pool are no reservations.
        with_tree(RPI5, |tree| {
            assert_eq!(
                tree.no_map_reserved_memory().collect::<Vec<_>>(),
                [Region {
                    start: 0,
                    size: 0x8_0000
                }]
            );
        });
        for dtb in [RPI3, RPI4] {
            with_tree(dtb, |tree| {
                assert_eq!(tree.no_map_reserved_memory().count(), 0);
            });
        }
    }

    #[test]
    fn devices_are_translated_named_and_exclude_memory() {
        with_tree(RPI3, |tree| {
            let local = tree
                .devices()
                .find(|device| device.name == "local_intc")
                .expect("local controller device");
            assert_eq!(local.compatible, "brcm,bcm2836-l1-intc");
            assert_eq!(
                local.region,
                Region {
                    start: 0x4000_0000,
                    size: 0x100
                }
            );
            assert!(local.enabled);
            assert!(local.phandle.is_some());
            assert!(tree.devices().all(|device| device.name != "memory"));
            // Untranslatable entries (e.g. on busses without `ranges`) are absent.
            assert!(
                tree.devices()
                    .all(|device| device.region.start >= 0x3f00_0000)
            );
        });
        with_tree(RPI4, |tree| {
            // A node whose `status` is "disabled".
            let disabled = tree
                .devices()
                .find(|device| device.compatible == "brcm,bcm2711-l2-intc" && !device.enabled);
            assert!(disabled.is_some());
        });
    }

    #[test]
    fn device_region_counts_per_board() {
        // kickstart sorts the device regions in a fixed array
        // (`MAX_DEVICE_REGIONS`, 256): the Raspberry Pi 4 tree has over a
        // hundred, which overflowed the 100 it once had.
        for (dtb, regions) in [(RPI3, 48), (RPI4, 102), (RPI5, 84)] {
            with_tree(dtb, |tree| assert_eq!(tree.devices().count(), regions));
        }
    }

    #[test]
    fn dump_writes_device_tree_source() {
        with_tree(RPI3, |tree| {
            let mut out = String::new();
            dump(tree, &mut out).expect("dump into a String");
            assert!(out.starts_with("// magic:\t\t0xd00dfeed\n"));
            assert!(out.contains("\n/ {\n"));
            assert!(out.contains("    local_intc@40000000 {\n"));
            assert!(out.contains("      compatible = \"brcm,bcm2836-l1-intc\";\n"));
            assert!(out.contains("      reg = <0x40000000 0x100>;\n"));
            assert!(out.contains("      interrupt-controller;\n"));
            assert!(out.trim_end().ends_with("};"));
        });
    }

    // ── Interrupt topology ──────────────────────────────────────────────

    use vesper_devicetree::interrupts::{
        Interrupt, TimerInterrupt, distinct_interrupts, generic_timer, interrupt,
        interrupt_controllers, interrupt_named, interrupts, root_interrupt_controller,
        timer_interrupt,
    };

    fn phandle_of_compatible(index: &DevTreeIndex<'_, '_>, compatible: &str) -> u32 {
        let (node, _) = find_compatible(index, &[compatible]).expect("node");
        phandle_of(&node).expect("phandle")
    }

    #[test]
    fn rpi3_timer_interrupts_resolve_to_the_local_controller() {
        with_index(RPI3, |index| {
            let local = phandle_of_compatible(index, "brcm,bcm2836-l1-intc");
            let root = root_interrupt_controller(index).expect("root controller");
            assert_eq!(phandle_of(&root), Some(local));
            let physical = timer_interrupt(index, TimerInterrupt::NonSecurePhysical).unwrap();
            assert_eq!(
                (physical.controller, physical.cells()),
                (local, &[1, 4][..])
            );
            let virtual_timer = timer_interrupt(index, TimerInterrupt::Virtual).unwrap();
            assert_eq!(virtual_timer.cells(), &[3, 4]);
            let timer = generic_timer(index).expect("timer");
            assert_eq!(interrupts(index, &timer).count(), 4);
            assert_eq!(interrupt(index, &timer, 4), None);
        });
    }

    #[test]
    fn rpi4_timer_interrupts_resolve_to_the_gic() {
        with_index(RPI4, |index| {
            let gic = phandle_of_compatible(index, "arm,gic-400");
            let virtual_timer = timer_interrupt(index, TimerInterrupt::Virtual).unwrap();
            assert_eq!(
                (virtual_timer.controller, virtual_timer.cells()),
                (gic, &[1, 11, 0xf08][..])
            );
            let physical = timer_interrupt(index, TimerInterrupt::NonSecurePhysical).unwrap();
            assert_eq!(physical.cells(), &[1, 14, 0xf08]);
        });
    }

    #[test]
    fn rpi4_bridge_msi_line_is_found_by_its_name() {
        // The upstream tree names SPI 148 for both `pcie` and `msi`; the
        // tree the Pi 4 firmware ships names SPI 147 for `pcie`.
        for (tree, pcie_line) in [(RPI4, 148), (RPI4_FIRMWARE, 147)] {
            with_index(tree, |index| {
                let (bridge, _) = find_compatible(index, &["brcm,bcm2711-pcie"]).expect("pcie0");
                let msi = interrupt_named(index, &bridge, "msi").expect("msi line");
                assert_eq!(msi.cells(), &[0, 148, 4]);
                let pcie = interrupt_named(index, &bridge, "pcie").expect("pcie line");
                assert_eq!(pcie.cells(), &[0, pcie_line, 4]);
                assert_eq!(interrupt_named(index, &bridge, "intx"), None);
            });
        }
    }

    #[test]
    fn uart_interrupts_resolve_through_their_inherited_parent() {
        // RPi3: the PL011 sits on the cascaded ARM-control block, GPU IRQ 57.
        with_index(RPI3, |index| {
            let (uart, _) = find_compatible(index, &["arm,pl011"]).expect("PL011");
            let armctrl = phandle_of_compatible(index, "brcm,bcm2836-armctrl-ic");
            let uart_interrupt = interrupt(index, &uart, 0).expect("PL011 interrupt");
            assert_eq!(
                (uart_interrupt.controller, uart_interrupt.cells()),
                (armctrl, &[2, 25][..])
            );
        });
        // RPi4: SPI 121 on the GIC.
        with_index(RPI4, |index| {
            let (uart, _) = find_compatible(index, &["arm,pl011"]).expect("PL011");
            let gic = phandle_of_compatible(index, "arm,gic-400");
            let uart_interrupt = interrupt(index, &uart, 0).expect("PL011 interrupt");
            assert_eq!(
                (uart_interrupt.controller, uart_interrupt.cells()),
                (gic, &[0, 121, 4][..])
            );
        });
    }

    #[test]
    fn rpi3_dma_node_lists_its_shared_line_once_when_deduplicated() {
        // The DMA node lists GPU interrupts 16 to 26 for channels 0 to 10,
        // 27 four times (channels 11 to 14 share it) and 28 (dma-shared-all).
        with_index(RPI3, |index| {
            let (dma, _) = find_compatible(index, &["brcm,bcm2835-dma"]).expect("DMA node");
            let armctrl = phandle_of_compatible(index, "brcm,bcm2836-armctrl-ic");
            assert_eq!(interrupts(index, &dma).count(), 16);
            let distinct: Vec<_> = distinct_interrupts(index, &dma).collect();
            assert_eq!(distinct.len(), 13);
            assert!(distinct.iter().all(|line| line.controller == armctrl));
            let lines: Vec<_> = distinct.iter().map(|line| line.cells()[1]).collect();
            assert_eq!(lines, (16..=28).collect::<Vec<_>>());
            assert!(distinct.iter().all(|line| line.cells()[0] == 1));
        });
        // A node without repeats keeps every entry.
        with_index(RPI3, |index| {
            let (uart, _) = find_compatible(index, &["arm,pl011"]).expect("PL011");
            assert!(distinct_interrupts(index, &uart).eq(interrupts(index, &uart)));
        });
    }

    #[test]
    fn interrupt_controllers_list_the_root_first() {
        with_index(RPI3, |index| {
            let local = phandle_of_compatible(index, "brcm,bcm2836-l1-intc");
            let armctrl = phandle_of_compatible(index, "brcm,bcm2836-armctrl-ic");
            let driven: Vec<_> =
                interrupt_controllers(index, |node| is_compatible(node, "brcm,bcm2836-armctrl-ic"))
                    .map(|node| phandle_of(&node))
                    .collect();
            assert_eq!(driven, [Some(local), Some(armctrl)]);
            // Without a filter every other enabled controller follows, the
            // root never twice.
            let all: Vec<_> = interrupt_controllers(index, |_| true)
                .map(|node| phandle_of(&node))
                .collect();
            assert_eq!(all[0], Some(local));
            assert!(all.contains(&Some(armctrl)));
            assert_eq!(all.iter().filter(|&&p| p == Some(local)).count(), 1);
        });
        with_index(RPI4, |index| {
            let gic = phandle_of_compatible(index, "arm,gic-400");
            let first = interrupt_controllers(index, |_| false).map(|node| phandle_of(&node));
            assert_eq!(first.collect::<Vec<_>>(), [Some(gic)]);
        });
    }

    /// A minimal flattened device tree: `/` with the controllers `a`
    /// (phandle 1, one cell) and `b` (phandle 2, two cells) and a node `dev`
    /// whose `interrupts-extended` routes one interrupt to each.
    fn extended_interrupts_blob() -> Vec<u8> {
        const BEGIN_NODE: u32 = 1;
        const END_NODE: u32 = 2;
        const PROP: u32 = 3;
        const END: u32 = 9;
        const HEADER: usize = 40;
        const RESERVE_MAP: usize = 16;
        let mut strings = Vec::new();
        let mut structure = Vec::new();
        let word = |structure: &mut Vec<u8>, value: u32| {
            structure.extend_from_slice(&value.to_be_bytes());
        };
        let begin = |structure: &mut Vec<u8>, name: &str| {
            structure.extend_from_slice(&BEGIN_NODE.to_be_bytes());
            structure.extend_from_slice(name.as_bytes());
            structure.push(0);
            while !structure.len().is_multiple_of(4) {
                structure.push(0);
            }
        };
        let mut prop = |structure: &mut Vec<u8>, name: &str, cells: &[u32]| {
            let offset = u32::try_from(strings.len()).unwrap();
            strings.extend_from_slice(name.as_bytes());
            strings.push(0);
            structure.extend_from_slice(&PROP.to_be_bytes());
            structure.extend_from_slice(&u32::try_from(cells.len() * 4).unwrap().to_be_bytes());
            structure.extend_from_slice(&offset.to_be_bytes());
            for cell in cells {
                structure.extend_from_slice(&cell.to_be_bytes());
            }
        };
        begin(&mut structure, "");
        for (name, phandle, cells) in [("a", 1, 1), ("b", 2, 2)] {
            begin(&mut structure, name);
            prop(&mut structure, "interrupt-controller", &[]);
            prop(&mut structure, "#interrupt-cells", &[cells]);
            prop(&mut structure, "phandle", &[phandle]);
            word(&mut structure, END_NODE);
        }
        begin(&mut structure, "dev");
        prop(&mut structure, "interrupts-extended", &[1, 7, 2, 5, 6]);
        word(&mut structure, END_NODE);
        word(&mut structure, END_NODE);
        word(&mut structure, END);

        let structure_offset = HEADER + RESERVE_MAP;
        let strings_offset = structure_offset + structure.len();
        let total = strings_offset + strings.len();
        let mut blob = Vec::new();
        for value in [
            0xd00d_feed,
            total,
            structure_offset,
            strings_offset,
            HEADER,
            17,
            16,
            0,
            strings.len(),
            structure.len(),
        ] {
            blob.extend_from_slice(&u32::try_from(value).unwrap().to_be_bytes());
        }
        blob.extend_from_slice(&[0; RESERVE_MAP]);
        blob.extend_from_slice(&structure);
        blob.extend_from_slice(&strings);
        blob
    }

    #[test]
    fn interrupts_extended_names_each_entrys_controller() {
        let blob = extended_interrupts_blob();
        with_tree(&blob, |tree| {
            let index = tree.index();
            let device = tree.node("/dev").expect("dev node");
            assert_eq!(
                interrupt(index, &device, 0),
                Some(Interrupt {
                    controller: 1,
                    cell_count: 1,
                    cells: [7, 0, 0, 0]
                })
            );
            assert_eq!(
                interrupt(index, &device, 1),
                Some(Interrupt {
                    controller: 2,
                    cell_count: 2,
                    cells: [5, 6, 0, 0]
                })
            );
            assert_eq!(interrupt(index, &device, 2), None);
            assert_eq!(interrupts(index, &device).count(), 2);
        });
    }

    mod board {
        use {
            super::{RPI3, RPI4, RPI5, with_tree},
            vesper_devicetree::{
                ActivityLed, Board, ConsoleUart, DeviceTree, Gpio, GpioKind, Mailbox, Soc,
                UartClock, find_compatible, fixed_clock_rate,
            },
        };

        #[test]
        fn every_board_tree_indexes_in_the_fixed_index_buffer() {
            for dtb in [RPI3, RPI4, RPI5] {
                let blob = super::AlignedBlob::new(dtb);
                // SAFETY: the blob is u32-aligned, complete, and outlives the call.
                let raw =
                    unsafe { DeviceTree::blob_from_raw(blob.bytes().as_ptr()) }.expect("valid DTB");
                let layout = DeviceTree::index_layout(&raw).expect("index layout");
                assert!(
                    layout.size() + layout.align() <= vesper_devicetree::INDEX_BUFFER_BYTES,
                    "{} bytes",
                    layout.size() + layout.align()
                );
            }
        }

        #[test]
        fn the_fallback_hardware_is_what_each_boards_tree_says() {
            for (dtb, soc) in [
                (RPI3, Soc::Bcm2837),
                (RPI4, Soc::Bcm2711),
                (RPI5, Soc::Bcm2712),
            ] {
                with_tree(dtb, |tree| {
                    assert_eq!(tree.board().soc, Some(soc));
                    assert_eq!(tree.boot_hardware(), Some(soc.fallback_boot_hardware()));
                });
            }
        }

        #[test]
        fn cpu_parts_name_the_socs_of_their_boards() {
            assert_eq!(Soc::from_cpu_part(0xd03), Some(Soc::Bcm2837));
            assert_eq!(Soc::from_cpu_part(0xd08), Some(Soc::Bcm2711));
            assert_eq!(Soc::from_cpu_part(0xd0b), Some(Soc::Bcm2712));
            // A Cortex-A57 is on no Raspberry Pi.
            assert_eq!(Soc::from_cpu_part(0xd07), None);
        }

        #[test]
        fn the_root_names_board_and_soc() {
            for (dtb, model, compatible, soc) in [
                (
                    RPI3,
                    "Raspberry Pi 3 Model B+",
                    "raspberrypi,3-model-b-plus",
                    Soc::Bcm2837,
                ),
                (
                    RPI4,
                    "Raspberry Pi 4 Model B",
                    "raspberrypi,4-model-b",
                    Soc::Bcm2711,
                ),
                (
                    RPI5,
                    "Raspberry Pi 5",
                    "raspberrypi,5-model-b",
                    Soc::Bcm2712,
                ),
            ] {
                with_tree(dtb, |tree| {
                    assert_eq!(
                        tree.board(),
                        Board {
                            model: Some(model),
                            compatible: Some(compatible),
                            soc: Some(soc),
                            // Only the firmware writes `/system/linux,revision`.
                            revision: None,
                        }
                    );
                });
            }
        }

        #[test]
        fn the_console_is_the_first_pl011_unless_stdout_path_names_one() {
            // The Raspberry Pi 3 and 4 trees have no `stdout-path`; their first
            // PL011 is `uart0`, clocked by the firmware.
            for (dtb, base) in [(RPI3, 0x3f20_1000), (RPI4, 0xfe20_1000)] {
                with_tree(dtb, |tree| {
                    assert_eq!(
                        tree.console_uart(),
                        Some(ConsoleUart {
                            base,
                            clock: UartClock::Firmware
                        })
                    );
                });
            }
            // The Raspberry Pi 5 tree's `stdout-path` is `serial10`, the debug
            // connector's PL011 in the high peripheral window, on a fixed clock.
            with_tree(RPI5, |tree| {
                assert_eq!(
                    tree.console_uart(),
                    Some(ConsoleUart {
                        base: 0x10_7d00_1000,
                        clock: UartClock::Fixed(9_216_000)
                    })
                );
            });
        }

        #[test]
        fn the_header_gpio_block_and_its_kind() {
            with_tree(RPI3, |tree| {
                assert_eq!(
                    tree.header_gpio(),
                    Some(Gpio {
                        base: 0x3f20_0000,
                        kind: GpioKind::Bcm2835
                    })
                );
            });
            with_tree(RPI4, |tree| {
                assert_eq!(
                    tree.header_gpio(),
                    Some(Gpio {
                        base: 0xfe20_0000,
                        kind: GpioKind::Bcm2711
                    })
                );
            });
            // The Raspberry Pi 5's header is on RP1.
            with_tree(RPI5, |tree| assert_eq!(tree.header_gpio(), None));
        }

        #[test]
        fn the_mailbox_and_the_address_the_firmware_sees() {
            // The uncached bus alias on the BCM2837 and BCM2711, from `dma-ranges`.
            for (dtb, base) in [(RPI3, 0x3f00_b880), (RPI4, 0xfe00_b880)] {
                with_tree(dtb, |tree| {
                    assert_eq!(
                        tree.firmware_mailbox(),
                        Some(Mailbox {
                            base,
                            bus_offset: 0xc000_0000
                        })
                    );
                });
            }
            // The BCM2712's bus has no `dma-ranges`: addresses map one to one.
            with_tree(RPI5, |tree| {
                assert_eq!(
                    tree.firmware_mailbox(),
                    Some(Mailbox {
                        base: 0x10_7c01_3880,
                        bus_offset: 0
                    })
                );
            });
        }

        #[test]
        fn the_activity_led_and_how_it_is_switched() {
            with_tree(RPI3, |tree| {
                assert_eq!(
                    tree.activity_led(),
                    Some(ActivityLed::Gpio {
                        gpio: Gpio {
                            base: 0x3f20_0000,
                            kind: GpioKind::Bcm2835
                        },
                        pin: 29,
                        active_low: false
                    })
                );
            });
            with_tree(RPI4, |tree| {
                assert_eq!(
                    tree.activity_led(),
                    Some(ActivityLed::Gpio {
                        gpio: Gpio {
                            base: 0xfe20_0000,
                            kind: GpioKind::Bcm2711
                        },
                        pin: 42,
                        active_low: false
                    })
                );
            });
            // On the always-on `brcm,brcmstb-gpio` block, which no boot
            // program drives.
            with_tree(RPI5, |tree| assert_eq!(tree.activity_led(), None));
        }

        #[test]
        fn sd_controller_clocks() {
            let clock_of = |tree: &DeviceTree<'_, '_>, compatible| {
                let (node, _) =
                    find_compatible(tree.index(), &[compatible]).expect("the SD controller");
                fixed_clock_rate(tree.index(), &node)
            };
            // The Raspberry Pi 5's SD controller runs from a fixed 200 MHz
            // clock (`clk_emmc2`).
            with_tree(RPI5, |tree| {
                assert_eq!(clock_of(tree, "brcm,bcm2712-sdhci"), Some(200_000_000));
            });
            // The Raspberry Pi 4's EMMC2 clock is the clock manager's: the
            // firmware tells its rate.
            with_tree(RPI4, |tree| {
                assert_eq!(clock_of(tree, "brcm,bcm2711-emmc2"), None);
            });
        }
    }

    mod cpus {
        use {
            super::{RPI3, RPI4, RPI5, with_index},
            vesper_devicetree::cpus::{
                Cpu, PSCI, PSCI_CPU_ON_64, Psci, PsciConduit, SPIN_TABLE, common_enable_method,
                cpus, psci,
            },
        };

        /// The spin-table CPUs of the Raspberry Pi 3 and 4: Aff0 0..3, release words
        /// 0xd8, 0xe0, 0xe8, 0xf0.
        fn spin_table_cpus() -> Vec<Cpu<'static>> {
            [0xd8, 0xe0, 0xe8, 0xf0]
                .into_iter()
                .enumerate()
                .map(|(index, release_address)| Cpu {
                    index,
                    affinity: index as u64,
                    enable_method: Some(SPIN_TABLE),
                    release_address: Some(release_address),
                })
                .collect()
        }

        #[test]
        fn pi_3_and_4_cpus_are_released_through_a_spin_table() {
            for blob in [RPI3, RPI4] {
                with_index(blob, |tree| {
                    assert_eq!(cpus(tree).collect::<Vec<_>>(), spin_table_cpus());
                    assert_eq!(common_enable_method(cpus(tree)), Some(SPIN_TABLE));
                    assert_eq!(psci(tree), None);
                });
            }
        }

        #[test]
        fn pi_5_cpus_are_numbered_by_aff1_and_started_by_psci() {
            with_index(RPI5, |tree| {
                // The cache nodes under each `cpu@N` are not CPUs.
                let found: Vec<_> = cpus(tree).collect();
                let affinities: Vec<_> = found.iter().map(|cpu| cpu.affinity).collect();
                assert_eq!(affinities, [0x000, 0x100, 0x200, 0x300]);
                assert!(
                    found
                        .iter()
                        .enumerate()
                        .all(|(index, cpu)| cpu.index == index
                            && cpu.enable_method == Some(PSCI)
                            && cpu.release_address.is_none())
                );
                assert_eq!(common_enable_method(cpus(tree)), Some(PSCI));
                assert_eq!(
                    psci(tree),
                    Some(Psci {
                        compatible: "arm,psci-1.0",
                        conduit: PsciConduit::Smc,
                        cpu_on: PSCI_CPU_ON_64,
                    })
                );
            });
        }

        #[test]
        fn disagreeing_or_missing_enable_methods_have_no_common_method() {
            let spin = spin_table_cpus();
            let mut mixed = spin.clone();
            mixed[2].enable_method = Some(PSCI);
            assert_eq!(common_enable_method(mixed.into_iter()), None);
            let mut missing = spin;
            missing[0].enable_method = None;
            assert_eq!(common_enable_method(missing.into_iter()), None);
            assert_eq!(common_enable_method(core::iter::empty()), None);
        }
    }
}
