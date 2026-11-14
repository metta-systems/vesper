//! Host tests of enumeration against modelled buses: the Raspberry Pi 4's
//! root port with the VL805 behind it, and the Raspberry Pi 5's with RP1.

#[cfg(test)]
mod tests {
    use vesper_pci::{Bar, BarKind, ConfigSpace, Device, Function, PortType, Window, enumerate};

    /// Words of a modelled function's configuration space.
    const WORDS: usize = 1024;

    /// A modelled function: its words and which bits of each a write
    /// changes.
    struct Modelled {
        at: Function,
        words: Vec<u32>,
        writable: Vec<u32>,
    }

    impl Modelled {
        fn new(at: Function, vendor: u16, device: u16, class: u32, layout: u32) -> Self {
            let mut words = vec![0; WORDS];
            let mut writable = vec![0; WORDS];
            words[0] = u32::from(device) << 16 | u32::from(vendor);
            words[2] = class << 8;
            words[3] = layout << 16;
            // Command: I/O, memory, bus master, interrupt disable.
            writable[1] = 0x0000_0407;
            if layout == 1 {
                // Bus numbers and the windows; the prefetchable window has
                // upper halves.
                writable[0x18 / 4] = 0x00ff_ffff;
                writable[0x20 / 4] = 0xfff0_fff0;
                words[0x24 / 4] = 0x0001_0001;
                writable[0x24 / 4] = 0xfff0_fff0;
                writable[0x28 / 4] = u32::MAX;
                writable[0x2c / 4] = u32::MAX;
            }
            Self {
                at,
                words,
                writable,
            }
        }

        /// A memory BAR of `size` bytes at BAR `index`, 64-bit if `wide`.
        fn bar(mut self, index: usize, size: u64, wide: bool, prefetchable: bool) -> Self {
            let word = 4 + index;
            let mask = !(size - 1);
            self.words[word] = u32::from(wide) << 2 | u32::from(prefetchable) << 3;
            self.writable[word] = u32::try_from(mask & 0xffff_fff0).expect("low half");
            if wide {
                self.writable[word + 1] = u32::try_from(mask >> 32).expect("high half");
            }
            self
        }

        /// A capability list of `(offset, header word)`, linked in order.
        fn capabilities(mut self, list: &[(u16, u32)]) -> Self {
            self.words[1] |= 1 << 20;
            self.words[0x34 / 4] = list
                .first()
                .map_or(0, |(offset, _)| u32::from(*offset));
            for (position, (offset, header)) in list.iter().enumerate() {
                let next = list
                    .get(position + 1)
                    .map_or(0, |(next, _)| u32::from(*next));
                self.words[usize::from(offset / 4)] = header | next << 8;
            }
            self
        }

        fn multi_function(mut self) -> Self {
            self.words[3] |= 1 << 23;
            self
        }
    }

    /// A modelled bus: bus 0 holds only device 0 (as behind the Broadcom
    /// root complex), other buses answer once a bridge forwards them.
    struct Model {
        functions: Vec<Modelled>,
        /// Every function a read reached
        probed: Vec<Function>,
    }

    impl Model {
        fn forwarded(&self, bus: u8) -> bool {
            bus == 0
                || self.functions.iter().any(|modelled| {
                    let numbers = modelled.words[0x18 / 4];
                    modelled.words[3] >> 16 & 0x7f == 1
                        && (numbers >> 8 & 0xff) as u8 <= bus
                        && bus <= (numbers >> 16 & 0xff) as u8
                        && numbers >> 8 & 0xff != 0
                })
        }

        fn find(&mut self, function: Function) -> Option<&mut Modelled> {
            if (function.bus == 0 && function.device != 0) || !self.forwarded(function.bus) {
                return None;
            }
            self.functions
                .iter_mut()
                .find(|modelled| modelled.at == function)
        }
    }

    impl ConfigSpace for Model {
        fn read(&mut self, function: Function, offset: u16) -> Option<u32> {
            if (function.bus == 0 && function.device != 0) || !self.forwarded(function.bus) {
                return None;
            }
            self.probed.push(function);
            Some(
                self.find(function)
                    .map_or(u32::MAX, |modelled| modelled.words[usize::from(offset / 4)]),
            )
        }

        fn write(&mut self, function: Function, offset: u16, value: u32) {
            if let Some(modelled) = self.find(function) {
                let word = usize::from(offset / 4);
                let writable = modelled.writable[word];
                modelled.words[word] = modelled.words[word] & !writable | value & writable;
            }
        }
    }

    const fn at(bus: u8, device: u8, function: u8) -> Function {
        Function {
            bus,
            device,
            function,
        }
    }

    /// A root port's PCI Express capability header.
    const ROOT_PORT: u32 = 0x0042_0010;
    /// An endpoint's.
    const ENDPOINT: u32 = 0x0002_0010;
    const MSI: u32 = 0x05;
    const MSIX: u32 = 0x11;

    fn root_port(device: u16) -> Modelled {
        Modelled::new(at(0, 0, 0), 0x14e4, device, 0x06_0400, 1)
            .capabilities(&[(0xac, ROOT_PORT)])
    }

    fn word(model: &Model, function: Function, offset: usize) -> u32 {
        model
            .functions
            .iter()
            .find(|modelled| modelled.at == function)
            .expect("modelled")
            .words[offset / 4]
    }

    fn walk(model: &mut Model, windows: &[Window]) -> Vec<Device> {
        let mut found = [Device::default(); 8];
        let count = enumerate(&mut *model, windows, &mut found).expect("enumeration");
        found[..count].to_vec()
    }

    #[test]
    fn rpi4_vl805_gets_its_bar_from_the_window_behind_the_root_port() {
        let mut model = Model {
            functions: vec![
                root_port(0x2711),
                Modelled::new(at(1, 0, 0), 0x1106, 0x3483, 0x0c_0330, 0)
                    .bar(0, 0x1000, true, false)
                    .capabilities(&[(0x90, MSI), (0xc4, ENDPOINT)]),
            ],
            probed: Vec::new(),
        };
        let found = walk(
            &mut model,
            &[Window {
                pci: 0xc000_0000,
                size: 0x4000_0000,
                prefetchable: false,
            }],
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].function, at(0, 0, 0));
        assert_eq!(found[0].buses, Some((1, 1)));
        assert_eq!(found[0].express, Some(PortType::RootPort));
        let vl805 = &found[1];
        assert_eq!((vl805.vendor, vl805.device, vl805.class), (0x1106, 0x3483, 0x0c_0330));
        assert_eq!(vl805.express, Some(PortType::Endpoint));
        assert_eq!((vl805.msi, vl805.msix), (Some(0x90), None));
        assert_eq!(
            vl805.bars(),
            [Bar {
                index: 0,
                kind: BarKind::Memory64,
                pci: 0xc000_0000,
                size: 0x1000,
                prefetchable: false,
                assigned: true,
            }]
        );
        assert_eq!(word(&model, at(1, 0, 0), 0x10), 0xc000_0004);
        assert_eq!(word(&model, at(1, 0, 0), 0x14), 0);
        // The endpoint's decoding stays off; the root port forwards.
        assert_eq!(word(&model, at(1, 0, 0), 0x04) & 0x7, 0);
        assert_eq!(word(&model, at(0, 0, 0), 0x04) & 0x7, 0b110);
        assert_eq!(word(&model, at(0, 0, 0), 0x18), 0x0001_0100);
        // Memory window 0xc000_0000..=0xc00f_ffff; prefetchable closed.
        assert_eq!(word(&model, at(0, 0, 0), 0x20), 0xc000_c000);
        assert_eq!(word(&model, at(0, 0, 0), 0x24), 0x0001_fff1);
        // Behind a root port only device 0 is looked at.
        assert!(
            model
                .probed
                .iter()
                .all(|function| function.bus == 0 || function.device == 0)
        );
    }

    #[test]
    fn rpi5_rp1_bars_fill_the_32_bit_and_prefetchable_windows() {
        let mut model = Model {
            functions: vec![
                root_port(0x2712),
                Modelled::new(at(1, 0, 0), 0x1de4, 0x0001, 0x02_0000, 0)
                    .bar(1, 0x40_0000, false, false)
                    .bar(2, 0x1_0000, false, false)
                    .bar(4, 0x10_0000, true, true)
                    .capabilities(&[(0x40, ENDPOINT), (0xb0, MSIX)]),
            ],
            probed: Vec::new(),
        };
        let found = walk(
            &mut model,
            &[
                Window {
                    pci: 0,
                    size: 0xffff_fffc,
                    prefetchable: false,
                },
                Window {
                    pci: 0x4_0000_0000,
                    size: 0x3_0000_0000,
                    prefetchable: true,
                },
            ],
        );
        let rp1 = &found[1];
        assert_eq!(rp1.msix, Some(0xb0));
        let assigned: Vec<_> = rp1
            .bars()
            .iter()
            .map(|bar| (bar.index, bar.pci, bar.size, bar.assigned))
            .collect();
        assert_eq!(
            assigned,
            [
                (1, 0, 0x40_0000, true),
                (2, 0x40_0000, 0x1_0000, true),
                (4, 0x4_0000_0000, 0x10_0000, true),
            ]
        );
        // BAR 4: the low half of 0x4_0000_0000 with its 64-bit and
        // prefetchable flags, then the high half.
        assert_eq!(word(&model, at(1, 0, 0), 0x20), 0b1100);
        assert_eq!(word(&model, at(1, 0, 0), 0x24), 4);
        // Memory window 0..=0x4f_ffff, prefetchable 0x4_0000_0000..=0x4_000f_ffff.
        assert_eq!(word(&model, at(0, 0, 0), 0x20), 0x0040_0000);
        assert_eq!(word(&model, at(0, 0, 0), 0x24), 0x0001_0001);
        assert_eq!(word(&model, at(0, 0, 0), 0x28), 4);
        assert_eq!(word(&model, at(0, 0, 0), 0x2c), 4);
    }

    #[test]
    fn a_conventional_bridge_is_scanned_whole_with_its_functions() {
        let mut model = Model {
            functions: vec![
                Modelled::new(at(0, 0, 0), 0x1234, 0x0001, 0x06_0400, 1),
                Modelled::new(at(1, 3, 0), 0x1234, 0x0010, 0x02_0000, 0)
                    .multi_function()
                    .bar(0, 0x2000, false, false),
                Modelled::new(at(1, 3, 2), 0x1234, 0x0011, 0x02_0000, 0)
                    .bar(0, 0x1000, false, false),
            ],
            probed: Vec::new(),
        };
        let found = walk(
            &mut model,
            &[Window {
                pci: 0x1000_0000,
                size: 0x1000_0000,
                prefetchable: false,
            }],
        );
        let functions: Vec<_> = found.iter().map(|device| device.function).collect();
        assert_eq!(functions, [at(0, 0, 0), at(1, 3, 0), at(1, 3, 2)]);
        assert_eq!(found[1].bars()[0].pci, 0x1000_0000);
        assert_eq!(found[2].bars()[0].pci, 0x1000_2000);
    }

    #[test]
    fn a_bar_no_window_holds_stays_unassigned() {
        let mut model = Model {
            functions: vec![
                root_port(0x2711),
                Modelled::new(at(1, 0, 0), 0x1106, 0x3483, 0x0c_0330, 0)
                    .bar(0, 0x8000_0000, false, false),
            ],
            probed: Vec::new(),
        };
        let found = walk(
            &mut model,
            &[Window {
                pci: 0xc000_0000,
                size: 0x4000_0000,
                prefetchable: false,
            }],
        );
        assert!(!found[1].bars()[0].assigned);
        assert_eq!(word(&model, at(1, 0, 0), 0x10), 0);
        // Nothing behind the root port took memory: its window is closed.
        assert_eq!(word(&model, at(0, 0, 0), 0x20), 0x0000_fff0);
    }
}
