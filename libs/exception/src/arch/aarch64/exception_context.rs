use {
    aarch64_cpu::registers::{ESR_EL1, FAR_EL1, Readable},
    core::fmt,
    tock_registers::LocalRegisterCopy,
};

/// The exception context as it is stored on the stack on exception entry.
/// Keep in sync with exception setup code in vectors.S!
#[repr(C)]
pub struct ExceptionContext {
    /// General Purpose Registers, x0-x29
    pub gpr: [u64; 30],
    /// The link register, aka x30.
    pub lr: u64,
    /// Saved program status.
    pub spsr_el1: super::spsr_el1::SpsrEL1,
    /// Exception link register. The program counter at the time the exception happened.
    pub elr_el1: u64,
}

impl ExceptionContext {
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
        writeln!(f)?;
        self.write_gprs(f)?;
        write!(f, "      lr : {:#018x}", self.lr)
    }
}
