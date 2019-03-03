// Interrupt handling

// The base address is given by VBAR_ELn and each entry has a defined offset from this
// base address. Each table has 16 entries, with each entry being 128 bytes (32 instructions)
// in size. The table effectively consists of 4 sets of 4 entries.

// Minimal implementation to help catch MMU traps
// Reads ESR_ELx to understand why trap was taken.

// VBAR_EL1, VBAR_EL2, VBAR_EL3

// CurrentEL with SP0: +0x0

//     Synchronous
//     IRQ/vIRQ
//     FIQ
//     SError/vSError

// CurrentEL with SPx: +0x200

//     Synchronous
//     IRQ/vIRQ
//     FIQ
//     SError/vSError

// Lower EL using AArch64: +0x400

//     Synchronous
//     IRQ/vIRQ
//     FIQ
//     SError/vSError

// Lower EL using AArch32: +0x600

//     Synchronous
//     IRQ/vIRQ
//     FIQ
//     SError/vSError

// When the processor takes an exception to AArch64 execution state,
// all of the PSTATE interrupt masks is set automatically. This means
// that further exceptions are disabled. If software is to support
// nested exceptions, for example, to allow a higher priority interrupt
// to interrupt the handling of a lower priority source, then software needs
// to explicitly re-enable interrupts
use crate::{arch::endless_sleep, println};
use cortex_a::{barrier, regs};
use register::cpu::RegisterReadWrite;

global_asm!(include_str!("vectors.S"));

pub unsafe fn set_vbar_el1_checked(vec_base_addr: u64) -> bool {
    if vec_base_addr.trailing_zeros() < 11 {
        false
    } else {
        regs::VBAR_EL1.set(vec_base_addr);

        // Force VBAR update to complete before next instruction.
        barrier::isb(barrier::SY);

        true
    }
}

#[repr(C)]
pub struct GPR {
    x: [u64; 31],
}

#[repr(C)]
pub struct ExceptionContext {
    // General Purpose Registers
    gpr: GPR,
    spsr_el1: u64,
    elr_el1: u64,
}

/// The default exception, invoked for every exception type unless the handler
/// is overwritten.
#[no_mangle]
unsafe extern "C" fn default_exception_handler() -> ! {
    println!("Unexpected exception. Halting CPU.");

    endless_sleep()
}

mod esr_el1 {
    // use cortex_a::{sys_coproc_read_raw, sys_coproc_write_raw};
    use register::{cpu::RegisterReadWrite, register_bitfields};

    pub struct Reg;

    register_bitfields! {
        u64,

        ESR_EL1 [
            ISS OFFSET(0) NUMBITS(25) [], // @todo Additional ISS encodings
            IL OFFSET(25) NUMBITS(1) [], // Instruction Length
            EC OFFSET(26) NUMBITS(6) [
                Unknown = 0b000_000,
                TrappedWfiOrWfe = 0b000_001,
                TrappedMcrOrMrc = 0b000_011,
                TrappedMcrrOrMrrc = 0b000_100,
                TrappedMcrOrMrc2 = 0b000_101,
                TrappedLdcOrStc = 0b000_110,
                TrappedAdvSIMD = 0b000_111,
                TrappedMrrc = 0b001_100,
                IllegalExecState = 0b001_110,
                SvcInAArch32 = 0b010_001,
                SvcInAArch64 = 0b010_101,
                TrappedMrsOrMsr = 0b011_000,
                TrappedSve = 0b011_001,
                InsnAbortFromLowerEL = 0b100_000,
                InsnAbortFromSameEL = 0b100_001,
                PcAlignmentFault = 0b100_010,
                DataAbortFromLowerEL = 0b100_100,
                DataAbortFromSameEL = 0b100_101,
                SpAlignmentFault = 0b100_110,
                TrappedFpuFromAArch32 = 0b101_000,
                TrappedFpuFromAArch64 = 0b101_100,
                SError = 0b101_111,
                BreakpointFromLowerEL = 0b110_000,
                BreakpointFromSameEL = 0b110_001,
                SoftwareStepFromLowerEL = 0b110_010,
                SoftwareStepFromSameEL = 0b110_011,
                WatchpointFromLowerEL = 0b110_100,
                WatchpointFromSameEL = 0b110_101,
                BrkptInAArch32 = 0b111_000,
                BrkInAArch64 = 0b111_100
            ]
        ]
    }

    impl RegisterReadWrite<u64, ESR_EL1::Register> for Reg {
        // sys_coproc_read_raw!(u64, "ESR_EL1");
        // sys_coproc_write_raw!(u64, "ESR_EL1");

        // Manually unmacroed
        /// Reads the raw bits of the CPU register.
        #[inline]
        fn get(&self) -> u64 {
            match () {
                #[cfg(target_arch = "aarch64")]
                () => {
                    let reg;
                    unsafe {
                        asm!(concat!("mrs", " $0, ", "ESR_EL1") : "=r"(reg) ::: "volatile");
                    }
                    reg
                }

                #[cfg(not(target_arch = "aarch64"))]
                () => unimplemented!(),
            }
        }

        /// Writes raw bits to the CPU register.
        #[cfg_attr(not(target_arch = "aarch64"), allow(unused_variables))]
        #[inline]
        fn set(&self, value: u64) {
            match () {
                #[cfg(target_arch = "aarch64")]
                () => unsafe {
                    asm!(concat!("msr", " ", "ESR_EL1", ", $0") :: "r"(value) :: "volatile")
                },

                #[cfg(not(target_arch = "aarch64"))]
                () => unimplemented!(),
            }
        }
    }

    pub static ESR_EL1: Reg = Reg {};
}

mod far_el1 {
    use register::cpu::RegisterReadWrite;

    pub struct Reg;

    impl RegisterReadWrite<u64, ()> for Reg {
        // sys_coproc_read_raw!(u64, "FAR_EL1");
        // sys_coproc_write_raw!(u64, "FAR_EL1");

        // Manually unmacroed
        /// Reads the raw bits of the CPU register.
        #[inline]
        fn get(&self) -> u64 {
            match () {
                #[cfg(target_arch = "aarch64")]
                () => {
                    let reg;
                    unsafe {
                        asm!(concat!("mrs", " $0, ", "FAR_EL1") : "=r"(reg) ::: "volatile");
                    }
                    reg
                }

                #[cfg(not(target_arch = "aarch64"))]
                () => unimplemented!(),
            }
        }

        /// Writes raw bits to the CPU register.
        #[cfg_attr(not(target_arch = "aarch64"), allow(unused_variables))]
        #[inline]
        fn set(&self, value: u64) {
            match () {
                #[cfg(target_arch = "aarch64")]
                () => unsafe {
                    asm!(concat!("msr", " ", "FAR_EL1", ", $0") :: "r"(value) :: "volatile")
                },

                #[cfg(not(target_arch = "aarch64"))]
                () => unimplemented!(),
            }
        }
    }

    pub static FAR_EL1: Reg = Reg {};
}

use esr_el1::ESR_EL1;
use far_el1::FAR_EL1;

// To implement an exception handler, overwrite it by defining the respective
// function below.
// Don't forget the #[no_mangle] attribute.
//
// unsafe extern "C" fn current_el0_synchronous(e: &mut ExceptionContext);
// unsafe extern "C" fn current_el0_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn current_el0_serror(e: &mut ExceptionContext);

// unsafe extern "C" fn current_elx_synchronous(e: &mut ExceptionContext);
// unsafe extern "C" fn current_elx_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn current_elx_serror(e: &mut ExceptionContext);

// unsafe extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext);

// unsafe extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext);

#[no_mangle]
unsafe extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    println!("[!] A synchronous exception happened.");
    println!("      ESR_EL1: {:#010x} (syndrome)", ESR_EL1.get());
    println!("           EC: {:#06b} (cause)", ESR_EL1.read(ESR_EL1::EC));
    println!("      FAR_EL1: {:#016x} (location)", FAR_EL1.get());
    println!("      ELR_EL1: {:#010x}", e.elr_el1);

    println!(
        "      Incrementing ELR_EL1 by 4 now to continue with the first \
         instruction after the exception!"
    );

    e.elr_el1 += 4;

    println!("      ELR_EL1 modified: {:#010x}", e.elr_el1);
    println!("      Returning from exception...\n");
}
