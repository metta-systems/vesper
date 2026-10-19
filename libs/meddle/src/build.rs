//! Generate interface and client crates from a build script.
//!
//! An interface crate's `build.rs` calls [`interface_crate`] and its
//! `lib.rs` includes the result at the crate root:
//!
//! ```ignore
//! include!(concat!(env!("OUT_DIR"), "/mailbox_v1.rs"));
//! ```
//!
//! A client crate does the same with [`client_crate`].

use {
    crate::Failure,
    std::{
        env, fs,
        path::{Path, PathBuf},
    },
};

/// Generate the interface crate's module from `interface_file` (relative
/// to the crate's manifest directory) into `OUT_DIR`. `include` lists the
/// directories (also relative to the manifest directory) searched for the
/// interfaces the file's `installs` clauses name; nothing else is searched, so
/// a file that installs another interface lists the directory holding it,
/// `"."` included if it is the file's own.
///
/// # Panics
/// On errors in the interface file, with the rendered diagnostics, which
/// fails the build.
pub fn interface_crate(interface_file: impl AsRef<Path>, include: &[&str]) {
    let path = manifest_relative(interface_file.as_ref());
    let include = include_directories(include);
    write_output(&path, crate::generate_interface(&path, &include));
    rerun_for_includes(&include);
}

/// Generate the client crate's module from `interface_file` (relative to
/// the crate's manifest directory) into `OUT_DIR`; the generated code
/// re-exports the interface's types from `interface_crate`. `include` is as for
/// [`interface_crate`].
///
/// # Panics
/// On errors in the interface file, with the rendered diagnostics, which
/// fails the build.
pub fn client_crate(interface_file: impl AsRef<Path>, interface_crate: &str, include: &[&str]) {
    let path = manifest_relative(interface_file.as_ref());
    let include = include_directories(include);
    write_output(
        &path,
        crate::generate_client(&path, interface_crate, &include),
    );
    rerun_for_includes(&include);
}

fn include_directories(include: &[&str]) -> Vec<PathBuf> {
    include
        .iter()
        .map(|directory| manifest_relative(Path::new(directory)))
        .collect()
}

fn rerun_for_includes(include: &[PathBuf]) {
    for directory in include {
        println!("cargo::rerun-if-changed={}", directory.display());
    }
}

fn manifest_relative(path: &Path) -> PathBuf {
    env::var_os("CARGO_MANIFEST_DIR").map_or_else(
        || path.to_owned(),
        |directory| PathBuf::from(directory).join(path),
    )
}

fn write_output(interface_file: &Path, generated: Result<String, Failure>) {
    println!("cargo::rerun-if-changed={}", interface_file.display());
    let source = match generated {
        Ok(source) => source,
        Err(failure) => panic!("\n{}", failure.render(false)),
    };
    let stem = interface_file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let out_directory = env::var_os("OUT_DIR").map(PathBuf::from);
    let Some(out_directory) = out_directory else {
        panic!("OUT_DIR is not set: call this from a build script");
    };
    let output = out_directory.join(format!("{stem}.rs"));
    if let Err(error) = fs::write(&output, source) {
        panic!("cannot write {}: {error}", output.display());
    }
}
