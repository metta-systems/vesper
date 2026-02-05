use {aarch64_cpu::registers::ESR_EL1, core::fmt, tock_registers::LocalRegisterCopy};

// Exception syndrome register.
#[repr(transparent)]
pub struct EsrEL1(pub LocalRegisterCopy<u64, ESR_EL1::Register>);

impl EsrEL1 {
    #[inline(always)]
    pub fn exception_class(&self) -> Option<ESR_EL1::EC::Value> {
        self.0.read_as_enum(ESR_EL1::EC)
    }

    #[cfg(any(test, feature = "test_build"))]
    #[inline(always)]
    pub fn iss(&self) -> u64 {
        self.0.read(ESR_EL1::ISS)
    }
}

/// Human readable `ESR_EL1`.
#[rustfmt::skip]
impl fmt::Display for EsrEL1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Raw print of whole register.
        writeln!(f, "ESR_EL1: {:#010x}", self.0.get())?;

        // Raw print of exception class.
        write!(f, "      Exception Class         (EC) : {:#x}", self.0.read(ESR_EL1::EC))?;

        // Exception class.
        let ec_translation = match self.exception_class() {
            Some(ESR_EL1::EC::Value::DataAbortCurrentEL) => "Data Abort, current EL",
            _ => "N/A",
        };
        writeln!(f, " - {ec_translation}")?;

        // Raw print of instruction specific syndrome.
        write!(f, "      Instr Specific Syndrome (ISS): {:#x}", self.0.read(ESR_EL1::ISS))
    }
}
