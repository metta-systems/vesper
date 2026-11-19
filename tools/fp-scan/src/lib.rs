//! Find `AArch64` FP/SIMD instructions in an ELF image.
//!
//! Every 32-bit word of the executable `PT_LOAD` segments is decoded by its
//! A64 encoding class, except where the ELF's mapping symbols (`$d`) mark data
//! in code. A word is a hit when it is:
//!
//! - in the scalar FP and Advanced SIMD data-processing group, or a load or
//!   store of an FP/SIMD register: A64 bits 27 and 26 both set (`op0` `x111`,
//!   and `x1x0` with the `V` bit), which covers nothing else;
//! - an SVE instruction (`op0` `0010`);
//! - an `mrs` or `msr` of FPCR or FPSR.
//!
//! The same scan decides a component's FP flag in image-build and checks the
//! images in the Justfile's `_audit-fp-simd`, so the two cannot disagree.

use {
    goblin::elf::{
        Elf,
        header::EM_AARCH64,
        program_header::{PF_X, PT_LOAD},
    },
    std::fmt,
};

/// The encoding class of an FP/SIMD [`Hit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FpClass {
    /// Scalar FP or Advanced SIMD data processing (`fadd`, `movi`, `fmov`).
    DataProcessing,
    /// A load or store of FP/SIMD registers (`ldr d0`, `stp q0, q1`, `st1`).
    LoadStore,
    /// An SVE instruction.
    Sve,
    /// `mrs` or `msr` of FPCR or FPSR.
    ControlRegister,
}

impl fmt::Display for FpClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DataProcessing => "FP/SIMD data processing",
            Self::LoadStore => "FP/SIMD load/store",
            Self::Sve => "SVE",
            Self::ControlRegister => "FPCR/FPSR access",
        })
    }
}

/// The `mrs`/`msr` encoding with `Rt` and the direction bit (`L`, bit 21)
/// masked out.
const SYSTEM_REGISTER_MASK: u32 = 0xFFDF_FFE0;
/// `msr fpcr, x0`: `op0` 3, `op1` 3, `CRn` 4, `CRm` 4, `op2` 0.
const FPCR_ACCESS: u32 = 0xD51B_4400;
/// `msr fpsr, x0`: as FPCR with `op2` 1.
const FPSR_ACCESS: u32 = 0xD51B_4420;

/// The FP/SIMD class of the A64 instruction `word`, or `None` for any other
/// instruction.
pub const fn classify(word: u32) -> Option<FpClass> {
    let masked = word & SYSTEM_REGISTER_MASK;
    if masked == FPCR_ACCESS || masked == FPSR_ACCESS {
        return Some(FpClass::ControlRegister);
    }
    // Bits 27 and 26: `op0` x111 is FP/SIMD data processing; `op0` x1x0
    // is loads and stores, where bit 26 is `V`, the FP/SIMD register file.
    match (word >> 25) & 0b111 {
        0b111 => return Some(FpClass::DataProcessing),
        0b110 => return Some(FpClass::LoadStore),
        _ => {}
    }
    if (word >> 25) & 0b1111 == 0b0010 {
        return Some(FpClass::Sve);
    }
    None
}

/// Whether the words from an address on are code or data, from an ELF
/// mapping symbol (`$x` or `$d`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapping {
    Code,
    Data,
}

/// One FP/SIMD instruction found in an image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub address: u64,
    pub word: u32,
    pub class: FpClass,
    /// The function the word is in (its mangled name), if the image says.
    pub symbol: Option<String>,
}

impl fmt::Display for Hit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:#x}: {:08x} {} in {}",
            self.address,
            self.word,
            self.class,
            self.symbol.as_deref().unwrap_or("<unknown>")
        )
    }
}

/// Scan the code at `base`, `bytes` long, with the mapping symbols
/// `mappings` (address and kind, in any order): the address, word and class
/// of every FP/SIMD word outside data. Words before the first mapping symbol
/// are code; a trailing partial word is ignored.
pub fn scan_code(base: u64, bytes: &[u8], mappings: &[(u64, Mapping)]) -> Vec<(u64, u32, FpClass)> {
    let mut sorted = mappings.to_vec();
    sorted.sort_by_key(|&(address, _)| address);
    let mut next_mapping = 0;
    let mut current = Mapping::Code;
    let mut hits = Vec::new();
    let (words, _partial) = bytes.as_chunks::<4>();
    for (index, &chunk) in words.iter().enumerate() {
        let address = base + 4 * index as u64;
        while next_mapping < sorted.len() && sorted[next_mapping].0 <= address {
            current = sorted[next_mapping].1;
            next_mapping += 1;
        }
        if current == Mapping::Data {
            continue;
        }
        let word = u32::from_le_bytes(chunk);
        if let Some(class) = classify(word) {
            hits.push((address, word, class));
        }
    }
    hits
}

/// Why an image could not be scanned.
#[derive(Debug)]
pub enum ScanError {
    Parse(goblin::error::Error),
    NotAarch64,
    /// A segment's file range lies outside the file.
    Truncated,
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "not an ELF image: {error}"),
            Self::NotAarch64 => formatter.write_str("not an AArch64 ELF64 image"),
            Self::Truncated => formatter.write_str("a segment extends past the end of the file"),
        }
    }
}

impl std::error::Error for ScanError {}

/// Every FP/SIMD instruction in the executable segments of the ELF image
/// `bytes`, with the function it is in.
///
/// # Errors
///
/// If `bytes` is not an `AArch64` ELF64 image, or a segment is truncated.
pub fn scan(bytes: &[u8]) -> Result<Vec<Hit>, ScanError> {
    let elf = Elf::parse(bytes).map_err(ScanError::Parse)?;
    if !elf.is_64 || elf.header.e_machine != EM_AARCH64 {
        return Err(ScanError::NotAarch64);
    }
    let mut mappings = Vec::new();
    let mut functions = Vec::new();
    for symbol in &elf.syms {
        let Some(name) = elf.strtab.get_at(symbol.st_name) else {
            continue;
        };
        if name == "$x" || name.starts_with("$x.") {
            mappings.push((symbol.st_value, Mapping::Code));
        } else if name == "$d" || name.starts_with("$d.") {
            mappings.push((symbol.st_value, Mapping::Data));
        } else if symbol.is_function() && symbol.st_size > 0 {
            functions.push((symbol.st_value, symbol.st_size, name));
        }
    }
    let mut hits = Vec::new();
    for header in &elf.program_headers {
        if header.p_type != PT_LOAD || header.p_flags & PF_X == 0 {
            continue;
        }
        let start = usize::try_from(header.p_offset).map_err(|_too_large| ScanError::Truncated)?;
        let length = usize::try_from(header.p_filesz).map_err(|_too_large| ScanError::Truncated)?;
        let segment = bytes
            .get(start..start.checked_add(length).ok_or(ScanError::Truncated)?)
            .ok_or(ScanError::Truncated)?;
        for (address, word, class) in scan_code(header.p_vaddr, segment, &mappings) {
            let symbol = functions
                .iter()
                .find(|&&(value, size, _)| (value..value + size).contains(&address))
                .map(|&(_, _, name)| name.to_string());
            hits.push(Hit {
                address,
                word,
                class,
                symbol,
            });
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FP/SIMD instructions, each with its class (encodings as `rust-objdump`
    /// prints them for fp-worker and the nucleus's `fp_unit_*` routines).
    const FP_WORDS: [(u32, FpClass, &str); 13] = [
        (0x2F00_E400, FpClass::DataProcessing, "movi d0, #0"),
        (0x6F00_E408, FpClass::DataProcessing, "movi v8.2d, #0"),
        (0x1E6E_1001, FpClass::DataProcessing, "fmov d1, #1.0"),
        (0x7E61_D842, FpClass::DataProcessing, "ucvtf d2, d2"),
        (0x1E62_0842, FpClass::DataProcessing, "fmul d2, d2, d2"),
        (0x1E62_2800, FpClass::DataProcessing, "fadd d0, d0, d2"),
        (0xFD40_07E2, FpClass::LoadStore, "ldr d2, [sp, #8]"),
        (0xAD00_0400, FpClass::LoadStore, "stp q0, q1, [x0]"),
        (
            0x4C9F_2C08,
            FpClass::LoadStore,
            "st1 {v8.2d-v11.2d}, [x0], #64",
        ),
        (0xD53B_4401, FpClass::ControlRegister, "mrs x1, fpcr"),
        (0xD53B_4421, FpClass::ControlRegister, "mrs x1, fpsr"),
        (0xD51B_4401, FpClass::ControlRegister, "msr fpcr, x1"),
        (0x2518_E3E0, FpClass::Sve, "ptrue p0.b"),
    ];

    /// Integer instructions, including system-register accesses next to
    /// FPCR's encoding and loads and stores of general registers.
    const INTEGER_WORDS: [(u32, &str); 14] = [
        (0x5280_0029, "mov w9, #1"),
        (0x9100_0529, "add x9, x9, #1"),
        (0xF100_0400, "subs x0, x0, #1"),
        (0xF900_07E9, "str x9, [sp, #8]"),
        (0xB900_0401, "str w1, [x0, #4]"),
        (0xA9BF_7BFD, "stp x29, x30, [sp, #-16]!"),
        (0x4C9F_2C08 & !(1 << 26), "st1's encoding with V clear"),
        (0x54FF_FF01, "b.ne"),
        (0xD65F_03C0, "ret"),
        (0xD503_201F, "nop"),
        (0xD400_0001, "svc #0"),
        (0xD420_00E0, "brk #7"),
        (0xD53B_D040, "mrs x0, tpidr_el0"),
        (0x0000_0000, "udf #0"),
    ];

    #[test]
    fn every_fp_class_is_found() {
        for (word, class, text) in FP_WORDS {
            assert_eq!(classify(word), Some(class), "{text} ({word:#010x})");
        }
    }

    #[test]
    fn integer_instructions_are_not_hits() {
        for (word, text) in INTEGER_WORDS {
            assert_eq!(classify(word), None, "{text} ({word:#010x})");
        }
    }

    fn words(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    #[test]
    fn data_in_code_is_skipped() {
        // An integer function, then a literal pool holding 1.0 (whose high
        // word decodes as FP/SIMD data processing), then FP code again.
        let one_high = (1.0_f64.to_bits() >> 32) as u32;
        assert!(
            classify(one_high).is_some(),
            "the literal must look like FP"
        );
        let code = words(&[0x9100_0529, 0xD65F_03C0, 0, one_high, 0x1E62_2800]);
        let mappings = [
            (0x1000, Mapping::Code),
            (0x1008, Mapping::Data),
            (0x1010, Mapping::Code),
        ];
        assert_eq!(
            scan_code(0x1000, &code, &mappings),
            [(0x1010, 0x1E62_2800, FpClass::DataProcessing)]
        );
        // Without the mapping symbols the literal is a hit.
        assert_eq!(scan_code(0x1000, &code, &[]).len(), 2);
    }

    #[test]
    fn a_trailing_partial_word_is_ignored() {
        let mut code = words(&[0x1E62_2800]);
        code.extend([0xFF, 0xFF]);
        assert_eq!(scan_code(0, &code, &[]).len(), 1);
    }

    #[test]
    fn a_non_elf_image_is_an_error() {
        let error = scan(b"not an elf").unwrap_err();
        assert!(matches!(error, ScanError::Parse(_)), "{error}");
    }
}
