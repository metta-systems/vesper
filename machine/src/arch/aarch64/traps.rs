// @todo this file must be moved to exception/mod.rs
// @todo finish porting the exception printing part...

fn cause_to_string(cause: u64) -> &'static str {
    if cause == ESR_EL1::EC::DataAbortCurrentEL.read(ESR_EL1::EC) {
        "Data Alignment Check"
    } else {
        "Unknown"
    }
}

register_bitfields! {
    u64,
    /// ISS structure for Data Abort exceptions
    ISS_DA [
        /// Instruction Syndrome Valid. Indicates whether the syndrome information in ISS[23:14] is valid.
        /// (This includes SAS, SSE, SRT, SF, and AR)
        ISV   OFFSET(24) NUMBITS(1) [],
        SAS   OFFSET(22) NUMBITS(2) [
            Byte = 0b00,
            Halfword = 0b01,
            Word = 0b10,
            DoubleWord = 0b11
        ],
        SSE   OFFSET(21) NUMBITS(1) [],
        SRT   OFFSET(16) NUMBITS(5) [],
        SF    OFFSET(15) NUMBITS(1) [],
        AR    OFFSET(14) NUMBITS(1) [],
        VNCR  OFFSET(13) NUMBITS(1) [],
        SET   OFFSET(11) NUMBITS(2) [
            UER = 0b00, // Recoverable state
            UC = 0b10, // Uncontainable
            UEO = 0b11 // Restartable state
        ],
        FNV   OFFSET(10) NUMBITS(1) [],
        EA    OFFSET(9)  NUMBITS(1) [],
        CM    OFFSET(8)  NUMBITS(1) [],
        S1PTW OFFSET(7)  NUMBITS(1) [],
        WNR   OFFSET(6)  NUMBITS(1) [],
        DFSC  OFFSET(0)  NUMBITS(6) [
            /// Address size fault, level 0 of translation or translation table base register.
            AddressSizeTL0 = 0b000000,
            /// Address size fault, level 1.
            AddressSizeTL1 = 0b000001,
            ///Address size fault, level 2.
            AddressSizeTL2 = 0b000010,
            /// Address size fault, level 3.
            AddressSizeTL3 = 0b000011,
            /// Translation fault, level 0.
            TranslationFaultTL0 = 0b000100,
            /// Translation fault, level 1.
            TranslationFaultTL1 = 0b000101,
            /// Translation fault, level 2.
            TranslationFaultTL2 = 0b000110,
            /// Translation fault, level 3.
            TranslationFaultTL3 = 0b000111,
            /// Access flag fault, level 1.
            AccessFaultTL1 = 0b001001,
            /// Access flag fault, level 2.
            AccessFaultTL2 = 0b001010,
            /// Access flag fault, level 3.
            AccessFaultTL3 = 0b001011,
            /// Permission fault, level 1.
            PermissionFaultTL1 = 0b001101,
            /// Permission fault, level 2.
            PermissionFaultTL2 = 0b001110,
            /// Permission fault, level 3.
            PermissionFaultTL3 = 0b001111,
            /// Synchronous External abort, not on translation table walk or hardware update of translation table.
            SyncExternalAbort = 0b010000,
            /// Synchronous Tag Check Fault.
            /// (When FEAT_MTE is implemented)
            SyncTagCheckFault = 0b010001,
            /// Synchronous External abort on translation table walk or hardware update of translation table, level 0.
            SyncAbortOnTranslationTL0 = 0b010100,
            /// Synchronous External abort on translation table walk or hardware update of translation table, level 1.
            SyncAbortOnTranslationTL1 = 0b010101,
            /// Synchronous External abort on translation table walk or hardware update of translation table, level 2.
            SyncAbortOnTranslationTL2 = 0b010110,
            /// Synchronous External abort on translation table walk or hardware update of translation table, level 3.
            SyncAbortOnTranslationTL3 = 0b010111,
            /// Synchronous parity or ECC error on memory access, not on translation table walk.
            /// (When FEAT_RAS is not implemented)
            SyncParityError = 0b011000,
            /// Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 0.
            /// (When FEAT_RAS is not implemented)
            SyncParityErrorOnTranslationTL0 = 0b011100,
            /// Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 1.
            /// (When FEAT_RAS is not implemented)
            SyncParityErrorOnTranslationTL1 = 0b011101,
            /// Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 2.
            /// (When FEAT_RAS is not implemented)
            SyncParityErrorOnTranslationTL2 = 0b011110,
            /// Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 3.
            /// (When FEAT_RAS is not implemented)
            SyncParityErrorOnTranslationTL3 = 0b011111,
            /// Alignment fault.
            AlignmentFault = 0b100001,
            /// TLB conflict abort.
            TlbConflictAbort = 0b110000,
            /// Unsupported atomic hardware update fault.
            /// (When FEAT_HAFDBS is implemented)
            UnsupportedAtomicUpdate = 0b110001,
            /// IMPLEMENTATION DEFINED fault (Lockdown).
            Lockdown = 0b110100,
            /// IMPLEMENTATION DEFINED fault (Unsupported Exclusive or Atomic access).
            UnsupportedAccess = 0b110101
        ]
    ]
}

type IssForDataAbort = LocalRegisterCopy<u64, ISS_DA::Register>;

fn iss_dfsc_to_string(iss: IssForDataAbort) -> &'static str {
    match iss.read_as_enum(ISS_DA::DFSC) {
        Some(ISS_DA::DFSC::Value::AddressSizeTL0) => "Address size fault, level 0 of translation or translation table base register",
        Some(ISS_DA::DFSC::Value::AddressSizeTL1) => "Address size fault, level 1",
        Some(ISS_DA::DFSC::Value::AddressSizeTL2) => "Address size fault, level 2",
        Some(ISS_DA::DFSC::Value::AddressSizeTL3) => "Address size fault, level 3",
        Some(ISS_DA::DFSC::Value::TranslationFaultTL0) => "Translation fault, level 0",
        Some(ISS_DA::DFSC::Value::TranslationFaultTL1) => "Translation fault, level 1",
        Some(ISS_DA::DFSC::Value::TranslationFaultTL2) => "Translation fault, level 2",
        Some(ISS_DA::DFSC::Value::TranslationFaultTL3) => "Translation fault, level 3",
        Some(ISS_DA::DFSC::Value::AccessFaultTL1) => "Access flag fault, level 1",
        Some(ISS_DA::DFSC::Value::AccessFaultTL2) => "Access flag fault, level 2",
        Some(ISS_DA::DFSC::Value::AccessFaultTL3) => "Access flag fault, level 3",
        Some(ISS_DA::DFSC::Value::PermissionFaultTL1) => "Permission fault, level 1",
        Some(ISS_DA::DFSC::Value::PermissionFaultTL2) => "Permission fault, level 2",
        Some(ISS_DA::DFSC::Value::PermissionFaultTL3) => "Permission fault, level 3",
        Some(ISS_DA::DFSC::Value::SyncExternalAbort) => "Synchronous External abort, not on translation table walk or hardware update of translation table",
        Some(ISS_DA::DFSC::Value::SyncTagCheckFault) => "Synchronous Tag Check Fault",
        Some(ISS_DA::DFSC::Value::SyncAbortOnTranslationTL0) => "Synchronous External abort on translation table walk or hardware update of translation table, level 0",
        Some(ISS_DA::DFSC::Value::SyncAbortOnTranslationTL1) => "Synchronous External abort on translation table walk or hardware update of translation table, level 1",
        Some(ISS_DA::DFSC::Value::SyncAbortOnTranslationTL2) => "Synchronous External abort on translation table walk or hardware update of translation table, level 2",
        Some(ISS_DA::DFSC::Value::SyncAbortOnTranslationTL3) => "Synchronous External abort on translation table walk or hardware update of translation table, level 3",
        Some(ISS_DA::DFSC::Value::SyncParityError) => "Synchronous parity or ECC error on memory access, not on translation table walk",
        Some(ISS_DA::DFSC::Value::SyncParityErrorOnTranslationTL0) => "Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 0",
        Some(ISS_DA::DFSC::Value::SyncParityErrorOnTranslationTL1) => "Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 1",
        Some(ISS_DA::DFSC::Value::SyncParityErrorOnTranslationTL2) => "Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 2",
        Some(ISS_DA::DFSC::Value::SyncParityErrorOnTranslationTL3) => "Synchronous parity or ECC error on memory access on translation table walk or hardware update of translation table, level 3",
        Some(ISS_DA::DFSC::Value::AlignmentFault) => "Alignment fault",
        Some(ISS_DA::DFSC::Value::TlbConflictAbort) => "TLB conflict abort",
        Some(ISS_DA::DFSC::Value::UnsupportedAtomicUpdate) => "Unsupported atomic hardware update fault",
        Some(ISS_DA::DFSC::Value::Lockdown) => "Lockdown (IMPLEMENTATION DEFINED fault)",
        Some(ISS_DA::DFSC::Value::UnsupportedAccess) => "Unsupported Exclusive or Atomic access (IMPLEMENTATION DEFINED fault)",
        _ => "Unknown",
    }
}

type SpsrCopy = LocalRegisterCopy<u64, SPSR_EL1::Register>;

/// Helper function to 1) display current exception, 2) skip the offending asm instruction.
/// Not for production use!
fn synchronous_common(e: &mut ExceptionContext) {
    println!("      ESR_EL1: {:#010x} (syndrome)", ESR_EL1.get());
    let cause = ESR_EL1.read(ESR_EL1::EC);
    println!(
        "           EC: {:#08b} (cause) -- {}",
        cause,
        cause_to_string(cause)
    );

    // Print more details about Data Alignment Check
    if cause == ESR_EL1::EC::DataAbortCurrentEL.read(ESR_EL1::EC) {
        let iss = ESR_EL1.read(ESR_EL1::ISS);
        let iss = IssForDataAbort::new(iss);
        if iss.is_set(ISS_DA::ISV) {
            println!(
                "               Access size: {} bytes ({}signed) to {}{}",
                2u64.pow(iss.read(ISS_DA::SAS) as u32),
                if iss.is_set(ISS_DA::SSE) { "" } else { "un" },
                if iss.is_set(ISS_DA::SF) { "x" } else { "r" },
                iss.read(ISS_DA::SRT)
            );
            println!(
                "               Acq/Rel semantics: {}present",
                if iss.is_set(ISS_DA::AR) { "" } else { "not " }
            );
        }
        // data abort specific encoding
        println!(
            "               {} address {:#016x} ({}valid)",
            if iss.is_set(ISS_DA::WNR) {
                "Writing to"
            } else {
                "Reading from"
            },
            FAR_EL1.get(),
            if iss.is_set(ISS_DA::FNV) { "not " } else { "" }
        );
        println!("               Specific fault: {}", iss_dfsc_to_string(iss));
    } else {
        #[rustfmt::skip]
        {
            println!("      FAR_EL1: {:#016x} (location)", FAR_EL1.get());
            println!("     SPSR_EL1: {:#016x} (state)", e.spsr_el1);
            let spsr = SpsrCopy::new(e.spsr_el1);
            println!("               N: {} (negative condition)", spsr.read(SPSR_EL1::N));
            println!("               Z: {} (zero condition)", spsr.read(SPSR_EL1::Z));
            println!("               C: {} (carry condition)", spsr.read(SPSR_EL1::C));
            println!("               V: {} (overflow condition)", spsr.read(SPSR_EL1::V));
            println!("               SS: {} (software step)", spsr.read(SPSR_EL1::SS));
            println!("               IL: {} (illegal execution state)", spsr.read(SPSR_EL1::IL));
            println!("               D: {} (debug masked)", spsr.read(SPSR_EL1::D));
            println!("               A: {} (serror masked)", spsr.read(SPSR_EL1::A));
            println!("               I: {} (irq masked)", spsr.read(SPSR_EL1::I));
            println!("               F: {} (fiq masked)", spsr.read(SPSR_EL1::F));
            println!("               M: {:#06b} (machine state)", spsr.read(SPSR_EL1::M));
        }
    }
    println!("      ELR_EL1: {:#010x} (return to)", e.elr_el1);

    // GPRs

    println!(
        "      Incrementing ELR_EL1 by 4 to continue with the first \
         instruction after the exception!"
    );

    e.elr_el1 += 4;

    println!("      ELR_EL1 modified: {:#010x} (return to)", e.elr_el1);
    println!("      Returning from exception...\n");
}
