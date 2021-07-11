use {
    crate::println,
    cortex_a::registers::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1},
    tock_registers::interfaces::Readable,
};

/// Parse the ID_AA64MMFR0_EL1 register for runtime information about supported MMU features.
/// Print the current state of TCR register.
pub fn print_features() {
    let sctlr = SCTLR_EL1.extract();

    if let Some(SCTLR_EL1::M::Value::Enable) = sctlr.read_as_enum(SCTLR_EL1::M) {
        println!("[i] MMU currently enabled");
    }

    if let Some(SCTLR_EL1::I::Value::Cacheable) = sctlr.read_as_enum(SCTLR_EL1::I) {
        println!("[i] MMU I-cache enabled");
    }

    if let Some(SCTLR_EL1::C::Value::Cacheable) = sctlr.read_as_enum(SCTLR_EL1::C) {
        println!("[i] MMU D-cache enabled");
    }

    let mmfr = ID_AA64MMFR0_EL1.extract();

    if let Some(ID_AA64MMFR0_EL1::TGran4::Value::Supported) =
        mmfr.read_as_enum(ID_AA64MMFR0_EL1::TGran4)
    {
        println!("[i] MMU: 4 KiB granule supported!");
    }

    if let Some(ID_AA64MMFR0_EL1::TGran16::Value::Supported) =
        mmfr.read_as_enum(ID_AA64MMFR0_EL1::TGran16)
    {
        println!("[i] MMU: 16 KiB granule supported!");
    }

    if let Some(ID_AA64MMFR0_EL1::TGran64::Value::Supported) =
        mmfr.read_as_enum(ID_AA64MMFR0_EL1::TGran64)
    {
        println!("[i] MMU: 64 KiB granule supported!");
    }

    match mmfr.read_as_enum(ID_AA64MMFR0_EL1::ASIDBits) {
        Some(ID_AA64MMFR0_EL1::ASIDBits::Value::Bits_16) => {
            println!("[i] MMU: 16 bit ASIDs supported!")
        }
        Some(ID_AA64MMFR0_EL1::ASIDBits::Value::Bits_8) => {
            println!("[i] MMU: 8 bit ASIDs supported!")
        }
        _ => println!("[i] MMU: Invalid ASID bits specified!"),
    }

    match mmfr.read_as_enum(ID_AA64MMFR0_EL1::PARange) {
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_32) => {
            println!("[i] MMU: Up to 32 Bit physical address range supported!")
        }
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_36) => {
            println!("[i] MMU: Up to 36 Bit physical address range supported!")
        }
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_40) => {
            println!("[i] MMU: Up to 40 Bit physical address range supported!")
        }
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_42) => {
            println!("[i] MMU: Up to 42 Bit physical address range supported!")
        }
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_44) => {
            println!("[i] MMU: Up to 44 Bit physical address range supported!")
        }
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_48) => {
            println!("[i] MMU: Up to 48 Bit physical address range supported!")
        }
        Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_52) => {
            println!("[i] MMU: Up to 52 Bit physical address range supported!")
        }
        _ => println!("[i] MMU: Invalid PARange specified!"),
    }

    let tcr = TCR_EL1.extract();

    match tcr.read_as_enum(TCR_EL1::IPS) {
        Some(TCR_EL1::IPS::Value::Bits_32) => {
            println!("[i] MMU: 32 Bit intermediate physical address size supported!")
        }
        Some(TCR_EL1::IPS::Value::Bits_36) => {
            println!("[i] MMU: 36 Bit intermediate physical address size supported!")
        }
        Some(TCR_EL1::IPS::Value::Bits_40) => {
            println!("[i] MMU: 40 Bit intermediate physical address size supported!")
        }
        Some(TCR_EL1::IPS::Value::Bits_42) => {
            println!("[i] MMU: 42 Bit intermediate physical address size supported!")
        }
        Some(TCR_EL1::IPS::Value::Bits_44) => {
            println!("[i] MMU: 44 Bit intermediate physical address size supported!")
        }
        Some(TCR_EL1::IPS::Value::Bits_48) => {
            println!("[i] MMU: 48 Bit intermediate physical address size supported!")
        }
        Some(TCR_EL1::IPS::Value::Bits_52) => {
            println!("[i] MMU: 52 Bit intermediate physical address size supported!")
        }
        _ => println!("[i] MMU: Invalid IPS specified!"),
    }

    match tcr.read_as_enum(TCR_EL1::TG0) {
        Some(TCR_EL1::TG0::Value::KiB_4) => println!("[i] MMU: TTBR0 4 KiB granule active!"),
        Some(TCR_EL1::TG0::Value::KiB_16) => println!("[i] MMU: TTBR0 16 KiB granule active!"),
        Some(TCR_EL1::TG0::Value::KiB_64) => println!("[i] MMU: TTBR0 64 KiB granule active!"),
        _ => println!("[i] MMU: Invalid TTBR0 granule size specified!"),
    }

    let t0sz = tcr.read(TCR_EL1::T0SZ);
    println!("[i] MMU: T0sz = 64-{} = {} bits", t0sz, 64 - t0sz);

    match tcr.read_as_enum(TCR_EL1::TG1) {
        Some(TCR_EL1::TG1::Value::KiB_4) => println!("[i] MMU: TTBR1 4 KiB granule active!"),
        Some(TCR_EL1::TG1::Value::KiB_16) => println!("[i] MMU: TTBR1 16 KiB granule active!"),
        Some(TCR_EL1::TG1::Value::KiB_64) => println!("[i] MMU: TTBR1 64 KiB granule active!"),
        _ => println!("[i] MMU: Invalid TTBR1 granule size specified!"),
    }

    let t1sz = tcr.read(TCR_EL1::T1SZ);
    println!("[i] MMU: T1sz = 64-{} = {} bits", t1sz, 64 - t1sz);
}
