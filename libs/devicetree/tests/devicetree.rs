//! Host tests against the real Raspberry Pi device trees in `targets/`.

#[cfg(test)]
mod tests {
    use vesper_devicetree::{
        Cells, DevTreeIndex, DeviceTree, PropReader, Region, bus_cells, dump, find_compatible,
        interrupt_cells, interrupt_parent, interrupt_specifier, is_compatible, is_enabled,
        is_interrupt_controller, node_by_phandle, property, reg, reg_cells, regions, translate,
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
}
