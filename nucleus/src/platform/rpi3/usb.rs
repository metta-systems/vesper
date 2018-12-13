use arch::*;
use core::ops;
use platform::{gpio, rpi3::PERIPHERAL_BASE};
use register::mmio::*;

// USB registers base 0x7e980000
const USB_BASE: u32 = PERIPHERAL_BASE + 0x98_0000; // fixme?

// https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf
// with links to http://read.pudn.com/downloads197/doc/926344/otg_dbook.pdf
// and [rpiv102](RPi - USB Controller v1.02.pdf)

#[allow(non_snake_case)]
#[repr(C)]
pub struct HCChannel {
    HCCHAR: ReadWrite<u32>,
    HCSPLT: ReadWrite<u32>,
    HCINT: ReadWrite<u32>,
    HCINTMSK: ReadWrite<u32>,
    HCTSIZ: ReadWrite<u32>,
    HCDMA: ReadWrite<u32>,
    __reserved_1: [u32; 2],
}


#[allow(non_snake_case)]
#[repr(C)]
pub struct CoreRegisters {
    // Core Global CSR Map
    GOTGCTL: ReadWrite<u32>,         // 0x00 - OTG control
    GOTGINT: ReadWrite<u32>,         // 0x04 - OTG interrupt
    GAHBCFG: ReadWrite<u32>,         // 0x08 - AHB configuration
    GUSBCFG: ReadWrite<u32>,         // 0x0c - Core USB configuration
    GRSTCTL: ReadWrite<u32>,         // 0x10 - Core Reset
    GINTSTS: ReadWrite<u32>,         // 0x14 - Core Interrupt Register
    GINTMSK: ReadWrite<u32>,         // 0x18 - Core Interrupt Mask
    GRXSTSR: ReadOnly<u32>,          // 0x1c - Rx Status Debug / Status Read
    GRXSTSP: ReadOnly<u32>,          // 0x20 - Rx Status Pop
    GRXFSIZ: ReadWrite<u32>,         // 0x24 - Rx FIFO Size
    GNPTXFSIZ: ReadWrite<u32>,       // 0x28 - Non-periodic Tx FIFO Size
    GNPTXSTS: ReadOnly<u32>,         // 0x2c - Non-periodic Tx FIFO Queue Status
    GI2CCTL: ReadWrite<u32>,         // 0x30 - I²C Control
    GPVNDCTL: ReadWrite<u32>,        // 0x34 - PHY Vendor Control
    GGPIO: ReadWrite<u32>,           // 0x38 - GPIO
    GUID: ReadWrite<u32>,            // 0x3c - User ID
    GSNPSID: ReadOnly<u32>,          // 0x40 - Synopsis ID
    GHWCFG1: ReadOnly<u32>,          // 0x44 - Synopsis (Endpoint Direction)
    GHWCFG2: ReadOnly<u32>,          // 0x48 - Synopsis (User HW Config 2)
    GHWCFG3: ReadOnly<u32>,          // 0x4c - Synopsis (User HW Config 3)
    GHWCFG4: ReadOnly<u32>,          // 0x50 - Synopsis (User HW Config 4)
                                     // Official synopsis config ends here
    LPMCFG: u32,                     // 0x54 - Low Power Mode Configuration (rpiv102)
    __reserved_1: [u32; 10],         // 0x58-0x7f
    MDIOCTL: u32,                    // 0x80 - MDIO Interface Control (rpiv102)
    MDIODAT: u32,                    // 0x84 - MDIO Data Interface (rpiv102)
    VBUSCTL: u32,                    // 0x88 - VBUS and MISC Controls (rpiv102)
    __reserved_2: [u32; 29],         // 0x8c-0xff
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct HostRegisters {
    HCFG: ReadWrite<u32>,
    HFIR: ReadWrite<u32>,
    HFNUM: ReadWrite<u32>,
    HPTXSTS: ReadWrite<u32>,
    HAINT: ReadOnly<u32>,
    HAINTMSK: ReadWrite<u32>,
    HPRT: ReadWrite<u32>,
    HCChannels: [HCChannel; 16],
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct DeviceEndpointCtl {
    PCTL: ReadWrite<u32>, // DIEPCTLn/DOEPCTLn
    __reserved_1: u32,
    PINT: ReadWrite<u32>, // DIEPINTn/DOEPINTn
    __reserved_2: u32,
    PTSIZ: ReadWrite<u32>,
    PDMA: ReadWrite<u32>, // 0x914+EPn*0x20
    TXFSTS: ReadOnly<u32>,
    __reserved_3: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct DeviceRegisters {
    DCFG: ReadWrite<u32>,
    DCTL: ReadWrite<u32>,
    DSTS: ReadOnly<u32>,
    DIEPMSK: ReadWrite<u32>,
    DOEPMSK: ReadWrite<u32>,
    DAINT: ReadOnly<u32>,
    DAINTMSK: ReadWrite<u32>,
    DTKNQR1: ReadOnly<u32>,
    DTKNQR2: ReadOnly<u32>,
    DTKNQR3: ReadOnly<u32>,
    DTKNQR4: ReadOnly<u32>,
    DVBUSDIS: ReadWrite<u32>,
    DVBUSPULSE: ReadWrite<u32>,
    DTHRCTL: ReadWrite<u32>,
    DIEPEMPMSK: ReadWrite<u32>,
    // 0x900-...
    InEndpoints: [DeviceEndpointCtl; 16],
    // 0xb00-...
    OutEndpoints: [DeviceEndpointCtl; 16],

}

#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    core: CoreRegisters,
    HPTXFSIZ: ReadWrite<u32>,        // 0x100
    DPTXFSIZ: [ReadWrite<u32>; 15],  // 0x104-0x  same location DIEPTXF: [ReadWrite<u32>; 15],
    // Host Global Registers, 0x400-...
    host: HostRegisters,
    // Device Mode Registers, 0x800-...
    device: DeviceRegisters,
    // Power Control 0xe00-...
}

// in the spec:
// https://stackoverflow.com/questions/28660787/verilog-code-translation
// http://www.utdallas.edu/~akshay.sridharan/index_files/Page4933.htm
//
//  1'b0 - means that you want to have binary number of 1 bit and make it low
//  1'b1 - means that you want to have binary number of 1 bit and make it high
//  1'h0 - make a 1 digit hexadecimal number, iow 0000b
//
// e.g. 3'b101 makes three bits signal high-low-high
// 3'b1 makes three bits all high

//register_bitfields! {
//u32,
//
///// Auxiliary enables
//AUX_ENABLES [
///// If set the mini UART is enabled. The UART will immediately
///// start receiving data, especially if the UART1_RX line is
///// low.
///// If clear the mini UART is disabled. That also disables any
///// mini UART register access
//MINI_UART_ENABLE OFFSET(0) NUMBITS(1) []
//],
//}

//    AUX_MU_IO: ReadWrite<u32>,                          // 0x40 - Mini Uart I/O Data
//    AUX_MU_IER: WriteOnly<u32>,                         // 0x44 - Mini Uart Interrupt Enable
//    AUX_MU_IIR: WriteOnly<u32, AUX_MU_IIR::Register>,   // 0x48
//    AUX_MU_LCR: WriteOnly<u32, AUX_MU_LCR::Register>,   // 0x4C
//    AUX_MU_MCR: WriteOnly<u32>,                         // 0x50
//    AUX_MU_LSR: ReadOnly<u32, AUX_MU_LSR::Register>,    // 0x54
//    __reserved_2: [u32; 2],                             // 0x58 - AUX_MU_MSR, AUX_MU_SCRATCH
//    AUX_MU_CNTL: WriteOnly<u32, AUX_MU_CNTL::Register>, // 0x60
//    __reserved_3: u32,                                  // 0x64 - AUX_MU_STAT
//    AUX_MU_BAUD: WriteOnly<u32, AUX_MU_BAUD::Register>, // 0x68
