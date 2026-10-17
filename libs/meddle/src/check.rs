//! Semantic checks of a parsed interface, and what the emitter needs to know
//! about its types.

use {
    crate::{
        ast::{self, Field, Interface, Name, TypeKind, TypeReference, Uses},
        diagnostic::Diagnostic,
    },
    std::{
        collections::HashMap,
        fmt::{self, Write as _},
    },
};

/// Argument words of a call (`x2..x7`).
pub const ARGUMENT_WORDS: usize = 6;

/// Words a call returns besides its outcome: the result or exception field.
pub const ANSWER_WORDS: usize = 1;

/// Largest data area an interface may declare: it is mapped in both the client
/// and the exporter for every pair, in a window of 2 MiB.
pub const MAX_DATA_BYTES: u64 = 64 * 1024;

/// Largest key-placement window an interface may declare: slots of the
/// exporter's table reserved for each client.
pub const MAX_PLACEMENT_SLOTS: u64 = 8;

/// Bytes of a page: data areas are whole pages.
pub const PAGE_BYTES: u64 = 4096;

/// Longest export name the image builder records.
pub const EXPORT_NAME_BYTES: usize = 32;

/// Names the emitter defines in every interface module.
const GENERATED_NAMES: [&str; 5] = ["Exception", "Error", "Client", "Bindings", "TYPE_CODE"];

/// A built-in Meddle type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Octet,
    Card16,
    Card32,
    Card64,
    Int8,
    Int16,
    Int32,
    Int64,
    Boolean,
}

impl Builtin {
    /// The built-in type called `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "octet" => Self::Octet,
            "card16" => Self::Card16,
            "card32" => Self::Card32,
            "card64" => Self::Card64,
            "int8" => Self::Int8,
            "int16" => Self::Int16,
            "int32" => Self::Int32,
            "int64" => Self::Int64,
            "boolean" => Self::Boolean,
            _ => return None,
        })
    }

    /// The Rust type it maps to.
    pub const fn rust_name(self) -> &'static str {
        match self {
            Self::Octet => "u8",
            Self::Card16 => "u16",
            Self::Card32 => "u32",
            Self::Card64 => "u64",
            Self::Int8 => "i8",
            Self::Int16 => "i16",
            Self::Int32 => "i32",
            Self::Int64 => "i64",
            Self::Boolean => "bool",
        }
    }
}

/// The kinds of key a function may lend: the closed set of kernel kinds a
/// capability can be lent as, so interfaces are strongly typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LendKind {
    KeyTable,
    Frame,
}

impl LendKind {
    /// The names `lends` accepts, in the order diagnostics list them.
    pub const NAMES: [&'static str; 2] = ["key_table", "frame"];

    /// The kind called `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "key_table" => Some(Self::KeyTable),
            "frame" => Some(Self::Frame),
            _ => None,
        }
    }

    /// The kind's name in an interface file.
    pub const fn name(self) -> &'static str {
        match self {
            Self::KeyTable => "key_table",
            Self::Frame => "frame",
        }
    }

    /// The typed key handle the generated code uses for it.
    pub const fn rust_name(self) -> &'static str {
        match self {
            Self::KeyTable => "KeyTableKey",
            Self::Frame => "FrameKey",
        }
    }
}

/// What a type name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    Builtin(Builtin),
    /// Index of a type the interface defines
    Defined(usize),
}

/// An interface that passed every check, with its types' word counts.
pub struct Checked<'interface> {
    interface: &'interface Interface,
    /// Words of each defined type, by definition index
    words: Vec<usize>,
}

impl<'interface> Checked<'interface> {
    /// The interface.
    pub const fn interface(&self) -> &'interface Interface {
        self.interface
    }

    /// What the type name `name` refers to.
    pub fn resolve(&self, name: &str) -> Option<Resolved> {
        resolve(self.interface, name)
    }

    /// Bytes of the data area each client shares with the exporter; zero when
    /// the interface passes no references.
    pub fn data_bytes(&self) -> u64 {
        self.interface
            .data
            .as_ref()
            .map_or(0, |declaration| declaration.bytes)
    }

    /// Slots of the exporter's table each client gets for lent keys; zero
    /// when no function lends a key.
    pub fn placement_slots(&self) -> u64 {
        self.interface
            .placement
            .as_ref()
            .map_or(0, |declaration| declaration.slots)
    }

    /// The key `method` lends, with its kind, if it lends one.
    pub fn lent_key<'method>(
        &self,
        method: &'method ast::Method,
    ) -> Option<(LendKind, &'method Name)> {
        method
            .lends
            .first()
            .and_then(|lent| LendKind::from_name(&lent.kind.text).map(|kind| (kind, &lent.name)))
    }

    /// Words a value of `ty` takes in a call.
    pub fn words_of(&self, ty: &TypeReference) -> usize {
        if ty.reference {
            return 2;
        }
        match self.resolve(&ty.name.text) {
            Some(Resolved::Defined(index)) => self.words[index],
            Some(Resolved::Builtin(_)) | None => 1,
        }
    }

    /// Words of `fields` together.
    pub fn words_of_fields(&self, fields: &[Field]) -> usize {
        fields.iter().map(|field| self.words_of(&field.ty)).sum()
    }

    /// Index of the exception called `name`.
    pub fn exception_index(&self, name: &str) -> Option<usize> {
        self.interface
            .exceptions
            .iter()
            .position(|exception| exception.name.text == name)
    }

    /// The interface's type code: a 48-bit fingerprint of everything that
    /// defines its wire behavior, in the top bits; the low 16 bits are zero.
    pub fn type_code(&self) -> u64 {
        seahash::hash(fingerprint_text(self.interface).as_bytes()) & !0xffff
    }
}

/// The export name of `method` in `interface`.
pub fn export_name(interface: &str, method: &str) -> String {
    format!("{interface}_{method}")
}

/// The `UpperCamelCase` form of a `snake_case` name.
pub fn camel_case(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        })
        .collect()
}

fn resolve(interface: &Interface, name: &str) -> Option<Resolved> {
    Builtin::from_name(name).map(Resolved::Builtin).or_else(|| {
        interface
            .types
            .iter()
            .position(|definition| definition.name.text == name)
            .map(Resolved::Defined)
    })
}

/// Check `interface`.
pub fn check(interface: &Interface) -> Result<Checked<'_>, Vec<Diagnostic>> {
    let mut pass = Checker {
        interface,
        diagnostics: Vec::new(),
    };
    pass.interface_name();
    pass.unique_names();
    pass.identifiers();
    pass.type_references();
    pass.data_area();
    pass.lends();
    pass.placement_window();
    let words = pass.word_counts();
    let checked = Checked { interface, words };
    pass.members(&checked);
    if pass.diagnostics.is_empty() {
        Ok(checked)
    } else {
        Err(pass.diagnostics)
    }
}

/// Check that each `uses` names functions its interface (one of
/// `interfaces`) declares.
pub fn check_uses(uses: &[Uses], interfaces: &[&Interface]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen_interfaces: HashMap<&str, &Name> = HashMap::new();
    for declaration in uses {
        let interface_name = &declaration.interface;
        if let Some(first) = seen_interfaces.insert(&interface_name.text, interface_name) {
            diagnostics.push(
                Diagnostic::error(
                    interface_name.span.clone(),
                    format!("`{}` is used twice", interface_name.text),
                )
                .with_related(first.span.clone(), "first used here"),
            );
        }
        let Some(interface) = interfaces
            .iter()
            .find(|interface| interface.name.text == interface_name.text)
        else {
            diagnostics.push(Diagnostic::error(
                interface_name.span.clone(),
                format!("no interface file `{}.if` found", interface_name.text),
            ));
            continue;
        };
        duplicates(&declaration.functions, "function", &mut diagnostics);
        for link in declaration.links.iter().skip(1) {
            if let ast::Link::Compatible(name) = link {
                diagnostics.push(Diagnostic::error(
                    name.span.clone(),
                    "a `compatible` link can only start a chain: later steps follow properties",
                ));
            }
        }
        for function in &declaration.functions {
            if !interface
                .methods
                .iter()
                .any(|method| method.name.text == function.text)
            {
                diagnostics.push(Diagnostic::error(
                    function.span.clone(),
                    format!(
                        "`{}` has no function `{}`",
                        interface.name.text, function.text
                    ),
                ));
            }
        }
    }
    diagnostics
}

/// Report names in `names` that repeat an earlier one.
fn duplicates<'name>(
    names: impl IntoIterator<Item = &'name Name>,
    what: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen: HashMap<&str, &Name> = HashMap::new();
    for name in names {
        if let Some(first) = seen.insert(&name.text, name) {
            diagnostics.push(
                Diagnostic::error(
                    name.span.clone(),
                    format!("{what} `{}` is defined twice", name.text),
                )
                .with_related(first.span.clone(), "first defined here"),
            );
        }
    }
}

struct Checker<'interface> {
    interface: &'interface Interface,
    diagnostics: Vec<Diagnostic>,
}

impl<'interface> Checker<'interface> {
    fn error(&mut self, name: &Name, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(name.span.clone(), message));
    }

    /// `name_vN`: interfaces are versioned, and a breaking change makes a new
    /// version.
    fn interface_name(&mut self) {
        let name = &self.interface.name;
        let versioned = name.text.rsplit_once("_v").is_some_and(|(base, version)| {
            !base.is_empty()
                && !version.is_empty()
                && version.bytes().all(|byte| byte.is_ascii_digit())
        });
        if !versioned {
            self.error(
                name,
                format!(
                    "interface `{}` has no version: name it `{}_v1`",
                    name.text, name.text
                ),
            );
        }
    }

    fn unique_names(&mut self) {
        let interface = self.interface;
        duplicates(
            interface
                .types
                .iter()
                .map(|definition| &definition.name)
                .chain(interface.exceptions.iter().map(|exception| &exception.name)),
            "type",
            &mut self.diagnostics,
        );
        duplicates(
            interface.methods.iter().map(|method| &method.name),
            "function",
            &mut self.diagnostics,
        );
        for definition in &interface.types {
            match &definition.kind {
                TypeKind::Enum(values) => duplicates(values, "value", &mut self.diagnostics),
                TypeKind::Record(fields) => {
                    duplicates(
                        fields.iter().map(|field| &field.name),
                        "field",
                        &mut self.diagnostics,
                    );
                }
                TypeKind::Alias(_) => {}
            }
        }
        for exception in &interface.exceptions {
            duplicates(
                exception.fields.iter().map(|field| &field.name),
                "field",
                &mut self.diagnostics,
            );
        }
        for method in &interface.methods {
            duplicates(
                method
                    .parameters
                    .iter()
                    .chain(&method.results)
                    .map(|field| &field.name)
                    .chain(method.lends.iter().map(|lent| &lent.name)),
                "parameter",
                &mut self.diagnostics,
            );
            duplicates(&method.raises, "raised exception", &mut self.diagnostics);
        }
    }

    /// Every name becomes a Rust identifier: as written for functions, fields
    /// and parameters, in `UpperCamelCase` for types, values and exceptions.
    fn identifiers(&mut self) {
        let interface = self.interface;
        let trait_name = camel_case(&interface.name.text);
        let mut as_written: Vec<&Name> = vec![&interface.name];
        let mut camel: Vec<&Name> = Vec::new();
        for definition in &interface.types {
            camel.push(&definition.name);
            match &definition.kind {
                TypeKind::Enum(values) => camel.extend(values),
                TypeKind::Record(fields) => {
                    as_written.extend(fields.iter().map(|field| &field.name));
                }
                TypeKind::Alias(_) => {}
            }
        }
        for exception in &interface.exceptions {
            camel.push(&exception.name);
            as_written.extend(exception.fields.iter().map(|field| &field.name));
        }
        for method in &interface.methods {
            as_written.push(&method.name);
            as_written.extend(
                method
                    .parameters
                    .iter()
                    .chain(&method.results)
                    .map(|field| &field.name)
                    .chain(method.lends.iter().map(|lent| &lent.name)),
            );
        }
        for name in as_written {
            if syn::parse_str::<syn::Ident>(&name.text).is_err() {
                self.error(name, format!("`{}` is a reserved word in Rust", name.text));
            }
        }
        for name in camel {
            let rust_name = camel_case(&name.text);
            if syn::parse_str::<syn::Ident>(&rust_name).is_err() {
                self.error(
                    name,
                    format!(
                        "`{}` becomes `{rust_name}`, a reserved word in Rust",
                        name.text
                    ),
                );
            } else if GENERATED_NAMES.contains(&rust_name.as_str()) || rust_name == trait_name {
                self.error(
                    name,
                    format!(
                        "`{}` becomes `{rust_name}`, which the generated code defines",
                        name.text
                    ),
                );
            }
        }
    }

    fn type_references(&mut self) {
        let interface = self.interface;

        for definition in &interface.types {
            match &definition.kind {
                TypeKind::Alias(target) => self.type_reference(target, false),
                TypeKind::Record(fields) => {
                    for field in fields {
                        self.type_reference(&field.ty, false);
                    }
                }
                TypeKind::Enum(values) => {
                    if values.is_empty() {
                        self.error(
                            &definition.name,
                            format!("enum `{}` has no values", definition.name.text),
                        );
                    }
                }
            }
        }
        for exception in &interface.exceptions {
            for field in &exception.fields {
                self.type_reference(&field.ty, false);
            }
        }
        for method in &interface.methods {
            for parameter in &method.parameters {
                self.type_reference(&parameter.ty, true);
                if parameter.ty.reference && !interface.local {
                    self.error(
                        &parameter.ty.name,
                        "references (`&`) need a `local` interface: they name memory in an xbuf data area",
                    );
                }
            }
            for result in &method.results {
                self.type_reference(&result.ty, false);
            }
            for raised in &method.raises {
                if !interface
                    .exceptions
                    .iter()
                    .any(|exception| exception.name.text == raised.text)
                {
                    self.error(
                        raised,
                        format!("no exception `{}` in this interface", raised.text),
                    );
                }
            }
        }
    }

    /// `ty` names a known type, and is a reference only where `reference_allowed`.
    fn type_reference(&mut self, ty: &TypeReference, reference_allowed: bool) {
        if resolve(self.interface, &ty.name.text).is_none() {
            self.error(&ty.name, format!("unknown type `{}`", ty.name.text));
        }
        if ty.reference && !reference_allowed {
            self.error(
                &ty.name,
                "references (`&`) are allowed only on function parameters",
            );
        }
    }

    /// Words of every defined type; a record that contains itself is an error
    /// and counts as one word.
    fn word_counts(&mut self) -> Vec<usize> {
        let count = self.interface.types.len();
        let mut words: Vec<Option<usize>> = vec![None; count];
        let mut visiting = vec![false; count];
        for index in 0..count {
            self.words_of_definition(index, &mut words, &mut visiting);
        }
        words.into_iter().map(|words| words.unwrap_or(1)).collect()
    }

    fn words_of_definition(
        &mut self,
        index: usize,
        words: &mut [Option<usize>],
        visiting: &mut [bool],
    ) -> usize {
        if let Some(known) = words[index] {
            return known;
        }
        let definition = &self.interface.types[index];
        if visiting[index] {
            self.error(
                &definition.name,
                format!("type `{}` contains itself", definition.name.text),
            );
            words[index] = Some(1);
            return 1;
        }
        visiting[index] = true;
        let total = match &definition.kind {
            TypeKind::Enum(_) => 1,
            TypeKind::Alias(target) => self.words_of_reference(target, words, visiting),
            TypeKind::Record(fields) => fields
                .iter()
                .map(|field| self.words_of_reference(&field.ty, words, visiting))
                .sum(),
        };
        visiting[index] = false;
        words[index] = Some(total);
        total
    }

    fn words_of_reference(
        &mut self,
        ty: &TypeReference,
        words: &mut [Option<usize>],
        visiting: &mut [bool],
    ) -> usize {
        if ty.reference {
            return 2;
        }
        match resolve(self.interface, &ty.name.text) {
            Some(Resolved::Defined(index)) => self.words_of_definition(index, words, visiting),
            Some(Resolved::Builtin(_)) | None => 1,
        }
    }

    /// A `data` declaration and the functions taking references go together:
    /// a reference names bytes of the data area, which has a declared size.
    fn data_area(&mut self) {
        let interface = self.interface;
        let takes_reference = interface.methods.iter().any(|method| {
            method
                .parameters
                .iter()
                .any(|parameter| parameter.ty.reference)
        });
        match (&interface.data, takes_reference) {
            (Some(declaration), _) if declaration.bytes == 0 => {
                self.diagnostics.push(Diagnostic::error(
                    declaration.span.clone(),
                    "a data area needs at least one page",
                ));
            }
            (Some(declaration), _)
                if declaration.bytes % PAGE_BYTES != 0 || declaration.bytes > MAX_DATA_BYTES =>
            {
                self.diagnostics.push(Diagnostic::error(
                    declaration.span.clone(),
                    format!(
                        "a data area is whole {PAGE_BYTES}-byte pages, at most {MAX_DATA_BYTES} bytes"
                    ),
                ));
            }
            (Some(declaration), false) => {
                self.diagnostics.push(Diagnostic::error(
                    declaration.span.clone(),
                    "no function takes a reference (`&`), so the data area is never used",
                ));
            }
            (None, true) => {
                self.error(
                    &interface.name,
                    "functions taking references (`&`) need a `data <bytes>;` area to point into",
                );
            }
            (Some(_), true) | (None, false) => {}
        }
    }

    /// `lends` clauses: at most one key of a known kind, in a `local`
    /// interface (a key names a capability of this machine).
    fn lends(&mut self) {
        let interface = self.interface;
        for method in &interface.methods {
            for lent in method.lends.iter().skip(1) {
                self.error(
                    &lent.name,
                    format!(
                        "`{}` lends more than one key; a call lends at most one",
                        method.name.text
                    ),
                );
            }
            for lent in &method.lends {
                if LendKind::from_name(&lent.kind.text).is_none() {
                    self.error(
                        &lent.kind,
                        format!(
                            "unknown kind of key `{}`; a function lends a `{}`",
                            lent.kind.text,
                            LendKind::NAMES.join("` or a `")
                        ),
                    );
                }
                if !interface.local {
                    self.error(
                        &lent.kind,
                        "`lends` needs a `local` interface: a key names a capability of this machine",
                    );
                }
            }
        }
    }

    /// A `placement` declaration and the functions with a `lends` clause go
    /// together: a lent key is placed in the exporter's table, in the window
    /// Kickstart reserves for each client.
    fn placement_window(&mut self) {
        let interface = self.interface;
        let lends_a_key = interface
            .methods
            .iter()
            .any(|method| !method.lends.is_empty());
        match (&interface.placement, lends_a_key) {
            (Some(declaration), _) if declaration.slots == 0 => {
                self.diagnostics.push(Diagnostic::error(
                    declaration.span.clone(),
                    "a placement window needs at least one slot",
                ));
            }
            (Some(declaration), _) if declaration.slots > MAX_PLACEMENT_SLOTS => {
                self.diagnostics.push(Diagnostic::error(
                    declaration.span.clone(),
                    format!("a placement window is at most {MAX_PLACEMENT_SLOTS} slots"),
                ));
            }
            (Some(declaration), false) => {
                self.diagnostics.push(Diagnostic::error(
                    declaration.span.clone(),
                    "no function has a `lends` clause, so the placement window is never used",
                ));
            }
            (None, true) => {
                self.error(
                    &interface.name,
                    "functions with a `lends` clause need a `placement <slots>;` window to place the key in",
                );
            }
            (Some(_), true) | (None, false) => {}
        }
    }

    /// Register budgets and export names.
    fn members(&mut self, checked: &Checked<'interface>) {
        let interface = self.interface;
        for exception in &interface.exceptions {
            let words = checked.words_of_fields(&exception.fields);
            if words > ANSWER_WORDS {
                self.error(
                    &exception.name,
                    format!(
                        "exception `{}` carries {words} words; at most {ANSWER_WORDS} fits beside the outcome",
                        exception.name.text
                    ),
                );
            }
        }
        for method in &interface.methods {
            let words = checked.words_of_fields(&method.parameters);
            if words > ARGUMENT_WORDS {
                self.error(
                    &method.name,
                    format!(
                        "`{}` takes {words} argument words; a call passes at most {ARGUMENT_WORDS} \
                         (pass larger values by reference)",
                        method.name.text
                    ),
                );
            }
            let words = checked.words_of_fields(&method.results);
            if method.results.len() > 1 || words > ANSWER_WORDS {
                self.error(
                    &method.name,
                    format!(
                        "`{}` returns {words} words; a call returns at most one result of \
                         {ANSWER_WORDS} word (return larger values through a reference parameter)",
                        method.name.text
                    ),
                );
            }
            let export = export_name(&interface.name.text, &method.name.text);
            if export.len() > EXPORT_NAME_BYTES {
                self.error(
                    &method.name,
                    format!(
                        "export name `{export}` is {} bytes; at most {EXPORT_NAME_BYTES} are recorded",
                        export.len()
                    ),
                );
            }
        }
    }
}

/// The text the type code hashes: names, types, values and signatures in
/// declaration order, without documentation.
fn fingerprint_text(interface: &Interface) -> String {
    let mut text = String::new();
    let Ok(()) = write_fingerprint(&mut text, interface) else {
        unreachable!("writing to a String cannot fail");
    };
    text
}

fn write_fingerprint(text: &mut String, interface: &Interface) -> fmt::Result {
    let local = if interface.local { "local " } else { "" };
    write!(text, "{local}interface {}{{", interface.name.text)?;
    if let Some(declaration) = &interface.data {
        write!(text, "data {};", declaration.bytes)?;
    }
    if let Some(declaration) = &interface.placement {
        write!(text, "placement {};", declaration.slots)?;
    }
    for definition in &interface.types {
        match &definition.kind {
            TypeKind::Alias(target) => {
                write!(text, "type ")?;
                write_type(text, target)?;
                write!(text, " {};", definition.name.text)?;
            }
            TypeKind::Enum(values) => {
                write!(text, "enum {}{{", definition.name.text)?;
                for value in values {
                    write!(text, "{},", value.text)?;
                }
                write!(text, "}}")?;
            }
            TypeKind::Record(fields) => {
                write!(text, "record {}{{", definition.name.text)?;
                write_fields(text, fields)?;
                write!(text, "}}")?;
            }
        }
    }
    for exception in &interface.exceptions {
        write!(text, "exception {}{{", exception.name.text)?;
        write_fields(text, &exception.fields)?;
        write!(text, "}}")?;
    }
    for method in &interface.methods {
        write!(text, "{}(", method.name.text)?;
        write_fields(text, &method.parameters)?;
        write!(text, ")")?;
        if !method.lends.is_empty() {
            write!(text, "lends(")?;
            for lent in &method.lends {
                write!(text, "{} {};", lent.kind.text, lent.name.text)?;
            }
            write!(text, ")")?;
        }
        write!(text, "returns(")?;
        write_fields(text, &method.results)?;
        write!(text, ")raises(")?;
        for raised in &method.raises {
            write!(text, "{},", raised.text)?;
        }
        write!(text, ");")?;
    }
    write!(text, "}}")
}

fn write_type(text: &mut String, ty: &TypeReference) -> fmt::Result {
    let reference = if ty.reference { "&" } else { "" };
    write!(text, "{}{reference}", ty.name.text)
}

fn write_fields(text: &mut String, fields: &[Field]) -> fmt::Result {
    for field in fields {
        write_type(text, &field.ty)?;
        write!(text, " {};", field.name.text)?;
    }
    Ok(())
}
