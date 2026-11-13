//! What a boot program finds on the board before any driver runs: which board
//! it is, the UART for its console, the firmware mailbox and the activity LED.
//!
//! Everything comes from the device tree the firmware passes, so one image
//! boots on every board it is built for.

use crate::{
    DeviceTree, FallibleIterator, Node, PropReader, dma_address, fixed_clock_rate, is_compatible,
    is_enabled, node_by_phandle, property, regions,
};

/// The Broadcom system on a chip, from the root node's `compatible` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Soc {
    /// The Raspberry Pi 3 and Zero 2 W
    Bcm2837,
    /// The Raspberry Pi 4, 400 and Compute Module 4
    Bcm2711,
    /// The Raspberry Pi 5, 500 and Compute Module 5
    Bcm2712,
}

impl Soc {
    const COMPATIBLES: [(&str, Self); 3] = [
        ("brcm,bcm2837", Self::Bcm2837),
        ("brcm,bcm2711", Self::Bcm2711),
        ("brcm,bcm2712", Self::Bcm2712),
    ];

    /// The `SoC` a root `compatible` entry names, if it is one of these.
    #[must_use]
    pub fn from_compatible(compatible: &str) -> Option<Self> {
        Self::COMPATIBLES
            .iter()
            .find_map(|(name, soc)| (*name == compatible).then_some(*soc))
    }

    /// The `SoC` whose cores have the part number `part` (`MIDR_EL1.PartNum`,
    /// Arm's numbers): the Cortex-A53, A72 and A76. Only for when the device
    /// tree cannot be read: other boards have these cores too.
    #[must_use]
    pub const fn from_cpu_part(part: u64) -> Option<Self> {
        match part {
            0xd03 => Some(Self::Bcm2837),
            0xd08 => Some(Self::Bcm2711),
            0xd0b => Some(Self::Bcm2712),
            _ => None,
        }
    }

    /// The boot console hardware of the `SoC`'s boards, as their device trees
    /// give it; for when the device tree cannot be read (the host tests check
    /// it against the trees in `targets/`).
    #[must_use]
    pub const fn fallback_boot_hardware(self) -> BootHardware {
        /// The firmware's uncached bus alias on the BCM2837 and BCM2711.
        const BUS_ALIAS: u64 = 0xc000_0000;
        match self {
            Self::Bcm2837 => BootHardware {
                uart: ConsoleUart {
                    base: 0x3f20_1000,
                    clock: UartClock::Firmware,
                },
                header_gpio: Some(Gpio {
                    base: 0x3f20_0000,
                    kind: GpioKind::Bcm2835,
                }),
                mailbox: Some(Mailbox {
                    base: 0x3f00_b880,
                    bus_offset: BUS_ALIAS,
                }),
            },
            Self::Bcm2711 => BootHardware {
                uart: ConsoleUart {
                    base: 0xfe20_1000,
                    clock: UartClock::Firmware,
                },
                header_gpio: Some(Gpio {
                    base: 0xfe20_0000,
                    kind: GpioKind::Bcm2711,
                }),
                mailbox: Some(Mailbox {
                    base: 0xfe00_b880,
                    bus_offset: BUS_ALIAS,
                }),
            },
            Self::Bcm2712 => BootHardware {
                uart: ConsoleUart {
                    base: 0x10_7d00_1000,
                    clock: UartClock::Fixed(9_216_000),
                },
                header_gpio: None,
                mailbox: Some(Mailbox {
                    base: 0x10_7c01_3880,
                    bus_offset: 0,
                }),
            },
        }
    }
}

/// What the boot console needs: the UART, the GPIO block that routes the
/// header pins to it where the board has one, and the firmware mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootHardware {
    pub uart: ConsoleUart,
    pub header_gpio: Option<Gpio>,
    pub mailbox: Option<Mailbox>,
}

/// The board, as its device tree describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Board<'dt> {
    /// The root `model`, e.g. "Raspberry Pi 4 Model B Rev 1.4"; for people to
    /// read, never to match on.
    pub model: Option<&'dt str>,
    /// The root's most specific `compatible` entry, e.g. "raspberrypi,4-model-b".
    pub compatible: Option<&'dt str>,
    /// The `SoC` named in the root `compatible` list.
    pub soc: Option<Soc>,
    /// The board revision code the firmware writes into `/system/linux,revision`.
    pub revision: Option<u32>,
}

/// The two GPIO blocks with the same function select and set/clear registers
/// and a different pull control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioKind {
    /// `brcm,bcm2835-gpio`: the pull is clocked in through `GPPUD` and `GPPUDCLKn`.
    Bcm2835,
    /// `brcm,bcm2711-gpio`: the pull is set in `GPIO_PUP_PDN_CNTRL_REGn`.
    Bcm2711,
}

/// A Broadcom GPIO block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gpio {
    /// The CPU physical address of its registers
    pub base: u64,
    pub kind: GpioKind,
}

impl GpioKind {
    /// Checked in this order: a BCM2711 tree names only its own block.
    const COMPATIBLES: [(&str, Self); 2] = [
        ("brcm,bcm2711-gpio", Self::Bcm2711),
        ("brcm,bcm2835-gpio", Self::Bcm2835),
    ];

    fn of(node: &Node<'_, '_, '_>) -> Option<Self> {
        Self::COMPATIBLES
            .iter()
            .find_map(|(compatible, kind)| is_compatible(node, compatible).then_some(*kind))
    }
}

/// Where a UART's reference clock comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartClock {
    /// A `fixed-clock` node running at this many Hz.
    Fixed(u32),
    /// A clock the firmware runs and reports through the mailbox.
    Firmware,
}

/// The PL011 the boot programs talk to the host over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleUart {
    /// The CPU physical address of its registers
    pub base: u64,
    pub clock: UartClock,
}

/// The `VideoCore` firmware's mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mailbox {
    /// The CPU physical address of its registers
    pub base: u64,
    /// What the firmware adds to a CPU physical address in low memory to reach
    /// it, from the buses' `dma-ranges` (the uncached alias on the BCM2837 and
    /// BCM2711, nothing on the BCM2712).
    pub bus_offset: u64,
}

/// The firmware numbers the GPIOs it owns from here (`raspberrypi,firmware-gpio`).
pub const FIRMWARE_GPIO_BASE: u32 = 128;

/// How the activity LED is switched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityLed {
    /// A pin of a Broadcom GPIO block.
    Gpio {
        gpio: Gpio,
        pin: u32,
        active_low: bool,
    },
    /// A GPIO only the firmware reaches (the Raspberry Pi 3B's expander), by
    /// its firmware number, for the mailbox's `SET_GPIO_STATE`.
    Firmware { pin: u32, active_low: bool },
}

/// The GPIO flag cell's active-low bit (`GPIO_ACTIVE_LOW`).
const GPIO_ACTIVE_LOW: u32 = 1;

/// The node a `/chosen/stdout-path` names: a path, or an alias, each optionally
/// followed by `:` and the line's options.
fn stdout_node<'a, 'i: 'a, 'dt: 'i>(tree: &'a DeviceTree<'i, 'dt>) -> Option<Node<'a, 'i, 'dt>> {
    let stdout_path = tree.property("/chosen/stdout-path")?.str().ok()?;
    let path = stdout_path.split(':').next()?;
    if path.starts_with('/') {
        return tree.node(path);
    }
    let alias = property(&tree.node("/aliases")?, path)?.str().ok()?;
    tree.node(alias)
}

/// Whether `node` is an enabled PL011 with registers the CPU reaches.
fn is_usable_pl011(node: &Node<'_, '_, '_>) -> bool {
    is_compatible(node, "arm,pl011") && is_enabled(node) && regions(node).next().is_some()
}

impl<'i, 'dt> DeviceTree<'i, 'dt> {
    /// Which board this is.
    pub fn board(&self) -> Board<'dt> {
        let root = self.index().root();
        let compatible = property(&root, "compatible");
        let soc = compatible.as_ref().and_then(|prop| {
            prop.iter_str()
                .find_map(|entry| Ok(Soc::from_compatible(entry)))
                .ok()
                .flatten()
        });
        Board {
            model: self.model(),
            compatible: compatible.and_then(|prop| prop.str().ok()),
            soc,
            revision: self
                .property("/system/linux,revision")
                .and_then(|prop| prop.u32(0).ok()),
        }
    }

    /// The PL011 for the boot console: the one `/chosen/stdout-path` names, or
    /// else the first enabled one. The Raspberry Pi 3 and 4 trees point
    /// `stdout-path`, when they have it, at the mini UART, and their first
    /// PL011 is the one that can be routed to the header's pins 14 and 15; the
    /// Raspberry Pi 5 tree points it at the PL011 on the debug connector.
    pub fn console_uart(&self) -> Option<ConsoleUart> {
        let node = stdout_node(self)
            .filter(is_usable_pl011)
            .or_else(|| self.index().nodes().find(is_usable_pl011))?;
        Some(ConsoleUart {
            base: regions(&node).next()?.start,
            clock: fixed_clock_rate(self.index(), &node)
                .map_or(UartClock::Firmware, UartClock::Fixed),
        })
    }

    /// What the boot console needs, if the tree names a console UART.
    pub fn boot_hardware(&self) -> Option<BootHardware> {
        Some(BootHardware {
            uart: self.console_uart()?,
            header_gpio: self.header_gpio(),
            mailbox: self.firmware_mailbox(),
        })
    }

    /// The enabled Broadcom GPIO block that carries the header pins, if the `SoC`
    /// has one of the two kinds (the Raspberry Pi 5's header pins are on RP1).
    pub fn header_gpio(&self) -> Option<Gpio> {
        self.index().nodes().filter(is_enabled).find_map(|node| {
            Some(Gpio {
                kind: GpioKind::of(&node)?,
                base: regions(&node).next()?.start,
            })
        })
    }

    /// The firmware's mailbox (`brcm,bcm2835-mbox`).
    pub fn firmware_mailbox(&self) -> Option<Mailbox> {
        let node = self
            .index()
            .nodes()
            .find(|node| is_compatible(node, "brcm,bcm2835-mbox") && is_enabled(node))?;
        Some(Mailbox {
            base: regions(&node).next()?.start,
            bus_offset: dma_address(&node, 0, 1)?,
        })
    }

    /// The activity LED: the `act` (or `led-act`) node under `/leds`, when its
    /// GPIO is on a Broadcom GPIO block or the firmware's.
    pub fn activity_led(&self) -> Option<ActivityLed> {
        let led = self
            .node("/leds/act")
            .or_else(|| self.node("/leds/led-act"))?;
        let gpios = property(&led, "gpios")?;
        let controller = node_by_phandle(self.index(), gpios.u32(0).ok()?)?;
        let pin = gpios.u32(1).ok()?;
        let active_low = gpios.u32(2).is_ok_and(|flags| flags & GPIO_ACTIVE_LOW != 0);
        if is_compatible(&controller, "raspberrypi,firmware-gpio") {
            return Some(ActivityLed::Firmware {
                pin: FIRMWARE_GPIO_BASE + pin,
                active_low,
            });
        }
        Some(ActivityLed::Gpio {
            gpio: Gpio {
                kind: GpioKind::of(&controller)?,
                base: regions(&controller).next()?.start,
            },
            pin,
            active_low,
        })
    }
}
