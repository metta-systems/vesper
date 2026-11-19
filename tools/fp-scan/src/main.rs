//! `fp-scan`: report the FP/SIMD instructions in `AArch64` ELF images.
//!
//! Exits with status 1 when an image has a hit outside the functions named
//! by `--allow`, or, with `--expect-hits`, when an image has none. The
//! Justfile's `_audit-fp-simd` runs it over every integer-only image.

use {argh::FromArgs, std::process::ExitCode};

/// Report the FP/SIMD instructions in `AArch64` ELF images.
#[derive(FromArgs)]
struct Arguments {
    /// a hit inside a function whose (mangled) name contains this text is
    /// allowed; repeatable
    #[argh(option)]
    allow: Vec<String>,
    /// require every image to have a hit (a check that the scan still
    /// finds what it must)
    #[argh(switch)]
    expect_hits: bool,
    /// the ELF images to scan
    #[argh(positional)]
    images: Vec<String>,
}

fn main() -> ExitCode {
    let arguments: Arguments = argh::from_env();
    let mut failed = false;
    for image in &arguments.images {
        let hits = match std::fs::read(image)
            .map_err(|error| error.to_string())
            .and_then(|bytes| fp_scan::scan(&bytes).map_err(|error| error.to_string()))
        {
            Ok(hits) => hits,
            Err(error) => {
                eprintln!("❌ {image}: {error}");
                failed = true;
                continue;
            }
        };
        if arguments.expect_hits {
            if hits.is_empty() {
                eprintln!("❌ {image}: no FP/SIMD instructions found, but some were expected");
                failed = true;
            } else {
                println!(
                    "✅ {image}: {} FP/SIMD instructions found, as expected",
                    hits.len()
                );
            }
            continue;
        }
        let allowed = |hit: &fp_scan::Hit| {
            hit.symbol.as_deref().is_some_and(|symbol| {
                arguments
                    .allow
                    .iter()
                    .any(|allow| symbol.contains(allow.as_str()))
            })
        };
        let (permitted, refused): (Vec<_>, Vec<_>) = hits.iter().partition(|hit| allowed(hit));
        if refused.is_empty() {
            if permitted.is_empty() {
                println!("✅ {image}: integer-only");
            } else {
                println!(
                    "✅ {image}: integer-only outside the allowed functions ({} instructions there)",
                    permitted.len()
                );
            }
        } else {
            eprintln!("❌ {image} contains FP/SIMD instructions:");
            for hit in refused {
                eprintln!("    {hit}");
            }
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
