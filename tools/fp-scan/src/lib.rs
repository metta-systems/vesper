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
    scan_image(bytes).map(|image| image.hits)
}

/// What a scan saw of an image, so a check can tell that it scanned something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scan {
    /// Words decoded in the executable segments (data words included).
    pub code_words: usize,
    /// The (mangled) names of the functions the symbol table defines.
    pub functions: Vec<String>,
    /// The FP/SIMD instructions found.
    pub hits: Vec<Hit>,
}

/// What an audit of one image requires.
#[derive(Clone, Copy, Debug, Default)]
pub struct Policy<'a> {
    /// A hit inside a function whose (mangled) name contains one of these is
    /// allowed; such a function must hold at least one hit.
    pub allow: &'a [String],
    /// The image must have hits (an image that executes FP on purpose).
    pub expect_hits: bool,
    /// The image must define a function that `allow` names.
    pub expect_allowed: bool,
}

/// Audit `scan` against `policy`: a line describing the passed image, or the
/// problems found.
///
/// Besides refusing hits outside the allowed functions, it refuses an audit
/// that saw nothing to check: an image with no code, an allowed function in
/// which no FP/SIMD instruction is found (the nucleus's own routines must be
/// seen, or the scan sees nothing), and, as `policy` asks, an image without
/// hits or without an allowed function.
///
/// # Errors
///
/// The problems found, one line each.
pub fn audit(scan: &Scan, policy: &Policy<'_>) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    if scan.code_words == 0 {
        problems.push(String::from("no executable code to scan"));
    }
    let is_allowed = |symbol: &str| {
        policy
            .allow
            .iter()
            .any(|allow| symbol.contains(allow.as_str()))
    };
    let passed = if policy.expect_hits {
        if scan.hits.is_empty() {
            problems.push(String::from(
                "no FP/SIMD instructions found, but some were expected",
            ));
        }
        format!(
            "{} FP/SIMD instructions found, as expected",
            scan.hits.len()
        )
    } else {
        let (permitted, refused): (Vec<_>, Vec<_>) = scan
            .hits
            .iter()
            .partition(|hit| hit.symbol.as_deref().is_some_and(is_allowed));
        problems.extend(
            refused
                .iter()
                .map(|hit| format!("FP/SIMD instruction {hit}")),
        );
        let allowed_functions: Vec<_> = scan
            .functions
            .iter()
            .filter(|function| is_allowed(function))
            .collect();
        for function in &allowed_functions {
            if !permitted
                .iter()
                .any(|hit| hit.symbol.as_deref() == Some(function.as_str()))
            {
                problems.push(format!(
                    "allowed function {function} holds no FP/SIMD instruction: the scan does not see it"
                ));
            }
        }
        if policy.expect_allowed && allowed_functions.is_empty() {
            problems.push(String::from(
                "defines no function the allow-list names, but it must",
            ));
        }
        if permitted.is_empty() {
            format!("integer-only ({} words scanned)", scan.code_words)
        } else {
            format!(
                "integer-only outside the allowed functions ({} instructions in {} of them, {} words scanned)",
                permitted.len(),
                allowed_functions.len(),
                scan.code_words
            )
        }
    };
    if problems.is_empty() {
        Ok(passed)
    } else {
        Err(problems)
    }
}

/// Scan the ELF image `bytes`: its FP/SIMD instructions, with how much code
/// was decoded and which functions it defines.
///
/// # Errors
///
/// If `bytes` is not an `AArch64` ELF64 image, or a segment is truncated.
pub fn scan_image(bytes: &[u8]) -> Result<Scan, ScanError> {
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
    let mut code_words = 0;
    for header in &elf.program_headers {
        if header.p_type != PT_LOAD || header.p_flags & PF_X == 0 {
            continue;
        }
        let start = usize::try_from(header.p_offset).map_err(|_too_large| ScanError::Truncated)?;
        let length = usize::try_from(header.p_filesz).map_err(|_too_large| ScanError::Truncated)?;
        let segment = bytes
            .get(start..start.checked_add(length).ok_or(ScanError::Truncated)?)
            .ok_or(ScanError::Truncated)?;
        code_words += segment.len() / 4;
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
    Ok(Scan {
        code_words,
        functions: functions
            .iter()
            .map(|&(_, _, name)| name.to_string())
            .collect(),
        hits,
    })
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

    /// A hit in `symbol`.
    fn hit(symbol: &str) -> Hit {
        Hit {
            address: 0x1000,
            word: 0x1E62_2800,
            class: FpClass::DataProcessing,
            symbol: Some(symbol.to_string()),
        }
    }

    /// A nucleus-like image: an integer function and two FP routines, with
    /// `hits`.
    fn kernel(hits: Vec<Hit>) -> Scan {
        Scan {
            code_words: 1000,
            functions: ["kernel_main", "fp_unit_save", "fp_unit_load"]
                .map(String::from)
                .to_vec(),
            hits,
        }
    }

    fn allow_fp_unit() -> Vec<String> {
        vec![String::from("fp_unit_")]
    }

    #[test]
    fn hits_in_every_allowed_function_pass() {
        let allow = allow_fp_unit();
        let policy = Policy {
            allow: &allow,
            expect_allowed: true,
            ..Policy::default()
        };
        let scan = kernel(vec![hit("fp_unit_save"), hit("fp_unit_load")]);
        audit(&scan, &policy).expect("both routines hold hits");
    }

    #[test]
    fn a_hit_outside_the_allowed_functions_fails() {
        let allow = allow_fp_unit();
        let policy = Policy {
            allow: &allow,
            ..Policy::default()
        };
        let scan = kernel(vec![
            hit("fp_unit_save"),
            hit("fp_unit_load"),
            hit("kernel_main"),
        ]);
        let problems = audit(&scan, &policy).unwrap_err();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("kernel_main"), "{problems:?}");
    }

    #[test]
    fn a_hit_without_a_symbol_fails() {
        let allow = allow_fp_unit();
        let policy = Policy {
            allow: &allow,
            ..Policy::default()
        };
        let mut stripped = hit("fp_unit_save");
        stripped.symbol = None;
        let scan = Scan {
            code_words: 1000,
            functions: Vec::new(),
            hits: vec![stripped],
        };
        audit(&scan, &policy).expect_err("an unattributed hit is refused");
    }

    #[test]
    fn an_allowed_function_without_hits_fails() {
        // The scan saw the routine but not its FP instructions: it is blind.
        let allow = allow_fp_unit();
        let policy = Policy {
            allow: &allow,
            ..Policy::default()
        };
        let problems = audit(&kernel(vec![hit("fp_unit_save")]), &policy).unwrap_err();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("fp_unit_load"), "{problems:?}");
        // No hits at all fails too, once per routine.
        assert_eq!(audit(&kernel(Vec::new()), &policy).unwrap_err().len(), 2);
    }

    #[test]
    fn an_image_without_code_fails() {
        let scan = Scan {
            code_words: 0,
            functions: Vec::new(),
            hits: Vec::new(),
        };
        audit(&scan, &Policy::default()).expect_err("nothing was scanned");
        // With code and nothing allowed, an integer-only image passes.
        let scan = Scan {
            code_words: 10,
            ..scan
        };
        audit(&scan, &Policy::default()).expect("integer-only");
    }

    #[test]
    fn a_missing_allowed_function_fails_when_expected() {
        let allow = allow_fp_unit();
        let scan = Scan {
            code_words: 10,
            functions: vec![String::from("kernel_main")],
            hits: Vec::new(),
        };
        let lenient = Policy {
            allow: &allow,
            ..Policy::default()
        };
        audit(&scan, &lenient).expect("no allowed function is required");
        let strict = Policy {
            expect_allowed: true,
            ..lenient
        };
        audit(&scan, &strict).expect_err("an allowed function is required");
    }

    #[test]
    fn expected_hits_must_be_found() {
        let policy = Policy {
            expect_hits: true,
            ..Policy::default()
        };
        let integer_only = Scan {
            code_words: 10,
            functions: Vec::new(),
            hits: Vec::new(),
        };
        audit(&integer_only, &policy).expect_err("hits are expected");
        let probe = Scan {
            hits: vec![hit("probe")],
            ..integer_only
        };
        audit(&probe, &policy).expect("the probe has hits");
    }
}
