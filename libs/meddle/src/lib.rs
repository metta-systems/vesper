//! Meddle, the Metta interface definition language: parser, checks and the
//! Rust code generator.
//!
//! A component declares each interface it exports in its own file,
//! `interfaces/<name>_vN.if`, holding exactly that interface, and the
//! functions of other components it calls in `interfaces/imports.if`, a list
//! of `uses` declarations. From an interface file the generator produces two
//! crates' worth of code (see [`emit`]); [`build`] does it from a crate's
//! build script, and the `meddler` tool does it by hand.
//!
//! # Language
//!
//! ```text
//! file      = { interface | uses } ;
//! interface = { doc } [ "local" ] "interface" name "{" { member } "}" ;
//! member    = { doc } ( alias | enum | record | exception | data | method ) ;
//! data      = "data" number ";" ;
//! alias     = "type" type name ";" ;
//! enum      = "enum" name "{" name { "," name } [ "," ] "}" ;
//! record    = "record" name "{" { field ";" } "}" ;
//! exception = "exception" name "{" { field ";" } "}" ;
//! method    = name "(" [ field { "," field } ] ")"
//!             [ "returns" "(" [ field { "," field } ] ")" ]
//!             [ "raises" "(" name { "," name } ")" ] ";" ;
//! field     = { doc } type name ;
//! type      = name [ "&" ] ;
//! uses      = "uses" name "via" link { "," link } "{" name { "," name } [ "," ] "}" ;
//! link      = name | string | "compatible" string ;
//! ```
//!
//! `#` starts a comment, `##` a documentation comment for the item that
//! follows. Built-in types are `octet`, `card16`, `card32`, `card64`,
//! `int8`, `int16`, `int32`, `int64` and `boolean`. `T&` is a reference to
//! bytes in the interface's data area, passed as its address and length; it
//! is allowed only on parameters of `local` interfaces, which must declare the
//! area with `data <bytes>;` (whole pages, at most 64 KiB). Kickstart gives
//! each client of the interface one such area, mapped at the same address in
//! the client and its exporter. Interface names carry their version
//! (`name_vN`): a change that breaks callers makes a new version.
//!
//! A `uses` declaration names the instance it calls by a chain of links, read
//! from the device tree node of the component that uses it: each property of
//! the current node holds the phandle of the next. The first link may instead
//! be `compatible "string"`, the one enabled node with that compatible string,
//! for providers no property leads to.
//!
//! A call passes at most six argument words and answers with an outcome word
//! and one more word, so a function returns at most one one-word result and
//! an exception carries at most one word; see `libmeddle-runtime` for the
//! encoding.

pub mod ast;
pub mod build;
pub mod check;
pub mod diagnostic;
pub mod emit;
mod lexer;
mod parser;

use {
    ast::{File, Interface, Uses},
    check::Checked,
    diagnostic::Diagnostic,
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

pub use parser::parse;

/// Name of a component's file of `uses` declarations.
pub const IMPORTS_FILE: &str = "imports.if";

/// Diagnostics for one source file, with the source to render them against.
#[derive(Debug)]
pub struct Failure {
    pub path: PathBuf,
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl Failure {
    fn new(path: &Path, source: String, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            path: path.to_owned(),
            source,
            diagnostics,
        }
    }

    /// Every diagnostic rendered with its source excerpt.
    pub fn render(&self, color: bool) -> String {
        let path = self.path.display().to_string();
        if self.diagnostics.is_empty() {
            return format!("{path}: cannot read the file\n");
        }
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.render(&path, &self.source, color))
            .collect()
    }
}

/// A source file read from disk.
#[derive(Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub source: String,
}

impl SourceFile {
    /// Read `path`.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, Failure> {
        let path = path.as_ref();
        fs::read_to_string(path)
            .map(|source| Self {
                path: path.to_owned(),
                source,
            })
            .map_err(|error| {
                Failure::new(
                    path,
                    String::new(),
                    vec![Diagnostic::error(0..0, format!("cannot read: {error}"))],
                )
            })
    }

    /// The file name, for messages and generated headers.
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(
            || self.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    fn failure(&self, diagnostics: Vec<Diagnostic>) -> Failure {
        Failure::new(&self.path, self.source.clone(), diagnostics)
    }

    fn parse(&self) -> Result<File, Failure> {
        match parse(&self.source) {
            (Some(file), diagnostics) if diagnostics.is_empty() => Ok(file),
            (_, diagnostics) => Err(self.failure(diagnostics)),
        }
    }

    /// The one interface of an interface file, named after the file.
    pub fn interface(&self) -> Result<Interface, Failure> {
        let mut file = self.parse()?;
        let stem = self
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut diagnostics: Vec<Diagnostic> = file
            .uses
            .iter()
            .map(|uses| {
                Diagnostic::error(
                    uses.interface.span.clone(),
                    format!("`uses` belongs in {IMPORTS_FILE}, not in an interface file"),
                )
            })
            .collect();
        if file.interfaces.len() != 1 {
            let span = file
                .interfaces
                .get(1)
                .map_or(0..0, |second| second.name.span.clone());
            diagnostics.push(Diagnostic::error(
                span,
                format!(
                    "an interface file holds exactly one interface, found {}",
                    file.interfaces.len()
                ),
            ));
        }
        if let Some(interface) = file.interfaces.first()
            && interface.name.text != stem
        {
            diagnostics.push(Diagnostic::error(
                interface.name.span.clone(),
                format!(
                    "interface `{}` must be in `{}.if`, not `{}`",
                    interface.name.text,
                    interface.name.text,
                    self.name()
                ),
            ));
        }
        if !diagnostics.is_empty() {
            return Err(self.failure(diagnostics));
        }
        Ok(file.interfaces.swap_remove(0))
    }

    /// Check the interface of an interface file and hand it to `use_checked`.
    pub fn with_checked<Output>(
        &self,
        use_checked: impl FnOnce(&Checked<'_>) -> Output,
    ) -> Result<Output, Failure> {
        let interface = self.interface()?;
        let checked = check::check(&interface).map_err(|diagnostics| self.failure(diagnostics))?;
        Ok(use_checked(&checked))
    }

    /// The `uses` declarations of an imports file, checked against the
    /// interface files found in `interface_directories`.
    pub fn imports(&self, interface_directories: &[PathBuf]) -> Result<Vec<Uses>, Failure> {
        let file = self.parse()?;
        let mut diagnostics: Vec<Diagnostic> = file
            .interfaces
            .iter()
            .map(|interface| {
                Diagnostic::error(
                    interface.name.span.clone(),
                    format!(
                        "interfaces belong in their own `{}.if`, not in {IMPORTS_FILE}",
                        interface.name.text
                    ),
                )
            })
            .collect();
        let mut interfaces = Vec::new();
        for uses in &file.uses {
            let found = interface_directories
                .iter()
                .map(|directory| directory.join(format!("{}.if", uses.interface.text)))
                .find(|candidate| candidate.is_file());
            if let Some(path) = found {
                interfaces.push(SourceFile::read(&path)?.interface()?);
            }
        }
        let interface_references: Vec<&Interface> = interfaces.iter().collect();
        diagnostics.extend(check::check_uses(&file.uses, &interface_references));
        if diagnostics.is_empty() {
            Ok(file.uses)
        } else {
            Err(self.failure(diagnostics))
        }
    }
}

/// Whether `path` is a component's imports file.
pub fn is_imports_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == IMPORTS_FILE)
}

/// The interface crate's module generated from the interface file `path`.
pub fn generate_interface(path: impl AsRef<Path>) -> Result<String, Failure> {
    let file = SourceFile::read(path)?;
    file.with_checked(|checked| emit::interface_module(checked, &file.name()))
}

/// The client crate's module generated from the interface file `path`, for
/// the interface crate `interface_crate`.
pub fn generate_client(path: impl AsRef<Path>, interface_crate: &str) -> Result<String, Failure> {
    let file = SourceFile::read(path)?;
    file.with_checked(|checked| emit::client_module(checked, &file.name(), interface_crate))
}

/// What an image builder needs to know about an interface file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceSummary {
    pub name: String,
    pub type_code: u64,
    /// Bytes of the data area each client shares with the exporter (zero: none)
    pub data_bytes: u64,
    /// The interface's functions, in declaration order: a function's
    /// position is its index in import records
    pub functions: Vec<String>,
}

impl InterfaceSummary {
    /// The export name of the function at `index`.
    pub fn export_name(&self, index: usize) -> Option<String> {
        self.functions
            .get(index)
            .map(|function| check::export_name(&self.name, function))
    }
}

/// One step of a link chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkSummary {
    /// The node a property (of the client or of the previous step's node)
    /// names by phandle
    Property(String),
    /// The one enabled node with this `compatible` string
    Compatible(String),
}

/// The functions a component uses of one interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportSummary {
    pub interface: InterfaceSummary,
    /// The steps linking the client node to the exporter, in order
    pub links: Vec<LinkSummary>,
    /// Indices of the used functions in `interface.functions`
    pub functions: Vec<usize>,
}

/// Summarize the interface file `path`.
pub fn summarize_interface(path: impl AsRef<Path>) -> Result<InterfaceSummary, Failure> {
    SourceFile::read(path)?.with_checked(|checked| InterfaceSummary {
        name: checked.interface().name.text.clone(),
        type_code: checked.type_code(),
        data_bytes: checked.data_bytes(),
        functions: checked
            .interface()
            .methods
            .iter()
            .map(|method| method.name.text.clone())
            .collect(),
    })
}

/// Summarize the imports file `path`, finding the interfaces it uses in
/// `interface_directories`.
pub fn summarize_imports(
    path: impl AsRef<Path>,
    interface_directories: &[PathBuf],
) -> Result<Vec<ImportSummary>, Failure> {
    let uses = SourceFile::read(path)?.imports(interface_directories)?;
    uses.iter()
        .map(|declaration| {
            // `imports` checked that the file exists and names these functions.
            let path = interface_directories
                .iter()
                .map(|directory| directory.join(format!("{}.if", declaration.interface.text)))
                .find(|candidate| candidate.is_file())
                .unwrap_or_default();
            let interface = summarize_interface(path)?;
            let functions = declaration
                .functions
                .iter()
                .filter_map(|function| {
                    interface
                        .functions
                        .iter()
                        .position(|name| *name == function.text)
                })
                .collect();
            Ok(ImportSummary {
                links: declaration
                    .links
                    .iter()
                    .map(|link| match link {
                        ast::Link::Property(name) => LinkSummary::Property(name.text.clone()),
                        ast::Link::Compatible(name) => LinkSummary::Compatible(name.text.clone()),
                    })
                    .collect(),
                interface,
                functions,
            })
        })
        .collect()
}

/// The interface files of a component's interface directory: every `.if`
/// file except [`IMPORTS_FILE`], sorted by name.
pub fn interface_files(directory: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension().is_some_and(|extension| extension == "if")
                        && !is_imports_file(path)
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}
