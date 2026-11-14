#![no_std]

//! Register logic of the Broadcom STB PCIe root complex: the Raspberry Pi
//! 4's (`brcm,bcm2711-pcie`) and the Raspberry Pi 5's (`brcm,bcm2712-pcie`),
//! with the BCM2712's MSI controller (`brcm,bcm2712-mip`) and the reset
//! controllers its bridge needs.
//!
//! [`Bridge::bring_up`] resets the bridge, opens its inbound windows (PCI
//! addresses that reach the system's memory) and outbound windows (CPU
//! addresses that reach PCI), sets up its own MSI controller where it serves
//! its MSIs, releases the fundamental reset and waits for the link. Then
//! [`Bridge::config`] reaches every function's configuration space for
//! `libpci`'s enumeration: the root port's directly in the bridge's
//! registers, the others through the index and data window.
//!
//! Broadcom publishes no documentation of these blocks. The registers and
//! the sequence follow Linux `drivers/pci/controller/pcie-brcmstb.c`,
//! `drivers/irqchip/irq-bcm2712-mip.c`, `drivers/reset/reset-brcmstb.c` and
//! `drivers/reset/reset-brcmstb-rescal.c` (Raspberry Pi `rpi-6.12.y`), and
//! Circle's `lib/bcmpciehostbridge.cpp`. Every register is a 32-bit word.

use {
    arbitrary_int::{u2, u4, u5, u6, u12, u20, u24, u30},
    bitbybit::bitfield,
    core::marker::PhantomData,
    libpci::{ConfigSpace, Function},
};

/// 32-bit register access to one register range.
pub trait Mmio {
    fn read(&mut self, offset: usize) -> u32;
    fn write(&mut self, offset: usize, value: u32);
}

impl<Inner: Mmio> Mmio for &mut Inner {
    fn read(&mut self, offset: usize) -> u32 {
        (**self).read(offset)
    }

    fn write(&mut self, offset: usize, value: u32) {
        (**self).write(offset, value);
    }
}

/// The contents of a register: a bitfield, or a plain word.
pub trait Word: Copy {
    fn from_raw(raw: u32) -> Self;
    fn raw(self) -> u32;
}

impl Word for u32 {
    fn from_raw(raw: u32) -> Self {
        raw
    }

    fn raw(self) -> u32 {
        self
    }
}

macro_rules! words {
    ($($kind:ty),* $(,)?) => {
        $(impl Word for $kind {
            fn from_raw(raw: u32) -> Self {
                Self::new_with_raw_value(raw)
            }

            fn raw(self) -> u32 {
                self.raw_value()
            }
        })*
    };
}

/// A register holding a `T`, at its offset in its range.
pub struct Register<T> {
    offset: usize,
    kind: PhantomData<T>,
}

impl<T> Clone for Register<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Register<T> {}

impl<T: Word> Register<T> {
    pub const fn at(offset: usize) -> Self {
        Self {
            offset,
            kind: PhantomData,
        }
    }

    pub const fn offset(self) -> usize {
        self.offset
    }

    pub fn read(self, mmio: &mut impl Mmio) -> T {
        T::from_raw(mmio.read(self.offset))
    }

    pub fn write(self, mmio: &mut impl Mmio, value: T) {
        mmio.write(self.offset, value.raw());
    }

    pub fn modify(self, mmio: &mut impl Mmio, change: impl FnOnce(T) -> T) {
        let value = change(self.read(mmio));
        self.write(mmio, value);
    }
}

// ── The root complex's own configuration registers ────────────────────────

/// `PCIE_RC_CFG_VENDOR_VENDOR_SPECIFIC_REG1`.
#[bitfield(u32, default = 0, debug)]
pub struct VendorSpecific1 {
    /// Byte order of inbound window 2: zero for little endian
    #[bits(2..=3, rw)]
    endian_mode_bar2: u2,
}

/// `PCIE_RC_CFG_PRIV1_ID_VAL3`: the class code the root port reports.
#[bitfield(u32, default = 0, debug)]
pub struct IdValue3 {
    #[bits(0..=23, rw)]
    class_code: u24,
}

/// `PCIE_RC_CFG_PRIV1_LINK_CAPABILITY`: what the link capability register
/// advertises.
#[bitfield(u32, default = 0, debug)]
pub struct LinkCapability {
    #[bits(0..=3, rw)]
    max_link_speed: u4,
    /// Bit 0 L0s, bit 1 L1
    #[bits(10..=11, rw)]
    aspm_support: u2,
}

/// ASPM states the root port advertises.
const ASPM_L0S: u2 = u2::new(0b01);
const ASPM_L1: u2 = u2::new(0b10);

/// The PCI Express capability's link control and status (§7.5.3.7,
/// §7.5.3.8 of the PCI Express Base Specification 4.0).
#[bitfield(u32, default = 0, debug)]
pub struct LinkControlStatus {
    #[bits(16..=19, r)]
    current_speed: u4,
    #[bits(20..=25, r)]
    width: u6,
}

/// The PCI Express capability's root control and root capabilities
/// (§7.5.3.12, §7.5.3.13).
#[bitfield(u32, default = 0, debug)]
pub struct RootControl {
    /// Configuration Request Retry Status software visibility enable
    #[bit(4, rw)]
    crs_visibility_enable: bool,
    #[bit(16, r)]
    crs_visibility: bool,
}

/// `PCIE_RC_DL_MDIO_ADDR`: an MDIO packet to the PHY.
#[bitfield(u32, default = 0, debug)]
pub struct MdioPacket {
    #[bits(0..=15, rw)]
    register: u16,
    #[bits(16..=19, rw)]
    port: u4,
    /// 1: read, 0: write
    #[bit(20, rw)]
    read: bool,
}

/// `PCIE_RC_DL_MDIO_WR_DATA` and `PCIE_RC_DL_MDIO_RD_DATA`.
#[bitfield(u32, default = 0, debug)]
pub struct MdioData {
    #[bits(0..=30, rw)]
    data: arbitrary_int::u31,
    /// Write: set to start, clears when done; read: set when done
    #[bit(31, rw)]
    done: bool,
}

/// `PCIE_RC_PL_PHY_CTL_15`.
#[bitfield(u32, default = 0, debug)]
pub struct PhyControl15 {
    /// The PM clock's period in nanoseconds, rounded down
    #[bits(0..=7, rw)]
    pm_clock_period: u8,
}

// ── The bridge's own registers ────────────────────────────────────────────

/// `PCIE_MISC_MISC_CTRL`.
#[bitfield(u32, default = 0, debug)]
pub struct MiscControl {
    #[bit(7, rw)]
    rcb_64b_mode: bool,
    #[bit(10, rw)]
    rcb_mps_mode: bool,
    #[bit(12, rw)]
    scb_access: bool,
    /// Configuration reads of absent functions answer all ones instead of
    /// an abort
    #[bit(13, rw)]
    config_read_ur_mode: bool,
    #[bits(20..=21, rw)]
    max_burst_size: u2,
    /// Memory controller 0's window, as log2 of its bytes minus 15
    #[bits(27..=31, rw)]
    scb0_size: u5,
}

/// An inbound window's PCI base and size (`PCIE_MISC_RC_BAR<n>_CONFIG_LO`).
#[bitfield(u32, default = 0, debug)]
pub struct InboundLow {
    /// [`encode_inbound_size`]; zero closes the window
    #[bits(0..=4, rw)]
    size: u5,
    /// PCI address bits 31:12
    #[bits(12..=31, rw)]
    base: u20,
}

/// Where an inbound window lands on the system bus
/// (`PCIE_MISC_UBUS_BAR<n>_CONFIG_REMAP`, BCM2712 only).
#[bitfield(u32, default = 0, debug)]
pub struct InboundRemap {
    #[bit(0, rw)]
    access_enable: bool,
    /// CPU address bits 31:12
    #[bits(12..=31, rw)]
    base: u20,
}

/// `PCIE_MISC_MSI_BAR_CONFIG_LO`: bit 0 enables the MSI target.
#[bitfield(u32, default = 0, debug)]
pub struct MsiTargetLow {
    #[bit(0, rw)]
    enable: bool,
    /// Address bits 31:2
    #[bits(2..=31, rw)]
    address: u30,
}

/// `PCIE_MISC_MSI_DATA_CONFIG`: which MSI data words the controller
/// catches, the vector in the bits the mask leaves out.
#[bitfield(u32, default = 0, debug)]
pub struct MsiDataMatch {
    #[bits(0..=15, rw)]
    pattern: u16,
    #[bits(16..=31, rw)]
    mask: u16,
}

/// The MSI data pattern Linux uses, and its mask for 32 vectors
/// (`PCIE_MISC_MSI_DATA_CONFIG_VAL_32`).
pub const MSI_DATA_PATTERN: u16 = 0x6540;
const MSI_DATA_MASK_32: u16 = 0xffe0;

/// Vectors of the bridge's own MSI controller.
pub const OWN_MSI_VECTORS: u32 = 32;

/// `PCIE_MISC_PCIE_CTRL` (BCM2712).
#[bitfield(u32, default = 0, debug)]
pub struct PcieControl {
    /// The fundamental reset, active low: set to release it
    #[bit(2, rw)]
    perst_released: bool,
}

/// `PCIE_MISC_PCIE_STATUS`.
#[bitfield(u32, default = 0, debug)]
pub struct PcieStatus {
    #[bit(4, r)]
    phy_link_up: bool,
    #[bit(5, r)]
    data_link_active: bool,
    #[bit(7, r)]
    root_complex: bool,
}

impl PcieStatus {
    const fn link_up(self) -> bool {
        self.phy_link_up() && self.data_link_active()
    }
}

/// An outbound window's CPU base and limit in megabytes, low bits
/// (`PCIE_MISC_CPU_2_PCIE_MEM_WIN<n>_BASE_LIMIT`).
#[bitfield(u32, default = 0, debug)]
pub struct OutboundBaseLimit {
    #[bits(4..=15, rw)]
    base: u12,
    #[bits(20..=31, rw)]
    limit: u12,
}

/// The high bits of an outbound window's base or limit in megabytes
/// (`..._MEM_WIN<n>_BASE_HI`, `..._LIMIT_HI`).
#[bitfield(u32, default = 0, debug)]
pub struct OutboundHigh {
    #[bits(0..=7, rw)]
    megabytes: u8,
}

/// `PCIE_MISC_CTRL_1` (BCM2712).
#[bitfield(u32, default = 0, debug)]
pub struct MiscControl1 {
    /// Take QoS from received vendor-defined messages
    #[bit(5, rw)]
    vdm_qos_control: bool,
}

/// `PCIE_MISC_UBUS_CTRL` (BCM2712).
#[bitfield(u32, default = 0, debug)]
pub struct UbusControl {
    #[bit(13, rw)]
    reply_error_disable: bool,
    #[bit(19, rw)]
    reply_decode_error_disable: bool,
}

/// `PCIE_MISC_AXI_INTF_CTRL` (BCM2712).
#[bitfield(u32, default = 0, debug)]
pub struct AxiInterface {
    #[bits(0..=5, rw)]
    max_outstanding: u6,
    #[bit(7, rw)]
    request_fifo_qos_propagation: bool,
    #[bit(11, rw)]
    disable_qos_gating: bool,
    #[bit(12, rw)]
    qos_update_timing_fix: bool,
    #[bit(13, rw)]
    rclk_qos_array_fix: bool,
}

/// `PCIE_MISC_HARD_PCIE_HARD_DEBUG`.
#[bitfield(u32, default = 0, debug)]
pub struct HardDebug {
    #[bit(1, rw)]
    clkreq_debug_enable: bool,
    #[bit(3, rw)]
    perst_assert: bool,
    #[bit(16, rw)]
    refclk_override_enable: bool,
    #[bit(20, rw)]
    refclk_override_out: bool,
    #[bit(21, rw)]
    l1ss_enable: bool,
    /// The SerDes powered down
    #[bit(27, rw)]
    serdes_iddq: bool,
}

/// `PCIE_RGR1_SW_INIT_1` (BCM2711).
#[bitfield(u32, default = 0, debug)]
pub struct SoftwareInit {
    /// The fundamental reset asserted
    #[bit(0, rw)]
    perst: bool,
    /// The bridge held in reset
    #[bit(1, rw)]
    bridge: bool,
}

/// `PCIE_EXT_CFG_INDEX`: the function the data window reaches.
#[bitfield(u32, default = 0, debug)]
pub struct ConfigIndex {
    #[bits(12..=14, rw)]
    function: arbitrary_int::u3,
    #[bits(15..=19, rw)]
    device: u5,
    #[bits(20..=27, rw)]
    bus: u8,
}

words!(
    VendorSpecific1,
    IdValue3,
    LinkCapability,
    LinkControlStatus,
    RootControl,
    MdioPacket,
    MdioData,
    PhyControl15,
    MiscControl,
    InboundLow,
    InboundRemap,
    MsiTargetLow,
    MsiDataMatch,
    PcieControl,
    PcieStatus,
    OutboundBaseLimit,
    OutboundHigh,
    MiscControl1,
    UbusControl,
    AxiInterface,
    HardDebug,
    SoftwareInit,
    ConfigIndex,
);

/// The root complex's configuration registers.
const VENDOR_SPECIFIC_1: Register<VendorSpecific1> = Register::at(0x0188);
const ID_VALUE_3: Register<IdValue3> = Register::at(0x043c);
const LINK_CAPABILITY: Register<LinkCapability> = Register::at(0x04dc);
/// Where the root port's PCI Express capability lies.
const EXPRESS_CAPABILITY: usize = 0x00ac;
const LINK_CONTROL_STATUS: Register<LinkControlStatus> = Register::at(EXPRESS_CAPABILITY + 0x10);
const ROOT_CONTROL: Register<RootControl> = Register::at(EXPRESS_CAPABILITY + 0x1c);
const MDIO_ADDRESS: Register<MdioPacket> = Register::at(0x1100);
const MDIO_WRITE_DATA: Register<MdioData> = Register::at(0x1104);
const MDIO_READ_DATA: Register<MdioData> = Register::at(0x1108);
const PHY_CONTROL_15: Register<PhyControl15> = Register::at(0x184c);

/// The bridge's registers.
const MISC_CONTROL: Register<MiscControl> = Register::at(0x4008);
const MSI_TARGET_LOW: Register<MsiTargetLow> = Register::at(0x4044);
const MSI_TARGET_HIGH: Register<u32> = Register::at(0x4048);
const MSI_DATA_MATCH: Register<MsiDataMatch> = Register::at(0x404c);
const CONFIG_RETRY_TIMEOUT: Register<u32> = Register::at(0x405c);
const PCIE_CONTROL: Register<PcieControl> = Register::at(0x4064);
const PCIE_STATUS: Register<PcieStatus> = Register::at(0x4068);
const REVISION: Register<u32> = Register::at(0x406c);
const MISC_CONTROL_1: Register<MiscControl1> = Register::at(0x40a0);
const UBUS_CONTROL: Register<UbusControl> = Register::at(0x40a4);
const UBUS_TIMEOUT: Register<u32> = Register::at(0x40a8);
const AXI_INTERFACE: Register<AxiInterface> = Register::at(0x416c);
const AXI_READ_ERROR_DATA: Register<u32> = Register::at(0x4170);
const CONFIG_DATA: usize = 0x8000;
const CONFIG_INDEX: Register<ConfigIndex> = Register::at(0x9000);
/// The BCM2711's internal bus timeout, two words before its software init.
const RBUS_TIMEOUT: Register<u32> = Register::at(0x9208);
const SOFTWARE_INIT: Register<SoftwareInit> = Register::at(0x9210);

/// The bridge's own MSI controller's interrupt block (a Broadcom level 2
/// interrupt controller: status, set, clear, mask status, mask set, mask
/// clear).
const MSI_INTERRUPTS: usize = 0x4500;
const L2_STATUS: usize = 0x00;
const L2_SET: usize = 0x04;
const L2_CLEAR: usize = 0x08;
const L2_MASK_SET: usize = 0x10;
const L2_MASK_CLEAR: usize = 0x14;

/// Outbound window `index`'s registers.
const fn outbound_pci_low(index: usize) -> Register<u32> {
    Register::at(0x400c + 8 * index)
}
const fn outbound_pci_high(index: usize) -> Register<u32> {
    Register::at(0x4010 + 8 * index)
}
const fn outbound_base_limit(index: usize) -> Register<OutboundBaseLimit> {
    Register::at(0x4070 + 4 * index)
}
const fn outbound_base_high(index: usize) -> Register<OutboundHigh> {
    Register::at(0x4080 + 8 * index)
}
const fn outbound_limit_high(index: usize) -> Register<OutboundHigh> {
    Register::at(0x4084 + 8 * index)
}

/// Inbound window `number`'s registers, counted from 1 as the hardware
/// does.
const fn inbound_low(number: usize) -> Register<InboundLow> {
    Register::at(if number <= 3 {
        0x402c + 8 * (number - 1)
    } else {
        0x40d4 + 8 * (number - 4)
    })
}
const fn inbound_high(number: usize) -> Register<u32> {
    Register::at(inbound_low(number).offset() + 4)
}
const fn inbound_remap_low(number: usize) -> Register<InboundRemap> {
    Register::at(if number <= 3 {
        0x40ac + 8 * (number - 1)
    } else {
        0x410c + 8 * (number - 4)
    })
}
const fn inbound_remap_high(number: usize) -> Register<u32> {
    Register::at(inbound_remap_low(number).offset() + 4)
}

/// Outbound windows the bridge has.
pub const OUTBOUND_WINDOWS: usize = 4;
/// Inbound windows the BCM2712 has (window 1 up).
pub const BCM2712_INBOUND_WINDOWS: usize = 10;

/// `PCIE_MISC_MISC_CTRL` burst size codes.
const BURST_128: u2 = u2::new(0);
const BURST_512: u2 = u2::new(2);

/// The class code of a PCI-to-PCI bridge, which the root port is made to
/// report (it resets to an endpoint's).
const CLASS_BRIDGE: u24 = u24::new(0x06_0400);

/// BCM2712 timeouts, in 750 MHz clocks: the bus 250 ms, configuration
/// retries a little less, so a retried request ends by the retry timeout.
const UBUS_TIMEOUT_CLOCKS: u32 = 0x0b2d_0000;
const CONFIG_RETRY_TIMEOUT_CLOCKS: u32 = 0x0aba_0000;
/// The BCM2711's internal bus timeout: 4 s in 216 MHz clocks, long enough
/// for L1 substates.
const RBUS_TIMEOUT_CLOCKS: u32 = 216 * 4_000_000;
/// What a failed read on the BCM2712's bus returns.
const READ_ERROR_DATA: u32 = u32::MAX;
/// Outstanding AXI requests when the QoS timing fix is missing.
const THROTTLED_OUTSTANDING: u6 = u6::new(15);

/// The BCM2712 PHY's PLL set up for the 54 MHz crystal reference: the MDIO
/// block address, then `(register, value)` pairs (Linux
/// `brcm_pcie_post_setup_bcm2712`).
const PLL_54MHZ_BLOCK: u16 = 0x1600;
const PLL_54MHZ: [(u16, u16); 7] = [
    (0x16, 0x50b9),
    (0x17, 0xbda1),
    (0x18, 0x0094),
    (0x19, 0x97b4),
    (0x1b, 0x5030),
    (0x1c, 0x5030),
    (0x1e, 0x0007),
];
/// The PM clock's period with the 54 MHz reference: 18.52 ns.
const PM_CLOCK_PERIOD_54MHZ: u8 = 0x12;

/// MDIO: the register that selects a register block.
const MDIO_BLOCK_SELECT: u16 = 0x1f;
/// The spread spectrum clocking block and its registers.
const SSC_BLOCK: u16 = 0x1100;
const SSC_CONTROL: u16 = 0x2;
const SSC_STATUS: u16 = 0x1;
const SSC_OVERRIDE_ENABLE: u32 = 1 << 15;
const SSC_OVERRIDE_VALUE: u32 = 1 << 14;
const SSC_STATUS_ON: u32 = 1 << 10;
const SSC_STATUS_PLL_LOCK: u32 = 1 << 11;
/// MDIO polls before a transfer counts as lost: 10 µs apart, as Linux.
const MDIO_POLLS: u32 = 10;

/// Where an MSI to the bridge's own controller goes: as low as the
/// inbound window allows (Linux `BRCM_MSI_TARGET_ADDR_LT_4GB`,
/// `..._GT_4GB`).
const MSI_TARGET_BELOW_4G: u64 = 0x0_ffff_fffc;
const MSI_TARGET_ABOVE_4G: u64 = 0xf_ffff_fffc;

/// Waits: the bridge's reset, the SerDes settling, and the link after the
/// fundamental reset (PCI Express CEM §2.2: 100 ms, then up to 100 ms more).
const RESET_US: u64 = 200;
const SERDES_US: u64 = 200;
const PERST_US: u64 = 100_000;
const LINK_POLL_US: u64 = 5_000;
const LINK_POLLS: u32 = 20;
const SSC_US: u64 = 2_000;

/// Which bridge it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Soc {
    Bcm2711,
    Bcm2712,
}

impl Soc {
    /// Where its PCIe interrupt and debug registers are.
    const fn hard_debug(self) -> Register<HardDebug> {
        match self {
            Self::Bcm2711 => Register::at(0x4204),
            Self::Bcm2712 => Register::at(0x4304),
        }
    }

    const fn cpu_interrupts(self) -> usize {
        match self {
            Self::Bcm2711 => 0x4300,
            Self::Bcm2712 => 0x4400,
        }
    }
}

/// One window, as the bridge's node describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub pci: u64,
    pub cpu: u64,
    pub size: u64,
}

/// What [`Bridge::bring_up`] needs to know.
#[derive(Debug, Clone, Copy)]
pub struct Setup<'windows> {
    /// Outbound: CPU addresses that reach PCI
    pub outbound: &'windows [Window],
    /// Inbound: PCI addresses that reach the CPU's side
    pub inbound: &'windows [Window],
    /// Whether the bridge serves its own MSIs
    pub own_msi: bool,
}

/// What the board does for the bridge: waits, and the BCM2712's resets,
/// which other blocks hold.
pub trait Board {
    fn delay_us(&mut self, microseconds: u64);
    /// Assert or release the bridge's reset (BCM2712: `brcmstb-reset`).
    fn bridge_reset(&mut self, asserted: bool);
    /// Calibrate the PCIe PHY's resistors (BCM2712: the rescal block);
    /// whether it finished.
    fn calibrate(&mut self) -> bool;
}

/// Why the bridge did not come up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// It reports endpoint mode
    NotRootComplex,
    /// No link within the wait
    LinkDown,
    /// An inbound window is no power of two, or misaligned
    BadInbound,
    /// More windows than the bridge has
    TooManyWindows,
    /// The PHY did not answer over MDIO
    Mdio,
    /// The resistor calibration did not finish
    Calibration,
}

/// The link the bridge brought up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// The link speed code: 1 2.5 GT/s, 2 5 GT/s, 3 8 GT/s
    pub speed: u8,
    pub width: u8,
    /// Whether spread spectrum clocking is on
    pub spread_spectrum: bool,
    /// `PCIE_MISC_REVISION`
    pub revision: u32,
    /// Where devices send MSIs to the bridge's own controller
    pub msi_target: Option<u64>,
}

/// The inbound window size code (`brcm_pcie_encode_ibar_size`): 4 KiB to
/// 32 KiB as 0x1c to 0x1f, 64 KiB to 64 GiB as 1 to 21; zero for anything
/// else.
pub fn encode_inbound_size(size: u64) -> u5 {
    if !size.is_power_of_two() {
        return u5::new(0);
    }
    let log2 = size.trailing_zeros();
    let code = match log2 {
        12..=15 => log2 - 12 + 0x1c,
        16..=36 => log2 - 15,
        _ => 0,
    };
    u5::new(u8::try_from(code).unwrap_or(0))
}

/// The low and high halves of a 64-bit address.
const fn halves(address: u64) -> (u32, u32) {
    let bytes = address.to_le_bytes();
    (
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    )
}

/// Address bits 31:12 of `address`.
fn page_bits(address: u64) -> u20 {
    u20::new(halves(address).0 >> 12)
}

/// A Broadcom STB PCIe root complex.
pub struct Bridge<Registers> {
    registers: Registers,
    soc: Soc,
}

impl<Registers: Mmio> Bridge<Registers> {
    pub const fn new(registers: Registers, soc: Soc) -> Self {
        Self { registers, soc }
    }

    pub const fn soc(&self) -> Soc {
        self.soc
    }

    /// Hold the bridge in reset, or let it go.
    fn reset_bridge(&mut self, asserted: bool, board: &mut impl Board) {
        match self.soc {
            Soc::Bcm2711 => SOFTWARE_INIT.modify(&mut self.registers, |value| {
                value.with_bridge(asserted)
            }),
            Soc::Bcm2712 => board.bridge_reset(asserted),
        }
    }

    /// Assert or release the fundamental reset (PERST#).
    fn set_perst(&mut self, asserted: bool) {
        match self.soc {
            Soc::Bcm2711 => SOFTWARE_INIT.modify(&mut self.registers, |value| {
                value.with_perst(asserted)
            }),
            Soc::Bcm2712 => PCIE_CONTROL.modify(&mut self.registers, |value| {
                value.with_perst_released(!asserted)
            }),
        }
    }

    /// Write `value` to PHY register `register` over MDIO.
    fn mdio_write(&mut self, register: u16, value: u16, board: &mut impl Board) -> Result<(), Error> {
        MDIO_ADDRESS.write(
            &mut self.registers,
            MdioPacket::new_with_raw_value(0).with_register(register),
        );
        // The read orders the packet before the data.
        let _: MdioPacket = MDIO_ADDRESS.read(&mut self.registers);
        MDIO_WRITE_DATA.write(
            &mut self.registers,
            MdioData::new_with_raw_value(u32::from(value)).with_done(true),
        );
        for _ in 0..MDIO_POLLS {
            if !MDIO_WRITE_DATA.read(&mut self.registers).done() {
                return Ok(());
            }
            board.delay_us(10);
        }
        Err(Error::Mdio)
    }

    /// Read PHY register `register` over MDIO.
    fn mdio_read(&mut self, register: u16, board: &mut impl Board) -> Result<u32, Error> {
        MDIO_ADDRESS.write(
            &mut self.registers,
            MdioPacket::new_with_raw_value(0)
                .with_register(register)
                .with_read(true),
        );
        let _: MdioPacket = MDIO_ADDRESS.read(&mut self.registers);
        for _ in 0..MDIO_POLLS {
            let data = MDIO_READ_DATA.read(&mut self.registers);
            if data.done() {
                return Ok(data.data().value());
            }
            board.delay_us(10);
        }
        Err(Error::Mdio)
    }

    /// Turn spread spectrum clocking on (`brcm_pcie_set_ssc`); whether it
    /// runs with the PLL locked.
    fn spread_spectrum(&mut self, board: &mut impl Board) -> Result<bool, Error> {
        self.mdio_write(MDIO_BLOCK_SELECT, SSC_BLOCK, board)?;
        let control = self.mdio_read(SSC_CONTROL, board)?;
        let control = control | SSC_OVERRIDE_ENABLE | SSC_OVERRIDE_VALUE;
        self.mdio_write(SSC_CONTROL, u16::try_from(control & 0xffff).unwrap_or(0), board)?;
        board.delay_us(SSC_US);
        let status = self.mdio_read(SSC_STATUS, board)?;
        Ok(status & SSC_STATUS_ON != 0 && status & SSC_STATUS_PLL_LOCK != 0)
    }

    /// Open the outbound windows: CPU addresses in megabytes, PCI addresses
    /// whole (`brcm_pcie_set_outbound_win`).
    fn set_outbound(&mut self, windows: &[Window]) -> Result<(), Error> {
        if windows.len() > OUTBOUND_WINDOWS {
            return Err(Error::TooManyWindows);
        }
        for (index, window) in windows.iter().enumerate() {
            let (pci_low, pci_high) = halves(window.pci);
            outbound_pci_low(index).write(&mut self.registers, pci_low);
            outbound_pci_high(index).write(&mut self.registers, pci_high);
            let base = window.cpu >> 20;
            let limit = (window.cpu + window.size - 1) >> 20;
            let low = |megabytes: u64| u12::new(u16::try_from(megabytes & 0xfff).unwrap_or(0));
            let high = |megabytes: u64| u8::try_from((megabytes >> 12) & 0xff).unwrap_or(0);
            outbound_base_limit(index).modify(&mut self.registers, |value| {
                value.with_base(low(base)).with_limit(low(limit))
            });
            outbound_base_high(index).modify(&mut self.registers, |value| {
                value.with_megabytes(high(base))
            });
            outbound_limit_high(index).modify(&mut self.registers, |value| {
                value.with_megabytes(high(limit))
            });
        }
        Ok(())
    }

    /// Open the inbound windows; where MSIs to the bridge's own controller
    /// may go (`brcm_pcie_get_inbound_wins`, `set_inbound_win_registers`).
    ///
    /// The BCM2711 maps one window, window 2, from its lowest PCI address
    /// onto the system bus from zero, as large as the next power of two
    /// covering all of them, and closes windows 1 and 3. The BCM2712 opens
    /// window *n* for range *n*, each remapped onto its CPU address.
    fn set_inbound(&mut self, windows: &[Window]) -> Result<u64, Error> {
        match self.soc {
            Soc::Bcm2711 => {
                let total: u64 = windows.iter().map(|window| window.size).sum();
                let lowest = windows
                    .iter()
                    .map(|window| window.pci)
                    .min()
                    .ok_or(Error::BadInbound)?;
                let size = total.checked_next_power_of_two().ok_or(Error::BadInbound)?;
                if lowest & (size - 1) != 0 || (lowest > 1 << 31 && lowest < 1 << 32) {
                    return Err(Error::BadInbound);
                }
                for number in [1, 3] {
                    inbound_low(number).write(&mut self.registers, InboundLow::new_with_raw_value(0));
                    inbound_high(number).write(&mut self.registers, 0);
                }
                let (_, high) = halves(lowest);
                inbound_low(2).write(
                    &mut self.registers,
                    InboundLow::new_with_raw_value(0)
                        .with_base(page_bits(lowest))
                        .with_size(encode_inbound_size(size)),
                );
                inbound_high(2).write(&mut self.registers, high);
                // The window's memory controller span.
                let span = u5::new(u8::try_from(size.trailing_zeros() - 15).unwrap_or(0));
                MISC_CONTROL.modify(&mut self.registers, |value| value.with_scb0_size(span));
                Ok(if lowest >= 1 << 32 || lowest + size < 1 << 32 {
                    MSI_TARGET_BELOW_4G
                } else {
                    MSI_TARGET_ABOVE_4G
                })
            }
            Soc::Bcm2712 => {
                if windows.len() > BCM2712_INBOUND_WINDOWS {
                    return Err(Error::TooManyWindows);
                }
                for (index, window) in windows.iter().enumerate() {
                    let number = index + 1;
                    if !window.size.is_power_of_two() || window.pci & (window.size - 1) != 0 {
                        return Err(Error::BadInbound);
                    }
                    inbound_low(number).write(
                        &mut self.registers,
                        InboundLow::new_with_raw_value(0)
                            .with_base(page_bits(window.pci))
                            .with_size(encode_inbound_size(window.size)),
                    );
                    inbound_high(number).write(&mut self.registers, halves(window.pci).1);
                    inbound_remap_low(number).write(
                        &mut self.registers,
                        InboundRemap::new_with_raw_value(0)
                            .with_base(page_bits(window.cpu))
                            .with_access_enable(true),
                    );
                    inbound_remap_high(number).write(&mut self.registers, halves(window.cpu).1);
                }
                // Window 2 is the one the MSI target must stay clear of.
                let second = windows.get(1).copied().unwrap_or(Window {
                    pci: 0,
                    cpu: 0,
                    size: 0,
                });
                Ok(if second.pci >= 1 << 32 || second.pci + second.size < 1 << 32 {
                    MSI_TARGET_BELOW_4G
                } else {
                    MSI_TARGET_ABOVE_4G
                })
            }
        }
    }

    /// The BCM2712's extra set-up (`brcm_pcie_post_setup_bcm2712`): the PLL
    /// for the 54 MHz reference, the PM clock, error replies, timeouts and
    /// the QoS fixes. The QoS maps stay at their reset values.
    fn bcm2712_setup(&mut self, board: &mut impl Board) -> Result<(), Error> {
        self.mdio_write(MDIO_BLOCK_SELECT, PLL_54MHZ_BLOCK, board)?;
        for (register, value) in PLL_54MHZ {
            self.mdio_write(register, value, board)?;
        }
        board.delay_us(RESET_US);
        PHY_CONTROL_15.modify(&mut self.registers, |value| {
            value.with_pm_clock_period(PM_CLOCK_PERIOD_54MHZ)
        });
        UBUS_CONTROL.modify(&mut self.registers, |value| {
            value
                .with_reply_error_disable(true)
                .with_reply_decode_error_disable(true)
        });
        AXI_READ_ERROR_DATA.write(&mut self.registers, READ_ERROR_DATA);
        UBUS_TIMEOUT.write(&mut self.registers, UBUS_TIMEOUT_CLOCKS);
        CONFIG_RETRY_TIMEOUT.write(&mut self.registers, CONFIG_RETRY_TIMEOUT_CLOCKS);
        AXI_INTERFACE.modify(&mut self.registers, |value| {
            value
                .with_request_fifo_qos_propagation(false)
                .with_rclk_qos_array_fix(true)
                .with_qos_update_timing_fix(true)
                .with_disable_qos_gating(true)
        });
        // Without the timing fix (a BCM2712C1, or a one-lane bridge), limit
        // the requests in flight instead.
        if !AXI_INTERFACE.read(&mut self.registers).qos_update_timing_fix() {
            AXI_INTERFACE.modify(&mut self.registers, |value| {
                value.with_max_outstanding(THROTTLED_OUTSTANDING)
            });
        }
        MISC_CONTROL_1.modify(&mut self.registers, |value| value.with_vdm_qos_control(false));
        Ok(())
    }

    /// Point MSIs at `target` and catch all 32 vectors of the bridge's own
    /// controller (`brcm_msi_set_regs`).
    fn set_own_msi(&mut self, target: u64) {
        self.registers
            .write(MSI_INTERRUPTS + L2_MASK_CLEAR, u32::MAX);
        self.registers.write(MSI_INTERRUPTS + L2_CLEAR, u32::MAX);
        let (low, high) = halves(target);
        MSI_TARGET_LOW.write(
            &mut self.registers,
            MsiTargetLow::new_with_raw_value(low).with_enable(true),
        );
        MSI_TARGET_HIGH.write(&mut self.registers, high);
        MSI_DATA_MATCH.write(
            &mut self.registers,
            MsiDataMatch::new_with_raw_value(0)
                .with_pattern(MSI_DATA_PATTERN)
                .with_mask(MSI_DATA_MASK_32),
        );
    }

    /// Bring the bridge and the link up (`brcm_pcie_probe`,
    /// `brcm_pcie_setup`, `brcm_pcie_start_link`). The bridge's own
    /// interrupts stay masked.
    ///
    /// # Errors
    ///
    /// When the bridge is in endpoint mode, the windows do not fit it, the
    /// PHY does not answer or the link does not come up.
    pub fn bring_up(&mut self, setup: &Setup<'_>, board: &mut impl Board) -> Result<Link, Error> {
        let hard_debug = self.soc.hard_debug();
        if self.soc == Soc::Bcm2712 {
            self.reset_bridge(false, board);
            if !board.calibrate() {
                return Err(Error::Calibration);
            }
        }
        self.reset_bridge(true, board);
        // Some boot loaders leave the fundamental reset released.
        if self.soc == Soc::Bcm2711 {
            self.set_perst(true);
        }
        board.delay_us(RESET_US);
        self.reset_bridge(false, board);
        hard_debug.modify(&mut self.registers, |value| value.with_serdes_iddq(false));
        board.delay_us(SERDES_US);

        MISC_CONTROL.modify(&mut self.registers, |value| {
            value
                .with_scb_access(true)
                .with_config_read_ur_mode(true)
                .with_max_burst_size(match self.soc {
                    Soc::Bcm2711 => BURST_128,
                    Soc::Bcm2712 => BURST_512,
                })
                .with_rcb_mps_mode(true)
                .with_rcb_64b_mode(true)
        });
        let msi_target = self.set_inbound(setup.inbound)?;
        if !PCIE_STATUS.read(&mut self.registers).root_complex() {
            return Err(Error::NotRootComplex);
        }
        // Advertise L1, and L0s on the BCM2711 (the BCM2712's RP1 link is
        // marked `aspm-no-l0s`).
        let aspm = match self.soc {
            Soc::Bcm2711 => u2::new(ASPM_L0S.value() | ASPM_L1.value()),
            Soc::Bcm2712 => ASPM_L1,
        };
        LINK_CAPABILITY.modify(&mut self.registers, |value| value.with_aspm_support(aspm));
        ID_VALUE_3.modify(&mut self.registers, |value| value.with_class_code(CLASS_BRIDGE));
        self.set_outbound(setup.outbound)?;
        VENDOR_SPECIFIC_1.modify(&mut self.registers, |value| {
            value.with_endian_mode_bar2(u2::new(0))
        });
        if self.soc == Soc::Bcm2712 {
            self.bcm2712_setup(board)?;
        }
        let revision = REVISION.read(&mut self.registers);
        // The bridge's own interrupts stay masked.
        let cpu_interrupts = self.soc.cpu_interrupts();
        self.registers.write(cpu_interrupts + L2_MASK_SET, u32::MAX);
        self.registers.write(cpu_interrupts + L2_CLEAR, u32::MAX);
        if setup.own_msi {
            self.set_own_msi(msi_target);
        }

        // Start the link: CLKREQ# ignored until it is up.
        hard_debug.modify(&mut self.registers, |value| {
            value
                .with_clkreq_debug_enable(false)
                .with_refclk_override_enable(false)
                .with_refclk_override_out(false)
                .with_l1ss_enable(false)
        });
        self.set_perst(false);
        board.delay_us(PERST_US);
        let mut polls = 0;
        while !PCIE_STATUS.read(&mut self.registers).link_up() {
            if polls == LINK_POLLS {
                return Err(Error::LinkDown);
            }
            board.delay_us(LINK_POLL_US);
            polls += 1;
        }
        // The default CLKREQ# mode: L0s, L1 and L1 substates.
        hard_debug.modify(&mut self.registers, |value| value.with_l1ss_enable(true));
        if self.soc == Soc::Bcm2711 {
            RBUS_TIMEOUT.write(&mut self.registers, RBUS_TIMEOUT_CLOCKS);
        }
        // The BCM2711's device tree asks for spread spectrum clocking
        // (`brcm,enable-ssc`); the BCM2712 cannot do it.
        let spread_spectrum = self.soc == Soc::Bcm2711 && self.spread_spectrum(board)?;
        let link = LINK_CONTROL_STATUS.read(&mut self.registers);
        // The fundamental reset cleared the root control.
        if ROOT_CONTROL.read(&mut self.registers).crs_visibility() {
            ROOT_CONTROL.modify(&mut self.registers, |value| value.with_crs_visibility_enable(true));
        }
        Ok(Link {
            speed: link.current_speed().value(),
            width: link.width().value(),
            spread_spectrum,
            revision,
            msi_target: setup.own_msi.then_some(msi_target),
        })
    }

    /// Whether the link is up.
    pub fn link_up(&mut self) -> bool {
        PCIE_STATUS.read(&mut self.registers).link_up()
    }

    /// Raise vector `vector` of the bridge's own MSI controller from
    /// software, as a device's MSI would.
    pub fn raise_own_msi(&mut self, vector: u32) {
        self.registers
            .write(MSI_INTERRUPTS + L2_SET, 1 << (vector % OWN_MSI_VECTORS));
    }

    /// The pending vectors of the bridge's own MSI controller, cleared.
    pub fn take_own_msis(&mut self) -> u32 {
        let pending = self.registers.read(MSI_INTERRUPTS + L2_STATUS);
        self.registers.write(MSI_INTERRUPTS + L2_CLEAR, pending);
        pending
    }

    /// Configuration access to the functions below the bridge.
    pub const fn config(&mut self) -> Config<'_, Registers> {
        Config { bridge: self }
    }
}

/// The configuration spaces the bridge reaches: the root port's at its
/// registers, every other function's through the data window once the link
/// is up (an access without a link aborts).
pub struct Config<'bridge, Registers> {
    bridge: &'bridge mut Bridge<Registers>,
}

impl<Registers: Mmio> Config<'_, Registers> {
    /// Where `offset` of `function` is in the bridge's registers.
    fn locate(&mut self, function: Function, offset: u16) -> Option<usize> {
        if offset >= 0x1000 {
            return None;
        }
        if function.bus == 0 {
            return (function.device == 0 && function.function == 0).then_some(usize::from(offset));
        }
        if !self.bridge.link_up() {
            return None;
        }
        CONFIG_INDEX.write(
            &mut self.bridge.registers,
            ConfigIndex::new_with_raw_value(0)
                .with_bus(function.bus)
                .with_device(u5::new(function.device & 0x1f))
                .with_function(arbitrary_int::u3::new(function.function & 0x7)),
        );
        Some(CONFIG_DATA + usize::from(offset))
    }
}

impl<Registers: Mmio> ConfigSpace for Config<'_, Registers> {
    fn read(&mut self, function: Function, offset: u16) -> Option<u32> {
        let at = self.locate(function, offset)?;
        Some(self.bridge.registers.read(at))
    }

    fn write(&mut self, function: Function, offset: u16, value: u32) {
        if let Some(at) = self.locate(function, offset) {
            self.bridge.registers.write(at, value);
        }
    }
}

// ── The BCM2712's MSI controller ──────────────────────────────────────────

/// The MIP's registers: raise, clear, and per host (the ARM) and VPU
/// configuration and masks, in low and high halves of its 64 vectors.
const MIP_RAISE: usize = 0x00;
const MIP_CONFIG_LOW_HOST: usize = 0x20;
const MIP_CONFIG_HIGH_HOST: usize = 0x30;
const MIP_MASK_LOW_HOST: usize = 0x40;
const MIP_MASK_HIGH_HOST: usize = 0x50;
const MIP_MASK_LOW_VPU: usize = 0x60;
const MIP_MASK_HIGH_VPU: usize = 0x70;

/// Route every MIP vector to the ARM, edge triggered, none to the VPU
/// (`mip_init_domains`).
pub fn set_up_mip(mip: &mut impl Mmio) {
    mip.write(MIP_MASK_LOW_HOST, 0);
    mip.write(MIP_MASK_HIGH_HOST, 0);
    mip.write(MIP_MASK_LOW_VPU, u32::MAX);
    mip.write(MIP_MASK_HIGH_VPU, u32::MAX);
    mip.write(MIP_CONFIG_LOW_HOST, u32::MAX);
    mip.write(MIP_CONFIG_HIGH_HOST, u32::MAX);
}

/// Raise MIP vector `vector` from software: the word a device's MSI writes
/// to the doorbell, which is this register.
pub fn raise_mip(mip: &mut impl Mmio, vector: u32) {
    mip.write(MIP_RAISE, vector);
}

// ── The BCM2712's reset controllers ───────────────────────────────────────

/// `brcmstb-reset`: one bank of set, clear and status words per 32 resets.
const RESET_BANK_BYTES: usize = 0x18;
const RESET_SET: usize = 0x00;
const RESET_CLEAR: usize = 0x04;

/// Assert or release reset `id` of a `brcm,brcmstb-reset` controller; a
/// release takes up to 14 µs to reach its block.
pub fn set_reset(controller: &mut impl Mmio, id: u32, asserted: bool) {
    let bank = usize::try_from(id >> 5).unwrap_or(0) * RESET_BANK_BYTES;
    let register = if asserted { RESET_SET } else { RESET_CLEAR };
    controller.write(bank + register, 1 << (id & 0x1f));
}

/// The rescal block's start and status words.
const RESCAL_START: usize = 0x0;
const RESCAL_STATUS: usize = 0x8;
const RESCAL_BIT: u32 = 1;
/// Status polls before the calibration counts as failed, 100 µs apart.
pub const RESCAL_POLLS: u32 = 10;

/// Start the resistor calibration of a `brcm,bcm7216-pcie-sata-rescal`
/// block; whether it started.
pub fn start_calibration(rescal: &mut impl Mmio) -> bool {
    let start = rescal.read(RESCAL_START);
    rescal.write(RESCAL_START, start | RESCAL_BIT);
    rescal.read(RESCAL_START) & RESCAL_BIT != 0
}

/// Whether the calibration finished; once it has, stop it.
pub fn calibration_done(rescal: &mut impl Mmio) -> bool {
    if rescal.read(RESCAL_STATUS) & RESCAL_BIT == 0 {
        return false;
    }
    let start = rescal.read(RESCAL_START);
    rescal.write(RESCAL_START, start & !RESCAL_BIT);
    true
}
