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

/// `uses interface via device link, ... { function, ... }` or
/// `uses interface via component name { function, ... }`: the functions of
/// `interface` a component calls, through the instance its `provider` names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uses {
    pub interface: Name,
    pub provider: Provider,
    pub functions: Vec<Name>,
}

/// How a `uses` declaration finds the instance it calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provider {
    /// `via device link, ...`: the device tree chain from the client node to
    /// the exporting node (each further property read from the node the
    /// previous one reached)
    Device(Vec<Link>),
    /// `via component name`: the loaded manifest component of that name
    Component(Name),
}

/// `[local] interface name_vN { ... }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub doc: Vec<String>,
    pub local: bool,
    pub name: Name,
    pub types: Vec<TypeDefinition>,
    pub exceptions: Vec<Exception>,
    /// The shared data area its functions pass references into
    pub data: Option<DataDeclaration>,
    /// The key-placement window each client's lending functions share
    pub placement: Option<PlacementDeclaration>,
    /// Whether every function receives the badge of the key it was called
    /// through (`badged;`)
    pub badged: bool,
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
    /// The key the function lends to its exporter for the duration of the call
    pub lends: Vec<LentKey>,
    /// The interface whose Invocation keys the function installs in the lent
    /// table
    pub installs: Option<Name>,
    pub results: Vec<Field>,
    pub raises: Vec<Name>,
}

/// `lends (kind name)`: a key the caller lends to the exporter through
/// `Invocation.CallWithKey`. It travels outside the argument words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LentKey {
    /// The kind of key, one of the closed set the checker knows
    pub kind: Name,
    pub name: Name,
}

/// `placement <slots>;`: how many slots of the exporter's table each client
/// of the interface gets for the keys its functions lend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementDeclaration {
    pub slots: u64,
    pub span: Span,
}

/// `data <bytes>;`: the size of the data area each client of the interface
/// shares with its exporter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataDeclaration {
    pub bytes: u64,
    pub span: Span,
}

/// One step of the chain leading from a client to the instance it calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// The node the client's (or the previous step's) property names by
    /// phandle
    Property(Name),
    /// The one enabled node with this `compatible` string; only a chain's
    /// first step, for providers no property of the client leads to
    Compatible(Name),
}

impl Link {
    /// The property name or `compatible` string the step is written with.
    pub const fn name(&self) -> &Name {
        match self {
            Self::Property(name) | Self::Compatible(name) => name,
        }
    }
}
