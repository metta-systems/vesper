//! Host tests of the bridge's register sequence against a modelled register
//! range, with the Raspberry Pi 4's and 5's windows.

#[cfg(test)]
mod tests {
    use {
        libpci::{ConfigSpace, Function},
        pcie_brcmstb::{
            Board, Bridge, Error, Mmio, Setup, Soc, Window, encode_inbound_size, raise_mip,
            set_reset, set_up_mip,
        },
        std::collections::BTreeMap,
    };

    const PCIE_STATUS: usize = 0x4068;
    const STATUS_UP: u32 = 1 << 7 | 1 << 5 | 1 << 4;
    const STATUS_ENDPOINT_LINK_DOWN: u32 = 0;
    const MDIO_WRITE_DATA: usize = 0x1104;
    const MDIO_READ_DATA: usize = 0x1108;
    /// The SSC status: on, with the PLL locked.
    const SSC_ON_LOCKED: u32 = 1 << 31 | 1 << 11 | 1 << 10;
    const LINK_CONTROL_STATUS: usize = 0xac + 0x10;
    /// 5 GT/s, one lane.
    const GEN2_X1: u32 = 2 << 16 | 1 << 20;

    /// A register range: words written stay; the status, MDIO data and
    /// link words read as the scenario sets them.
    #[derive(Default)]
    struct Model {
        words: BTreeMap<usize, u32>,
        writes: Vec<(usize, u32)>,
        status: u32,
    }

    impl Model {
        fn word(&self, offset: usize) -> u32 {
            self.words.get(&offset).copied().unwrap_or(0)
        }
    }

    impl Mmio for Model {
        fn read(&mut self, offset: usize) -> u32 {
            match offset {
                PCIE_STATUS => self.status,
                // A write's done bit clears at once; a read is done at once.
                MDIO_WRITE_DATA => 0,
                MDIO_READ_DATA => SSC_ON_LOCKED,
                LINK_CONTROL_STATUS => GEN2_X1,
                _ => self.word(offset),
            }
        }

        fn write(&mut self, offset: usize, value: u32) {
            self.words.insert(offset, value);
            self.writes.push((offset, value));
        }
    }

    /// The waits and resets a bring-up asked for.
    #[derive(Default)]
    struct Recorder {
        waited_us: u64,
        events: Vec<&'static str>,
    }

    impl Board for Recorder {
        fn delay_us(&mut self, microseconds: u64) {
            self.waited_us += microseconds;
        }

        fn bridge_reset(&mut self, asserted: bool) {
            self.events
                .push(if asserted { "assert" } else { "release" });
        }

        fn calibrate(&mut self) -> bool {
            self.events.push("calibrate");
            true
        }
    }

    const RPI4_OUTBOUND: [Window; 1] = [Window {
        pci: 0xc000_0000,
        cpu: 0x6_0000_0000,
        size: 0x4000_0000,
    }];
    const RPI4_INBOUND: [Window; 1] = [Window {
        pci: 0,
        cpu: 0,
        size: 0xc000_0000,
    }];

    #[test]
    fn rpi4_bring_up_opens_its_windows_and_msi_target() {
        let mut model = Model {
            status: STATUS_UP,
            ..Model::default()
        };
        let mut board = Recorder::default();
        let mut bridge = Bridge::new(&mut model, Soc::Bcm2711);
        let link = bridge
            .bring_up(
                &Setup {
                    outbound: &RPI4_OUTBOUND,
                    inbound: &RPI4_INBOUND,
                    own_msi: true,
                },
                &mut board,
            )
            .expect("link up");
        assert_eq!((link.speed, link.width), (2, 1));
        assert!(link.spread_spectrum);
        // A 4 GiB inbound window from 0 reaches past 4 GiB: the MSI target
        // goes above it.
        assert_eq!(link.msi_target, Some(0xf_ffff_fffc));
        assert_eq!(model.word(0x4044), 0xffff_fffd);
        assert_eq!(model.word(0x4048), 0xf);
        assert_eq!(model.word(0x404c), 0xffe0_6540);
        // Every MSI vector unmasked and cleared.
        assert_eq!(model.word(0x4514), u32::MAX);
        // Outbound window 0: PCI 0xc000_0000, CPU 0x6000 MiB up to 0x63ff.
        assert_eq!(model.word(0x400c), 0xc000_0000);
        assert_eq!(model.word(0x4010), 0);
        assert_eq!(model.word(0x4070), 0x3ff0_0000);
        assert_eq!(model.word(0x4080), 0x6);
        assert_eq!(model.word(0x4084), 0x6);
        // Inbound window 2: 4 GiB (size code 0x11) at 0; 1 and 3 closed.
        assert_eq!(model.word(0x4034), 0x11);
        assert_eq!(model.word(0x402c), 0);
        assert_eq!(model.word(0x403c), 0);
        // Misc control: SCB0 4 GiB (17), 128-byte bursts, UR mode, SCB
        // access, RCB modes.
        assert_eq!(model.word(0x4008), 17 << 27 | 1 << 13 | 1 << 12 | 1 << 10 | 1 << 7);
        // The root port reports a PCI-to-PCI bridge and advertises L0s and
        // L1.
        assert_eq!(model.word(0x043c), 0x06_0400);
        assert_eq!(model.word(0x04dc), 0b11 << 10);
        // Bridge out of reset, PERST# released; SerDes powered, L1 substates.
        assert_eq!(model.word(0x9210), 0);
        assert_eq!(model.word(0x4204), 1 << 21);
        assert_eq!(model.word(0x9208), 864_000_000);
        // The bridge's own interrupts masked.
        assert_eq!(model.word(0x4310), u32::MAX);
        // 200 µs reset, 200 µs SerDes, 100 ms after PERST#, 2 ms SSC.
        assert_eq!(board.waited_us, 200 + 200 + 100_000 + 2_000);
        assert!(board.events.is_empty());
        // The bridge was held in reset with PERST# asserted, then let go.
        let software_init: Vec<_> = model
            .writes
            .iter()
            .filter(|(offset, _)| *offset == 0x9210)
            .map(|(_, value)| *value)
            .collect();
        assert_eq!(software_init, [0b10, 0b11, 0b01, 0b00]);
    }

    #[test]
    fn rpi5_bring_up_remaps_each_inbound_window_and_leaves_msis_to_the_mip() {
        let mut model = Model {
            status: STATUS_UP,
            ..Model::default()
        };
        let mut board = Recorder::default();
        let inbound = [
            Window {
                pci: 0,
                cpu: 0x1f_0000_0000,
                size: 0x40_0000,
            },
            Window {
                pci: 0x10_0000_0000,
                cpu: 0,
                size: 0x10_0000_0000,
            },
            Window {
                pci: 0xff_ffff_f000,
                cpu: 0x10_0013_0000,
                size: 0x1000,
            },
        ];
        let outbound = [
            Window {
                pci: 0,
                cpu: 0x1f_0000_0000,
                size: 0xffff_fffc,
            },
            Window {
                pci: 0x4_0000_0000,
                cpu: 0x1c_0000_0000,
                size: 0x3_0000_0000,
            },
        ];
        let link = Bridge::new(&mut model, Soc::Bcm2712)
            .bring_up(
                &Setup {
                    outbound: &outbound,
                    inbound: &inbound,
                    own_msi: false,
                },
                &mut board,
            )
            .expect("link up");
        assert!(!link.spread_spectrum);
        assert_eq!(link.msi_target, None);
        assert_eq!(board.events, ["release", "calibrate", "assert", "release"]);
        // Window 1: 4 MiB (code 7) at PCI 0, onto 0x1f_0000_0000.
        assert_eq!(model.word(0x402c), 7);
        assert_eq!(model.word(0x4030), 0);
        assert_eq!(model.word(0x40ac), 1);
        assert_eq!(model.word(0x40b0), 0x1f);
        // Window 2: 64 GiB (code 21) at PCI 0x10_0000_0000, onto RAM.
        assert_eq!(model.word(0x4034), 21);
        assert_eq!(model.word(0x4038), 0x10);
        assert_eq!(model.word(0x40b4), 1);
        // Window 3: the MIP's doorbell page (code 0x1c).
        assert_eq!(model.word(0x403c), 0xffff_f000 | 0x1c);
        assert_eq!(model.word(0x4040), 0xff);
        assert_eq!(model.word(0x40bc), 0x0013_0001);
        assert_eq!(model.word(0x40c0), 0x10);
        // Outbound window 1: CPU 0x1c_0000_0000 is 0x1c000 MiB.
        assert_eq!(model.word(0x4018), 0x4);
        assert_eq!(model.word(0x4074), 0xfff0_0000);
        assert_eq!(model.word(0x4088), 0x1c);
        assert_eq!(model.word(0x408c), 0x1e);
        // PERST# released through PCIE_CTRL; 512-byte bursts; only L1.
        assert_eq!(model.word(0x4064), 1 << 2);
        assert_eq!(model.word(0x4008) >> 20 & 0b11, 2);
        assert_eq!(model.word(0x04dc), 0b10 << 10);
        // Error replies off, the 250 ms bus timeout, the PM clock.
        assert_eq!(model.word(0x40a4), 1 << 19 | 1 << 13);
        assert_eq!(model.word(0x40a8), 0x0b2d_0000);
        assert_eq!(model.word(0x184c), 0x12);
        // The bridge's own MSI controller is left alone.
        assert_eq!(model.word(0x4044), 0);
        assert_eq!(model.word(0x4514), 0);
        // The BCM2712 has no internal bus timer.
        assert_eq!(model.word(0x9208), 0);
    }

    #[test]
    fn a_link_that_never_comes_up_fails_after_the_wait() {
        let mut model = Model {
            status: 1 << 7,
            ..Model::default()
        };
        let mut board = Recorder::default();
        let failed = Bridge::new(&mut model, Soc::Bcm2711).bring_up(
            &Setup {
                outbound: &RPI4_OUTBOUND,
                inbound: &RPI4_INBOUND,
                own_msi: true,
            },
            &mut board,
        );
        assert_eq!(failed, Err(Error::LinkDown));
        assert_eq!(board.waited_us, 200 + 200 + 100_000 + 20 * 5_000);
        let mut model = Model {
            status: STATUS_ENDPOINT_LINK_DOWN,
            ..Model::default()
        };
        let failed = Bridge::new(&mut model, Soc::Bcm2711).bring_up(
            &Setup {
                outbound: &RPI4_OUTBOUND,
                inbound: &RPI4_INBOUND,
                own_msi: true,
            },
            &mut Recorder::default(),
        );
        assert_eq!(failed, Err(Error::NotRootComplex));
    }

    #[test]
    fn configuration_reaches_the_root_port_directly_and_the_rest_through_the_window() {
        let mut model = Model {
            status: STATUS_UP,
            ..Model::default()
        };
        model.words.insert(0x0000, 0x2711_14e4);
        model.words.insert(0x8000, 0x3483_1106);
        let mut bridge = Bridge::new(&mut model, Soc::Bcm2711);
        let mut config = bridge.config();
        let at = |bus, device| Function {
            bus,
            device,
            function: 0,
        };
        assert_eq!(config.read(at(0, 0), 0), Some(0x2711_14e4));
        // Only device 0 sits on the root bus.
        assert_eq!(config.read(at(0, 1), 0), None);
        assert_eq!(config.read(at(1, 0), 0), Some(0x3483_1106));
        assert_eq!(model.word(0x9000), 0x0010_0000);
        // Without a link nothing beyond the root port is touched.
        model.status = 1 << 7;
        let mut bridge = Bridge::new(&mut model, Soc::Bcm2711);
        assert_eq!(bridge.config().read(at(1, 0), 0), None);
    }

    #[test]
    fn inbound_sizes_encode_as_linux_does() {
        assert_eq!(encode_inbound_size(0x1000).value(), 0x1c);
        assert_eq!(encode_inbound_size(0x8000).value(), 0x1f);
        assert_eq!(encode_inbound_size(0x1_0000).value(), 1);
        assert_eq!(encode_inbound_size(0x1_0000_0000).value(), 17);
        assert_eq!(encode_inbound_size(0x10_0000_0000).value(), 21);
        assert_eq!(encode_inbound_size(0x800).value(), 0);
        assert_eq!(encode_inbound_size(0xc000_0000).value(), 0);
    }

    #[test]
    fn mip_and_reset_controller_words() {
        let mut mip = Model::default();
        set_up_mip(&mut mip);
        assert_eq!(
            mip.writes,
            [
                (0x40, 0),
                (0x50, 0),
                (0x60, u32::MAX),
                (0x70, u32::MAX),
                (0x20, u32::MAX),
                (0x30, u32::MAX),
            ]
        );
        raise_mip(&mut mip, 3);
        assert_eq!(mip.word(0x00), 3);
        // pcie2's bridge reset, 0x2c: bank 1, bit 12.
        let mut resets = Model::default();
        set_reset(&mut resets, 0x2c, true);
        set_reset(&mut resets, 0x2c, false);
        assert_eq!(resets.writes, [(0x18, 1 << 12), (0x1c, 1 << 12)]);
    }
}
