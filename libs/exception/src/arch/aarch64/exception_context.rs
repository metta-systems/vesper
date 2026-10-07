use {
    aarch64_cpu::registers::{ESR_EL1, FAR_EL1, Readable},
    core::{
        fmt,
        mem::{align_of, offset_of, size_of},
    },
    tock_registers::LocalRegisterCopy,
};

/// The vector group from which an exception entered, or to which a saved context returns.
#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionOrigin {
    /// Current EL using `SP_EL0` (`EL1t`).
    CurrentSp0 = 0,
    /// Current EL using `SP_EL1` (`EL1h`).
    CurrentSpx = 1,
    /// Lower EL executing `AArch64`.
    LowerAarch64 = 2,
    /// Lower EL executing `AArch32`.
    LowerAarch32 = 3,
}

impl fmt::Display for ExceptionOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CurrentSp0 => "current EL using SP_EL0",
            Self::CurrentSpx => "current EL using SP_EL1",
            Self::LowerAarch64 => "lower EL, AArch64",
            Self::LowerAarch32 => "lower EL, AArch32",
        })
    }
}

/// Kernel-private execution state copied out of a transient exception frame.
///
/// This value owns no stack storage and retains no exception-frame address.
/// Its origin and raw SPSR must describe a compatible return mode; the caller
/// establishes valid execution addresses and stacks before restoring it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedContext {
    /// General Purpose Registers, x0-x29.
    pub gpr: [u64; 30],
    /// The link register, aka x30.
    pub lr: u64,
    /// Raw saved program status, including all flags and interrupt masks.
    pub spsr_el1: u64,
    /// Saved execution program counter.
    pub elr_el1: u64,
    /// Execution stack pointer, not the transient exception-frame address.
    pub sp: u64,
    /// Vector group / return stack selection.
    pub origin: ExceptionOrigin,
    /// EL0 read/write thread-local storage register: per-Thread state.
    pub tpidr_el0: u64,
}

impl SavedContext {
    /// Initial context for trusted `EL1t` bootstrap/fixture execution on `SP_EL0`.
    ///
    /// All GPRs, LR and TLS are zero; D, A, I and F are masked. This constructs
    /// internal execution state, not a public privileged-Thread creation ABI.
    /// The caller supplies a valid PC and a mapped, 16-byte-aligned stack SP.
    pub const fn el1t(pc: u64, sp: u64) -> Self {
        Self {
            gpr: [0; 30],
            lr: 0,
            spsr_el1: 0x3c4,
            elr_el1: pc,
            sp,
            origin: ExceptionOrigin::CurrentSp0,
            tpidr_el0: 0,
        }
    }

    /// Initial context for unprivileged `EL0t` execution on `SP_EL0`.
    ///
    /// All GPRs, LR and TLS are zero except `x0 = argument`; D, A, I and F are
    /// masked (EL0 cannot unmask them: `SCTLR_EL1.UMA` traps DAIF access).
    /// The caller supplies a PC and a 16-byte-aligned SP that are mapped
    /// EL0-accessible in the Thread's `AddressSpace`.
    pub const fn el0(pc: u64, sp: u64, argument: u64) -> Self {
        let mut gpr = [0; 30];
        gpr[0] = argument;
        Self {
            gpr,
            lr: 0,
            spsr_el1: 0x3c0,
            elr_el1: pc,
            sp,
            origin: ExceptionOrigin::LowerAarch64,
            tpidr_el0: 0,
        }
    }
}

/// The exception context as it is stored on the stack on exception entry.
/// Keep in sync with exception setup code in vectors.S!
///
/// Implementation status: traps use `SP_EL1`; persistent continuations are copied
/// with `save`, never retained as addresses of this transient frame.
#[repr(C, align(16))]
pub struct ExceptionContext {
    /// General Purpose Registers, x0-x29
    pub gpr: [u64; 30],
    /// The link register, aka x30.
    pub lr: u64,
    /// Saved program status.
    pub spsr_el1: super::spsr_el1::SpsrEL1,
    /// Exception link register. The program counter at the time the exception happened.
    pub elr_el1: u64,
    /// Execution SP: `SP_EL0` for SP0/lower entries, pre-frame `SP_EL1` for `SPx`.
    pub sp: u64,
    /// Vector group / return stack selection.
    pub origin: ExceptionOrigin,
    /// EL0 read/write thread-local storage register, saved on entry and
    /// restored on `eret` (it also keeps the frame a multiple of 16 bytes).
    pub tpidr_el0: u64,
}

const _: () = {
    assert!(size_of::<ExceptionOrigin>() == 8);
    assert!(align_of::<ExceptionOrigin>() == 8);
    assert!(size_of::<super::spsr_el1::SpsrEL1>() == 8);
    assert!(align_of::<super::spsr_el1::SpsrEL1>() == 8);
    assert!(offset_of!(ExceptionContext, gpr) == 0);
    assert!(offset_of!(ExceptionContext, lr) == 240);
    assert!(offset_of!(ExceptionContext, spsr_el1) == 248);
    assert!(offset_of!(ExceptionContext, elr_el1) == 256);
    assert!(offset_of!(ExceptionContext, sp) == 264);
    assert!(offset_of!(ExceptionContext, origin) == 272);
    assert!(offset_of!(ExceptionContext, tpidr_el0) == 280);
    assert!(size_of::<ExceptionContext>() == 288);
    assert!(align_of::<ExceptionContext>() == 16);
    assert!(offset_of!(SavedContext, gpr) == 0);
    assert!(offset_of!(SavedContext, lr) == 240);
    assert!(offset_of!(SavedContext, spsr_el1) == 248);
    assert!(offset_of!(SavedContext, elr_el1) == 256);
    assert!(offset_of!(SavedContext, sp) == 264);
    assert!(offset_of!(SavedContext, origin) == 272);
    assert!(offset_of!(SavedContext, tpidr_el0) == 280);
    assert!(size_of::<SavedContext>() == 288);
    assert!(align_of::<SavedContext>() == 8);
};

impl From<SavedContext> for ExceptionContext {
    fn from(saved: SavedContext) -> Self {
        Self {
            gpr: saved.gpr,
            lr: saved.lr,
            spsr_el1: super::spsr_el1::SpsrEL1::from_raw(saved.spsr_el1),
            elr_el1: saved.elr_el1,
            sp: saved.sp,
            origin: saved.origin,
            tpidr_el0: saved.tpidr_el0,
        }
    }
}

impl ExceptionContext {
    /// Copy the execution state into storage independent of the trap stack.
    pub fn save(&self) -> SavedContext {
        SavedContext {
            gpr: self.gpr,
            lr: self.lr,
            spsr_el1: self.spsr_el1.raw(),
            elr_el1: self.elr_el1,
            sp: self.sp,
            origin: self.origin,
            tpidr_el0: self.tpidr_el0,
        }
    }

    /// Rewrite this transient frame for normal handler return and `eret`.
    ///
    /// The caller establishes valid addresses and a compatible origin/SPSR;
    /// this copies raw state without checking execution authority or mappings.
    pub fn restore(&mut self, saved: SavedContext) {
        *self = Self::from(saved);
    }
    // #[inline(always)]
    // fn exception_class(&self) -> Option<ESR_EL1::EC::Value> {
    //     self.esr_el1.exception_class()
    // }

    #[inline(always)]
    fn fault_address_valid() -> bool {
        use ESR_EL1::EC::Value::{
            DataAbortCurrentEL, DataAbortLowerEL, InstrAbortCurrentEL, InstrAbortLowerEL,
            PCAlignmentFault, WatchpointCurrentEL, WatchpointLowerEL,
        };

        let esr_el1 = super::esr_el1::EsrEL1(LocalRegisterCopy::new(ESR_EL1.get()));

        match esr_el1.exception_class() {
            None => false,
            Some(ec) => matches!(
                ec,
                InstrAbortLowerEL
                    | InstrAbortCurrentEL
                    | PCAlignmentFault
                    | DataAbortLowerEL
                    | DataAbortCurrentEL
                    | WatchpointLowerEL
                    | WatchpointCurrentEL
            ),
        }
    }

    pub fn write_gprs(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "General purpose registers:")?;

        let alternating = |x| -> _ { if x % 2 == 0 { "   " } else { "\n" } };

        // Print two registers per line.
        for (i, reg) in self.gpr.iter().enumerate() {
            write!(f, "      x{: <2}: {: >#018x}{}", i, reg, alternating(i))?;
        }
        Ok(())
    }
}

/// Human readable print of the exception context.
impl fmt::Display for ExceptionContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // writeln!(f, "{}", self.esr_el1)?;

        if Self::fault_address_valid() {
            writeln!(
                f,
                "FAR_EL1: {:#018x}",
                usize::try_from(FAR_EL1.get()).unwrap_or(0)
            )?;
        }

        writeln!(f, "{}", self.spsr_el1)?;
        writeln!(f, "ELR_EL1: {:#018x} (return to)", self.elr_el1)?;
        writeln!(f, "Execution SP: {:#018x}", self.sp)?;
        writeln!(f, "Exception origin: {}", self.origin)?;
        writeln!(f)?;
        self.write_gprs(f)?;
        write!(f, "      lr : {:#018x}", self.lr)
    }
}
