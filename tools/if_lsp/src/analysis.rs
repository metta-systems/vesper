//! Language features computed from one Meddle source text, independent of
//! the LSP transport so they can be tested on plain strings.

use libmeddle::{
    ast::{
        Exception, Field, File, Interface, Method, Name, Provider, Span, TypeDefinition, TypeKind,
    },
    check::{self, Builtin, LendKind},
    diagnostic::Diagnostic,
    lexer::Token,
    parse,
};
use logos::Logos;

/// A name written somewhere in the file, with where it leads and what to say
/// about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub span: Span,
    /// Where the named thing is declared; `None` for built-ins and unresolved names
    pub definition: Option<Span>,
    /// Markdown shown on hover
    pub hover: String,
    pub class: NameClass,
}

/// What a name denotes, for highlighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameClass {
    Interface,
    Type,
    BuiltinType,
    Enum,
    Exception,
    Method,
    Property,
    Component,
}

/// The outline entry of a declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: SymbolKind,
    /// The declaration's name
    pub selection: Span,
    pub children: Vec<Self>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    Interface,
    Type,
    Enum,
    Record,
    Exception,
    Method,
    Field,
    EnumMember,
}

/// A token to color.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Highlight {
    pub span: Span,
    pub class: HighlightClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightClass {
    Keyword,
    Number,
    String,
    Documentation,
    Parameter,
    Name(NameClass),
}

/// What completion offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub detail: String,
    pub kind: CandidateKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    Keyword,
    BuiltinType,
    Type,
    Exception,
    LendKind,
}

/// The result of parsing and checking a source text.
#[derive(Debug)]
pub struct Analysis {
    pub file: Option<File>,
    pub diagnostics: Vec<Diagnostic>,
}

const KEYWORDS: [&str; 14] = [
    "local",
    "interface",
    "exception",
    "type",
    "enum",
    "record",
    "returns",
    "raises",
    "compatible",
    "data",
    "lends",
    "placement",
    "via",
    "uses",
];

const BUILTINS: [&str; 9] = [
    "octet", "card16", "card32", "card64", "int8", "int16", "int32", "int64", "boolean",
];

/// Parse and check `source`. Checks run per interface, and the `uses`
/// declarations are checked against the interfaces of the same file.
pub fn analyze(source: &str) -> Analysis {
    let (file, mut diagnostics) = parse(source);
    // Checking a tree that had parse errors would only repeat them as noise.
    if diagnostics.is_empty()
        && let Some(file) = &file
    {
        for interface in &file.interfaces {
            if let Err(found) = check::check(interface) {
                diagnostics.extend(found);
            }
        }
        if !file.uses.is_empty() {
            let interfaces: Vec<&Interface> = file.interfaces.iter().collect();
            // Interface files declare no `uses`, so every name resolves in
            // the file itself only when it is declared there; an imports
            // file refers to interfaces of other files, which are not
            // visible here, so only check when this file declares some.
            if !interfaces.is_empty() {
                diagnostics.extend(check::check_uses(&file.uses, &interfaces));
            }
        }
    }
    Analysis { file, diagnostics }
}

impl Analysis {
    /// Analyze `source`.
    pub fn of(source: &str) -> Self {
        analyze(source)
    }

    /// Every named construct of the file with its definition and hover text.
    pub fn occurrences(&self) -> Vec<Occurrence> {
        let Some(file) = &self.file else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for interface in &file.interfaces {
            interface_occurrences(interface, &mut found);
        }
        for declaration in &file.uses {
            let target = file
                .interfaces
                .iter()
                .find(|interface| interface.name.text == declaration.interface.text);
            found.push(Occurrence {
                span: declaration.interface.span.clone(),
                definition: target.map(|interface| interface.name.span.clone()),
                hover: target.map_or_else(
                    || format!("unknown interface `{}`", declaration.interface.text),
                    interface_hover,
                ),
                class: NameClass::Interface,
            });
            if let Provider::Component(name) = &declaration.provider {
                found.push(Occurrence {
                    span: name.span.clone(),
                    definition: None,
                    hover: format!("component `{}`", name.text),
                    class: NameClass::Component,
                });
            }
            for function in &declaration.functions {
                let method = target.and_then(|interface| {
                    interface
                        .methods
                        .iter()
                        .find(|method| method.name.text == function.text)
                });
                found.push(Occurrence {
                    span: function.span.clone(),
                    definition: method.map(|method| method.name.span.clone()),
                    hover: method.map_or_else(
                        || format!("unknown function `{}`", function.text),
                        method_hover,
                    ),
                    class: NameClass::Method,
                });
            }
        }
        found
    }

    /// The innermost occurrence containing byte `offset`.
    pub fn occurrence_at(&self, offset: usize) -> Option<Occurrence> {
        self.occurrences()
            .into_iter()
            .filter(|occurrence| occurrence.span.start <= offset && offset <= occurrence.span.end)
            .min_by_key(|occurrence| occurrence.span.len())
    }

    /// Every occurrence that resolves to the same declaration as the name at
    /// `offset`, the declaration itself included.
    pub fn references(&self, offset: usize) -> Vec<Span> {
        let occurrences = self.occurrences();
        let Some(here) = self.occurrence_at(offset) else {
            return Vec::new();
        };
        let Some(target) = here.definition else {
            return vec![here.span];
        };
        occurrences
            .into_iter()
            .filter(|occurrence| occurrence.definition.as_ref() == Some(&target))
            .map(|occurrence| occurrence.span)
            .collect()
    }

    /// Highlighted tokens of `source`, in source order. Keywords, numbers,
    /// strings and `##` comments come from the lexer; names are classified by
    /// what they denote, with parameter and key names as plain parameters.
    pub fn highlights(&self, source: &str) -> Vec<Highlight> {
        let occurrences = self.occurrences();
        let mut highlights = Vec::new();
        for (token, span) in Token::lexer(source).spanned() {
            let Ok(token) = token else { continue };
            let class = match token {
                Token::Doc(_) => HighlightClass::Documentation,
                Token::Number(_) => HighlightClass::Number,
                Token::Text(_) => HighlightClass::String,
                Token::Identifier(_) => match occurrences.iter().find(|o| o.span == span) {
                    Some(occurrence) => HighlightClass::Name(occurrence.class),
                    None => HighlightClass::Parameter,
                },
                Token::Local
                | Token::Interface
                | Token::Exception
                | Token::Type
                | Token::Enum
                | Token::Record
                | Token::Returns
                | Token::Raises
                | Token::Compatible
                | Token::Data
                | Token::Lends
                | Token::Placement
                | Token::Via
                | Token::Uses => HighlightClass::Keyword,
                _ => continue,
            };
            highlights.push(Highlight { span, class });
        }
        highlights
    }

    /// The outline of the file.
    pub fn symbols(&self) -> Vec<Symbol> {
        let Some(file) = &self.file else {
            return Vec::new();
        };
        file.interfaces.iter().map(interface_symbol).collect()
    }

    /// What may be written at byte `offset` of `source`.
    pub fn completions(&self, source: &str, offset: usize) -> Vec<Candidate> {
        let before = &source[..offset.min(source.len())];
        let line = before.rsplit('\n').next().unwrap_or_default();
        let words: Vec<&str> = line
            .trim_start()
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .collect();
        // `words` ends with the (possibly empty) word being typed.
        let previous = words.len().checked_sub(2).map(|index| words[index]);

        let mut candidates = Vec::new();
        if previous == Some("lends") || line.trim_end().ends_with("lends (") {
            candidates.extend(LendKind::NAMES.map(|name| Candidate {
                label: name.to_owned(),
                detail: "kind of lent key".to_owned(),
                kind: CandidateKind::LendKind,
            }));
            return candidates;
        }
        let interface = self.file.as_ref().and_then(|file| {
            file.interfaces
                .iter()
                .find(|interface| interface_range(interface).contains(&offset))
        });
        if previous == Some("raises") || line.contains("raises (") {
            if let Some(interface) = interface {
                candidates.extend(interface.exceptions.iter().map(|exception| Candidate {
                    label: exception.name.text.clone(),
                    detail: "exception".to_owned(),
                    kind: CandidateKind::Exception,
                }));
            }
            return candidates;
        }
        candidates.extend(KEYWORDS.map(|word| Candidate {
            label: word.to_owned(),
            detail: "keyword".to_owned(),
            kind: CandidateKind::Keyword,
        }));
        candidates.extend(BUILTINS.map(|name| Candidate {
            label: name.to_owned(),
            detail: format!(
                "built-in, Rust `{}`",
                Builtin::from_name(name).map_or("?", Builtin::rust_name)
            ),
            kind: CandidateKind::BuiltinType,
        }));
        if let Some(interface) = interface {
            candidates.extend(interface.types.iter().map(|definition| Candidate {
                label: definition.name.text.clone(),
                detail: type_kind_word(&definition.kind).to_owned(),
                kind: CandidateKind::Type,
            }));
        }
        candidates
    }
}

/// The byte range an interface spans: from its name to its last member.
fn interface_range(interface: &Interface) -> Span {
    let mut end = interface.name.span.end;
    let mut extend = |span: &Span| end = end.max(span.end);
    interface.types.iter().for_each(|d| {
        extend(&d.name.span);
        match &d.kind {
            TypeKind::Alias(target) => extend(&target.name.span),
            TypeKind::Enum(values) => values.iter().for_each(|v| extend(&v.span)),
            TypeKind::Record(fields) => fields.iter().for_each(|f| extend(&f.name.span)),
        }
    });
    interface.exceptions.iter().for_each(|e| {
        extend(&e.name.span);
        e.fields.iter().for_each(|f| extend(&f.name.span));
    });
    interface.methods.iter().for_each(|m| {
        extend(&m.name.span);
        m.parameters
            .iter()
            .chain(&m.results)
            .for_each(|f| extend(&f.name.span));
        m.raises.iter().for_each(|r| extend(&r.span));
    });
    interface.data.iter().for_each(|d| extend(&d.span));
    interface.placement.iter().for_each(|p| extend(&p.span));
    // The closing brace and anything typed after the last member still
    // belong to the interface until the next one starts, so be generous.
    interface.name.span.start..end.saturating_add(1024)
}

fn type_kind_word(kind: &TypeKind) -> &'static str {
    match kind {
        TypeKind::Alias(_) => "type alias",
        TypeKind::Enum(_) => "enum",
        TypeKind::Record(_) => "record",
    }
}

fn with_docs(doc: &[String], signature: String) -> String {
    let mut text = format!("```meddle\n{signature}\n```");
    if !doc.is_empty() {
        text.push_str("\n\n");
        text.push_str(&doc.join("\n"));
    }
    text
}

fn field_list(fields: &[Field]) -> String {
    fields
        .iter()
        .map(|field| {
            format!(
                "{}{} {}",
                field.ty.name.text,
                if field.ty.reference { "&" } else { "" },
                field.name.text
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn interface_hover(interface: &Interface) -> String {
    let local = if interface.local { "local " } else { "" };
    with_docs(
        &interface.doc,
        format!("{local}interface {}", interface.name.text),
    )
}

fn method_hover(method: &Method) -> String {
    let mut signature = format!("{}({})", method.name.text, field_list(&method.parameters));
    if !method.lends.is_empty() {
        let keys: Vec<String> = method
            .lends
            .iter()
            .map(|key| format!("{} {}", key.kind.text, key.name.text))
            .collect();
        signature.push_str(&format!(" lends ({})", keys.join(", ")));
    }
    if !method.results.is_empty() {
        signature.push_str(&format!(" returns ({})", field_list(&method.results)));
    }
    if !method.raises.is_empty() {
        let raised: Vec<&str> = method.raises.iter().map(|n| n.text.as_str()).collect();
        signature.push_str(&format!(" raises ({})", raised.join(", ")));
    }
    with_docs(&method.doc, signature)
}

fn type_hover(definition: &TypeDefinition) -> String {
    let name = &definition.name.text;
    let signature = match &definition.kind {
        TypeKind::Alias(target) => format!("type {} {name}", target.name.text),
        TypeKind::Enum(values) => {
            let values: Vec<&str> = values.iter().map(|v| v.text.as_str()).collect();
            format!("enum {name} {{ {} }}", values.join(", "))
        }
        TypeKind::Record(fields) => format!("record {name} {{ {} }}", {
            fields
                .iter()
                .map(|f| format!("{} {};", f.ty.name.text, f.name.text))
                .collect::<Vec<_>>()
                .join(" ")
        }),
    };
    with_docs(&definition.doc, signature)
}

fn exception_hover(exception: &Exception) -> String {
    with_docs(
        &exception.doc,
        format!(
            "exception {} {{ {} }}",
            exception.name.text,
            exception
                .fields
                .iter()
                .map(|f| format!("{} {};", f.ty.name.text, f.name.text))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    )
}

fn interface_occurrences(interface: &Interface, found: &mut Vec<Occurrence>) {
    let declaration = |name: &Name, hover: String, class: NameClass| Occurrence {
        span: name.span.clone(),
        definition: Some(name.span.clone()),
        hover,
        class,
    };
    let type_use = |reference: &Name| -> Occurrence {
        if let Some(builtin) = Builtin::from_name(&reference.text) {
            return Occurrence {
                span: reference.span.clone(),
                definition: None,
                hover: format!("built-in type, Rust `{}`", builtin.rust_name()),
                class: NameClass::BuiltinType,
            };
        }
        match interface
            .types
            .iter()
            .find(|definition| definition.name.text == reference.text)
        {
            Some(definition) => Occurrence {
                span: reference.span.clone(),
                definition: Some(definition.name.span.clone()),
                hover: type_hover(definition),
                class: NameClass::Type,
            },
            None => Occurrence {
                span: reference.span.clone(),
                definition: None,
                hover: format!("unknown type `{}`", reference.text),
                class: NameClass::Type,
            },
        }
    };

    found.push(declaration(
        &interface.name,
        interface_hover(interface),
        NameClass::Interface,
    ));
    for definition in &interface.types {
        found.push(declaration(
            &definition.name,
            type_hover(definition),
            if matches!(definition.kind, TypeKind::Enum(_)) {
                NameClass::Enum
            } else {
                NameClass::Type
            },
        ));
        match &definition.kind {
            TypeKind::Alias(target) => found.push(type_use(&target.name)),
            TypeKind::Enum(values) => {
                for value in values {
                    found.push(declaration(
                        value,
                        format!(
                            "```meddle\n{} // of {}\n```",
                            value.text, definition.name.text
                        ),
                        NameClass::Property,
                    ));
                }
            }
            TypeKind::Record(fields) => {
                for field in fields {
                    found.push(type_use(&field.ty.name));
                    found.push(declaration(
                        &field.name,
                        with_docs(&field.doc, field_list(std::slice::from_ref(field))),
                        NameClass::Property,
                    ));
                }
            }
        }
    }
    for exception in &interface.exceptions {
        found.push(declaration(
            &exception.name,
            exception_hover(exception),
            NameClass::Exception,
        ));
        for field in &exception.fields {
            found.push(type_use(&field.ty.name));
        }
    }
    for method in &interface.methods {
        found.push(declaration(
            &method.name,
            method_hover(method),
            NameClass::Method,
        ));
        for field in method.parameters.iter().chain(&method.results) {
            found.push(type_use(&field.ty.name));
        }
        for raised in &method.raises {
            let exception = interface
                .exceptions
                .iter()
                .find(|exception| exception.name.text == raised.text);
            found.push(Occurrence {
                span: raised.span.clone(),
                definition: exception.map(|exception| exception.name.span.clone()),
                hover: exception.map_or_else(
                    || format!("unknown exception `{}`", raised.text),
                    exception_hover,
                ),
                class: NameClass::Exception,
            });
        }
    }
}

fn interface_symbol(interface: &Interface) -> Symbol {
    let mut children = Vec::new();
    for definition in &interface.types {
        let (kind, members) = match &definition.kind {
            TypeKind::Alias(_) => (SymbolKind::Type, Vec::new()),
            TypeKind::Enum(values) => (
                SymbolKind::Enum,
                values
                    .iter()
                    .map(|value| leaf(&value.text, None, SymbolKind::EnumMember, &value.span))
                    .collect(),
            ),
            TypeKind::Record(fields) => (SymbolKind::Record, field_symbols(fields)),
        };
        children.push(Symbol {
            children: members,
            ..leaf(
                &definition.name.text,
                Some(type_kind_word(&definition.kind)),
                kind,
                &definition.name.span,
            )
        });
    }
    for exception in &interface.exceptions {
        children.push(Symbol {
            children: field_symbols(&exception.fields),
            ..leaf(
                &exception.name.text,
                Some("exception"),
                SymbolKind::Exception,
                &exception.name.span,
            )
        });
    }
    for method in &interface.methods {
        children.push(leaf(
            &method.name.text,
            None,
            SymbolKind::Method,
            &method.name.span,
        ));
    }
    Symbol {
        children,
        ..leaf(
            &interface.name.text,
            Some(if interface.local {
                "local interface"
            } else {
                "interface"
            }),
            SymbolKind::Interface,
            &interface.name.span,
        )
    }
}

fn field_symbols(fields: &[Field]) -> Vec<Symbol> {
    fields
        .iter()
        .map(|field| {
            leaf(
                &field.name.text,
                Some(&field.ty.name.text),
                SymbolKind::Field,
                &field.name.span,
            )
        })
        .collect()
}

fn leaf(name: &str, detail: Option<&str>, kind: SymbolKind, selection: &Span) -> Symbol {
    Symbol {
        name: name.to_owned(),
        detail: detail.map(ToOwned::to_owned),
        kind,
        selection: selection.clone(),
        children: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "\
## A mailbox.
local interface mailbox_v1 {
    enum device { first, second }
    record message { card32 length; device from; }
    exception full { card32 capacity; }
    ## Send a message.
    send(message item) returns (boolean done) raises (full);
}
";

    fn offset_of(needle: &str, nth: usize) -> usize {
        SOURCE.match_indices(needle).nth(nth).unwrap().0
    }

    #[test]
    fn valid_file_has_no_diagnostics() {
        assert!(
            analyze(SOURCE).diagnostics.is_empty(),
            "{:?}",
            analyze(SOURCE).diagnostics
        );
    }

    #[test]
    fn parse_error_is_reported_without_check_noise() {
        let analysis = analyze("interface foo_v1 { send( ; }");
        assert_eq!(analysis.diagnostics.len(), 1);
    }

    #[test]
    fn check_error_is_reported() {
        let analysis = analyze("interface foo_v1 { send(missing item); }");
        assert!(!analysis.diagnostics.is_empty());
    }

    #[test]
    fn type_use_goes_to_definition() {
        let analysis = analyze(SOURCE);
        let use_offset = offset_of("message item", 0) + 2;
        let occurrence = analysis.occurrence_at(use_offset).unwrap();
        let definition = occurrence.definition.unwrap();
        assert_eq!(&SOURCE[definition], "message");
        assert!(occurrence.hover.contains("record message"));
    }

    #[test]
    fn raises_goes_to_exception() {
        let analysis = analyze(SOURCE);
        let occurrence = analysis.occurrence_at(offset_of("full", 1)).unwrap();
        assert_eq!(&SOURCE[occurrence.definition.unwrap()], "full");
    }

    #[test]
    fn builtin_hover_names_rust_type() {
        let analysis = analyze(SOURCE);
        let occurrence = analysis.occurrence_at(offset_of("boolean", 0)).unwrap();
        assert!(occurrence.hover.contains("bool"));
    }

    #[test]
    fn method_hover_has_signature_and_docs() {
        let analysis = analyze(SOURCE);
        let occurrence = analysis.occurrence_at(offset_of("send(", 0)).unwrap();
        assert!(
            occurrence
                .hover
                .contains("send(message item) returns (boolean done) raises (full)")
        );
        assert!(occurrence.hover.contains("Send a message."));
    }

    #[test]
    fn references_include_declaration_and_uses() {
        let analysis = analyze(SOURCE);
        let spans = analysis.references(offset_of("device", 0));
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn symbols_outline_interface_members() {
        let symbols = analyze(SOURCE).symbols();
        assert_eq!(symbols.len(), 1);
        let names: Vec<&str> = symbols[0]
            .children
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["device", "message", "full", "send"]);
    }

    #[test]
    fn uses_resolves_functions_in_same_file() {
        let source = format!("{SOURCE}uses mailbox_v1 via component core {{ send }}\n");
        let analysis = analyze(&source);
        let offset = source.rfind("send").unwrap();
        let occurrence = analysis.occurrence_at(offset).unwrap();
        assert_eq!(&source[occurrence.definition.unwrap()], "send");
    }

    #[test]
    fn highlights_classify_tokens() {
        let source = "## doc\ninterface a_v1 { enum e { x } f(e item, card32 n) raises (); }";
        let highlights = Analysis::of(source).highlights(source);
        let class_of = |text: &str| {
            let start = source.find(text).unwrap();
            highlights
                .iter()
                .find(|h| h.span.start == start)
                .unwrap()
                .class
        };
        assert_eq!(class_of("## doc"), HighlightClass::Documentation);
        assert_eq!(class_of("interface"), HighlightClass::Keyword);
        assert_eq!(class_of("a_v1"), HighlightClass::Name(NameClass::Interface));
        assert_eq!(class_of("f("), HighlightClass::Name(NameClass::Method));
        assert_eq!(
            class_of("card32"),
            HighlightClass::Name(NameClass::BuiltinType)
        );
        assert_eq!(class_of("item"), HighlightClass::Parameter);
        assert_eq!(class_of("x }"), HighlightClass::Name(NameClass::Property));
    }

    #[test]
    fn completion_after_lends_offers_key_kinds() {
        let analysis = analyze(SOURCE);
        let source = "interface a_v1 { f() lends (";
        let labels: Vec<String> = analysis
            .completions(source, source.len())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, ["key_table", "frame"]);
    }

    #[test]
    fn completion_offers_local_types() {
        let analysis = analyze(SOURCE);
        let offset = offset_of("send(", 0);
        let labels: Vec<String> = analysis
            .completions(SOURCE, offset)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.contains(&"message".to_owned()));
        assert!(labels.contains(&"card32".to_owned()));
    }
}
