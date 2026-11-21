//! `fp-scan`: report the FP/SIMD instructions in `AArch64` ELF images.
//!
//! Exits with status 1 when an image fails its audit ([`fp_scan::audit`]): a
//! hit outside the functions named by `--allow`, an image with no code, an
//! allowed function without hits, or, with `--expect-hits` or
//! `--expect-allowed`, an image without hits or without an allowed function.
//! The Justfile's `_audit-fp-simd` runs it over every soft-float image.

use {argh::FromArgs, std::process::ExitCode};

/// Report the FP/SIMD instructions in `AArch64` ELF images.
#[derive(FromArgs)]
struct Arguments {
    /// a hit inside a function whose (mangled) name contains this text is
    /// allowed, and each such function must hold a hit; repeatable
    #[argh(option)]
    allow: Vec<String>,
    /// require every image to have a hit (a check that the scan still
    /// finds what it must)
    #[argh(switch)]
    expect_hits: bool,
    /// require every image to define a function that `--allow` names
    #[argh(switch)]
    expect_allowed: bool,
    /// the ELF images to scan
    #[argh(positional)]
    images: Vec<String>,
}

fn main() -> ExitCode {
    let arguments: Arguments = argh::from_env();
    let policy = fp_scan::Policy {
        allow: &arguments.allow,
        expect_hits: arguments.expect_hits,
        expect_allowed: arguments.expect_allowed,
    };
    let mut failed = false;
    for image in &arguments.images {
        let outcome = std::fs::read(image)
            .map_err(|error| vec![error.to_string()])
            .and_then(|bytes| fp_scan::scan_image(&bytes).map_err(|error| vec![error.to_string()]))
            .and_then(|scan| fp_scan::audit(&scan, &policy));
        match outcome {
            Ok(passed) => println!("✅ {image}: {passed}"),
            Err(problems) => {
                eprintln!("❌ {image}:");
                for problem in problems {
                    eprintln!("    {problem}");
                }
                failed = true;
            }
        }
    }
    if arguments.images.is_empty() {
        eprintln!("❌ no images to scan");
        failed = true;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
