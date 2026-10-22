//! `meddler`: check Meddle interface files and print the Rust code generated
//! from them, for experimenting with `.if` files outside a build.

use {
    argh::FromArgs,
    libmeddle::{Failure, SourceFile, is_imports_file},
    std::{
        fs,
        io::{self, IsTerminal, Write},
        path::PathBuf,
        process::ExitCode,
    },
};

/// Meddle interface compiler.
#[derive(FromArgs)]
struct Arguments {
    #[argh(subcommand)]
    command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
enum Command {
    Check(Check),
    Interface(Interface),
    Client(Client),
}

/// Check interface files (`name_vN.if`) and imports files (`imports.if`).
#[derive(FromArgs)]
#[argh(subcommand, name = "check")]
struct Check {
    /// directory searched for the interface files an imports file uses and
    /// the ones an `installs` clause names (repeatable; an imports file's own
    /// directory comes first; nothing else is searched, so pass `-I .` for the
    /// current directory)
    #[argh(option, short = 'I', long = "include")]
    include: Vec<PathBuf>,

    /// files to check
    #[argh(positional)]
    files: Vec<PathBuf>,
}

/// Print the interface crate's module generated from an interface file.
#[derive(FromArgs)]
#[argh(subcommand, name = "interface")]
struct Interface {
    /// directory searched for the interfaces an `installs` clause names
    /// (repeatable; pass `-I .` for the current directory)
    #[argh(option, short = 'I', long = "include")]
    include: Vec<PathBuf>,

    /// write to this file instead of standard output
    #[argh(option, short = 'o')]
    output: Option<PathBuf>,

    /// the interface file
    #[argh(positional)]
    file: PathBuf,
}

/// Print the client crate's module generated from an interface file.
#[derive(FromArgs)]
#[argh(subcommand, name = "client")]
struct Client {
    /// directory searched for the interfaces an `installs` clause names
    /// (repeatable; pass `-I .` for the current directory)
    #[argh(option, short = 'I', long = "include")]
    include: Vec<PathBuf>,

    /// name of the interface crate the client re-exports the types from
    #[argh(option)]
    interface_crate: String,

    /// write to this file instead of standard output
    #[argh(option, short = 'o')]
    output: Option<PathBuf>,

    /// the interface file
    #[argh(positional)]
    file: PathBuf,
}

/// Report `failure` on standard error.
fn report(failure: &Failure) {
    let color = io::stderr().is_terminal();
    eprint!("{}", failure.render(color));
}

fn check(arguments: &Check) -> ExitCode {
    let mut failed = false;
    for path in &arguments.files {
        let result = SourceFile::read(path).and_then(|file| {
            if is_imports_file(path) {
                let mut directories: Vec<PathBuf> =
                    path.parent().map(PathBuf::from).into_iter().collect();
                directories.extend(arguments.include.iter().cloned());
                file.imports(&directories)
                    .map(|uses| format!("{} uses {} interfaces", path.display(), uses.len()))
            } else {
                file.with_checked(&arguments.include, |checked| {
                    format!(
                        "{}: `{}` with {} functions, type code {:#018x}",
                        path.display(),
                        checked.interface().name.text,
                        checked.interface().methods.len(),
                        checked.type_code()
                    )
                })
            }
        });
        match result {
            Ok(summary) => println!("{summary}"),
            Err(failure) => {
                report(&failure);
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn emit(generated: Result<String, Failure>, output: Option<&PathBuf>) -> ExitCode {
    let source = match generated {
        Ok(source) => source,
        Err(failure) => {
            report(&failure);
            return ExitCode::FAILURE;
        }
    };
    let written = match output {
        Some(path) => fs::write(path, source),
        None => io::stdout().write_all(source.as_bytes()),
    };
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("meddler: cannot write the output: {error}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let arguments: Arguments = argh::from_env();
    match arguments.command {
        Command::Check(arguments) => check(&arguments),
        Command::Interface(arguments) => emit(
            libmeddle::generate_interface(&arguments.file, &arguments.include),
            arguments.output.as_ref(),
        ),
        Command::Client(arguments) => emit(
            libmeddle::generate_client(
                &arguments.file,
                &arguments.interface_crate,
                &arguments.include,
            ),
            arguments.output.as_ref(),
        ),
    }
}
