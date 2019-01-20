// mod arch::aarch64

mod boot;
mod memory;
pub use self::memory::{PhysicalAddress, VirtualAddress};

use cortex_a::{asm, barrier, regs::*};

// Data memory barrier
#[inline]
pub fn dmb() {
    unsafe {
        barrier::dmb(barrier::SY);
    }
}

#[inline]
pub fn flushcache(address: usize) {
    unsafe {
        asm!("dc ivac, $0" :: "r"(address) :: "volatile");
    }
}

#[inline]
pub fn read_cpu_id() -> u64 {
    const CORE_MASK: u64 = 0x3;
    MPIDR_EL1.get() & CORE_MASK
}

#[inline]
pub fn current_el() -> u32 {
    CurrentEL.get()
}

#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}

#[inline]
pub fn loop_delay(rounds: u32) {
    for _ in 0..rounds {
        asm::nop();
    }
}

#[inline]
pub fn loop_until<F: Fn() -> bool>(f: F) {
    loop {
        if f() {
            break;
        }
        asm::nop();
    }
}

pub fn read_translation_table_base() -> PhysicalAddress {
    let mut base: PhysicalAddress = 0;
    unsafe {
        asm!("mrs $0, ttbr0_el1" : "=r"(base) ::: "volatile");
    }
    base
}

pub fn read_translation_control() -> u64 {
    let mut tcr: u64 = 0;
    unsafe {
        asm!("mrs $0, tcr_el1" : "=r"(tcr) ::: "volatile");
    }
    return tcr;
}

pub fn read_mair() -> u64 {
    let mut mair: u64 = 0;
    unsafe {
        asm!("mrs $0, mair_el1" : "=r"(mair) ::: "volatile");
    }
    return mair;
}

pub fn write_translation_table_base(base: PhysicalAddress) {
    unsafe {
        asm!("msr ttbr0_el1, $0" :: "r"(base) :: "volatile");
    }
}

// Helper function similar to u-boot
pub fn write_ttbr_tcr_mair(el: u8, base: PhysicalAddress, tcr: u64, attr: u64) {
    unsafe {
        asm!("dsb sy" :::: "volatile");
    }
    match (el) {
        1 => unsafe {
            asm!("msr ttbr0_el1, $0
                msr tcr_el1, $1
                msr mair_el1, $2" :: "r"(base), "r"(tcr), "r"(attr) : "memory" : "volatile");
        },
        2 => unsafe {
            asm!("msr ttbr0_el2, $0
                msr tcr_el2, $1
                msr mair_el2, $2" :: "r"(base), "r"(tcr), "r"(attr) : "memory" : "volatile");
        },
        3 => unsafe {
            asm!("msr ttbr0_el3, $0
                msr tcr_el3, $1
                msr mair_el3, $2" :: "r"(base), "r"(tcr), "r"(attr) : "memory" : "volatile");
        },
        _ => loop {},
    }
    unsafe {
        asm!("isb" :::: "volatile");
    }
}

// Identity-map things for now.
//
// > but more normal the simplest form is a table with 1024 32 bit entries starting at
// a 0x4000 aligned address, where each entry describes a 1 Mb memory part.
// On the rpi3 only the bottom 1024 entries are relevant as it has 1 Gb memory.

// aarch64 granules and page sizes howto:
// https://stackoverflow.com/questions/34269185/simultaneous-existence-of-different-sized-pages-on-aarch64

// Code from redox-os:

// pub static mut IDTR: DescriptorTablePointer = DescriptorTablePointer {
//     limit: 0,
//     base: 0
// };

// pub static mut IDT: [IdtEntry; 256] = [IdtEntry::new(); 256];

// /// A physical address.
// #[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
// pub struct PhysicalAddress(usize);

// impl PhysicalAddress {
//     pub fn new(address: usize) -> Self {
//         PhysicalAddress(address)
//     }

//     pub fn get(&self) -> usize {
//         self.0
//     }
// }

// /// A virtual address.
// #[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
// pub struct VirtualAddress(usize);

// impl VirtualAddress {
//     pub fn new(address: usize) -> Self {
//         VirtualAddress(address)
//     }

//     pub fn get(&self) -> usize {
//         self.0
//     }
// }

bitflags! {
    pub struct MemType: u64 {
        const DEVICE_NGNRNE = 0 << 2;
        const DEVICE_NGNRE  = 1 << 2;
        const DEVICE_GRE    = 2 << 2;
        const NORMAL_NC     = 3 << 2;
        const NORMAL        = 4 << 2;

        const NS            = 1 << 5;

        const NON_SHARE     = 0 << 8;
        const OUTER_SHARE   = 2 << 8;
        const INNER_SHARE   = 3 << 8;

        const AF            = 1 << 10;
        const NG            = 1 << 11;
        const PXN           = 1 << 53;
        const UXN           = 1 << 54;
    }
}

struct MemMapRegion {
    virt: VirtualAddress,
    phys: PhysicalAddress,
    size: usize,
    attr: MemType, // MAIR flags
}

impl MemMapRegion {}

fn setup_paging() {
    // test if paging is enabled
    // if so, loop here

    // @todo
    // Check mmu and dcache states, loop forever on some setting

    write_ttbr_tcr_mair(
        1, // EL1
        read_translation_table_base(),
        read_translation_control(),
        read_mair(),
    );

    let bcm2837_mem_map: [MemMapRegion; 2] = [
        MemMapRegion {
            virt: 0x0000_0000,
            phys: 0x0000_0000,
            size: 0x3f00_0000,
            attr: MemType::NORMAL | MemType::INNER_SHARE,
        },
        MemMapRegion {
            virt: 0x3f00_0000,
            phys: 0x3f00_0000,
            size: 0x0100_0000,
            attr: MemType::DEVICE_NGNRNE | MemType::NON_SHARE | MemType::PXN | MemType::UXN,
        },
    ];
}

pub struct BcmHost;

impl BcmHost {
    // As per https://www.raspberrypi.org/documentation/hardware/raspberrypi/peripheral_addresses.md
    /// This returns the ARM-side physical address where peripherals are mapped.
    pub fn get_peripheral_address() -> PhysicalAddress {
        0x3f00_0000
    }

    /// This returns the size of the peripheral's space.
    pub fn get_peripheral_size() -> usize {
        0x0100_0000
    }

    /// This returns the bus address of the SDRAM.
    pub fn get_sdram_address() -> PhysicalAddress {
        0xC000_0000 // uncached
    }
}
