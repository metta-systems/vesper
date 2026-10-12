//! The syntax tree of a Meddle source file.

use core::ops::Range;

/// Byte range of a construct in its source file.
pub type Span = Range<usize>;

/// A name and where it was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

/// Everything one source file declares, in source order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    pub interfaces: Vec<Interface>,
    pub uses: Vec<Uses>,
}

/// `uses interface via property { function, ... }`: the functions of
/// `interface` a component calls, through the instance its device tree node
/// links to with `property`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uses {
    pub interface: Name,
    /// The client node's property holding the exporting node's phandle
    pub link: Name,
    pub functions: Vec<Name>,
}

/// `[local] interface name_vN { ... }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub doc: Vec<String>,
    pub local: bool,
    pub name: Name,
    pub types: Vec<TypeDefinition>,
    pub exceptions: Vec<Exception>,
    pub methods: Vec<Method>,
}

/// A named type an interface defines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDefinition {
    pub doc: Vec<String>,
    pub name: Name,
    pub kind: TypeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeKind {
    /// `type existing new_name;`
    Alias(TypeReference),
    /// `enum name { first, second }`
    Enum(Vec<Name>),
    /// `record name { type field; ... }`
    Record(Vec<Field>),
}

/// `exception name { type field; ... }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exception {
    pub doc: Vec<String>,
    pub name: Name,
    pub fields: Vec<Field>,
}

/// A typed name: a record or exception field, or a method parameter or result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub doc: Vec<String>,
    pub ty: TypeReference,
    pub name: Name,
}

/// The use of a type: a built-in or defined type name, possibly followed by
/// `&` (a reference to a value in an xbuf data area).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeReference {
    pub name: Name,
    pub reference: bool,
}

/// `name(parameters) [returns (results)] [raises (exceptions)];`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    pub doc: Vec<String>,
    pub name: Name,
    pub parameters: Vec<Field>,
    pub results: Vec<Field>,
    pub raises: Vec<Name>,
}
