//! Parse Meddle tokens into a syntax tree.

use {
    crate::{
        ast::{
            DataDeclaration, Exception, Field, File, Interface, Method, Name, TypeDefinition,
            TypeKind, TypeReference, Uses,
        },
        diagnostic::Diagnostic,
        lexer::Token,
    },
    chumsky::{
        error::RichPattern,
        input::{Stream, ValueInput},
        prelude::*,
    },
    logos::Logos,
};

type Extra<'tokens, 'source> = extra::Err<Rich<'tokens, Token<'source>>>;

/// One member of an interface body.
enum Member {
    Type(TypeDefinition),
    Exception(Exception),
    Method(Method),
    Data(DataDeclaration),
    /// A member that did not parse; its error is already reported.
    Invalid,
}

/// A member without its documentation.
enum MemberBody {
    Type(Name, TypeKind),
    Exception(Name, Vec<Field>),
    Data(DataDeclaration),
    Method {
        name: Name,
        parameters: Vec<Field>,
        results: Vec<Field>,
        raises: Vec<Name>,
    },
}

/// One top-level item of a file.
enum Item {
    Interface(Interface),
    Uses(Uses),
}

fn file_parser<'tokens, 'source: 'tokens, I>()
-> impl Parser<'tokens, I, File, Extra<'tokens, 'source>>
where
    I: ValueInput<'tokens, Token = Token<'source>, Span = SimpleSpan>,
{
    let name = select! { Token::Identifier(text) => text }
        .map_with(|text: &str, extra| {
            let span: SimpleSpan = extra.span();
            Name {
                text: text.to_owned(),
                span: span.into_range(),
            }
        })
        .labelled("a name");

    let docs = select! { Token::Doc(text) => text.to_owned() }
        .repeated()
        .collect::<Vec<_>>();

    let type_reference = name
        .then(just(Token::Ampersand).or_not())
        .map(|(name, ampersand)| TypeReference {
            name,
            reference: ampersand.is_some(),
        })
        .labelled("a type");

    let field = docs
        .then(type_reference.clone())
        .then(name)
        .map(|((doc, ty), name)| Field { doc, ty, name });

    let field_block = field
        .clone()
        .then_ignore(just(Token::Semicolon))
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LeftBrace), just(Token::RightBrace));

    let name_list = name
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>();

    let parameter_list = field
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LeftParen), just(Token::RightParen));

    let alias = just(Token::Type)
        .ignore_then(type_reference)
        .then(name)
        .then_ignore(just(Token::Semicolon))
        .map(|(target, name)| (name, TypeKind::Alias(target)));

    let enumeration = just(Token::Enum)
        .ignore_then(name)
        .then(
            name_list
                .clone()
                .delimited_by(just(Token::LeftBrace), just(Token::RightBrace)),
        )
        .map(|(name, values)| (name, TypeKind::Enum(values)));

    let record = just(Token::Record)
        .ignore_then(name)
        .then(field_block.clone())
        .map(|(name, fields)| (name, TypeKind::Record(fields)));

    let type_definition = choice((alias, enumeration, record));

    let exception = just(Token::Exception).ignore_then(name).then(field_block);

    let method = name
        .then(parameter_list.clone())
        .then(just(Token::Returns).ignore_then(parameter_list).or_not())
        .then(
            just(Token::Raises)
                .ignore_then(
                    name_list
                        .clone()
                        .delimited_by(just(Token::LeftParen), just(Token::RightParen)),
                )
                .or_not(),
        )
        .then_ignore(just(Token::Semicolon));

    let data = just(Token::Data)
        .ignore_then(
            select! { Token::Number(bytes) => bytes }.map_with(|bytes: u64, extra| {
                let span: SimpleSpan = extra.span();
                DataDeclaration {
                    bytes,
                    span: span.into_range(),
                }
            }),
        )
        .then_ignore(just(Token::Semicolon));

    let member = docs
        .then(choice((
            type_definition.map(|(name, kind)| MemberBody::Type(name, kind)),
            exception.map(|(name, fields)| MemberBody::Exception(name, fields)),
            data.map(MemberBody::Data),
            method.map(
                |(((name, parameters), results), raises)| MemberBody::Method {
                    name,
                    parameters,
                    results: results.unwrap_or_default(),
                    raises: raises.unwrap_or_default(),
                },
            ),
        )))
        .map(|(doc, body)| match body {
            MemberBody::Type(name, kind) => Member::Type(TypeDefinition { doc, name, kind }),
            MemberBody::Data(declaration) => Member::Data(declaration),
            MemberBody::Exception(name, fields) => {
                Member::Exception(Exception { doc, name, fields })
            }
            MemberBody::Method {
                name,
                parameters,
                results,
                raises,
            } => Member::Method(Method {
                doc,
                name,
                parameters,
                results,
                raises,
            }),
        })
        // A member that does not parse is skipped up to its `;`, or up to the
        // `}` closing the interface, so the rest still gets checked.
        .recover_with(skip_until(
            any().ignored(),
            just(Token::Semicolon)
                .ignored()
                .or(just(Token::RightBrace).rewind().ignored()),
            || Member::Invalid,
        ));

    let interface = docs
        .then(just(Token::Local).or_not())
        .then_ignore(just(Token::Interface))
        .then(name)
        .then(
            just(Token::RightBrace)
                .not()
                .ignore_then(member)
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LeftBrace), just(Token::RightBrace)),
        )
        .map(|(((doc, local), name), members)| {
            let mut interface = Interface {
                doc,
                local: local.is_some(),
                name,
                types: Vec::new(),
                exceptions: Vec::new(),
                data: None,
                methods: Vec::new(),
            };
            for member in members {
                match member {
                    Member::Type(definition) => interface.types.push(definition),
                    Member::Exception(exception) => interface.exceptions.push(exception),
                    Member::Method(method) => interface.methods.push(method),
                    Member::Data(declaration) => interface.data = Some(declaration),
                    Member::Invalid => {}
                }
            }
            interface
        });

    let link = name
        .or(
            select! { Token::Text(text) => text }.map_with(|text: &str, extra| {
                let span: SimpleSpan = extra.span();
                Name {
                    text: text.to_owned(),
                    span: span.into_range(),
                }
            }),
        )
        .labelled("a device tree property");

    let uses = just(Token::Uses)
        .ignore_then(name)
        .then_ignore(just(Token::Via))
        .then(
            link.separated_by(just(Token::Comma))
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .then(name_list.delimited_by(just(Token::LeftBrace), just(Token::RightBrace)))
        .map(|((interface, links), functions)| Uses {
            interface,
            links,
            functions,
        });

    choice((interface.map(Item::Interface), uses.map(Item::Uses)))
        .repeated()
        .collect::<Vec<_>>()
        .then_ignore(end())
        .map(|items| {
            let mut file = File::default();
            for item in items {
                match item {
                    Item::Interface(interface) => file.interfaces.push(interface),
                    Item::Uses(uses) => file.uses.push(uses),
                }
            }
            file
        })
}

/// Parse `source`. On errors returns what parsed (if anything) together with
/// the diagnostics.
pub fn parse(source: &str) -> (Option<File>, Vec<Diagnostic>) {
    let tokens = Token::lexer(source)
        .spanned()
        .map(|(token, span)| (token.unwrap_or(Token::Error), SimpleSpan::from(span)));
    let stream =
        Stream::from_iter(tokens).map(SimpleSpan::from(source.len()..source.len()), |pair| pair);
    let (file, errors) = file_parser().parse(stream).into_output_errors();
    let diagnostics = errors
        .into_iter()
        .map(|error| Diagnostic::error(error.span().into_range(), describe(&error)))
        .collect();
    (file, diagnostics)
}

/// "expected X, found Y" for a parse error.
fn describe(error: &Rich<'_, Token<'_>>) -> String {
    let found = error
        .found()
        .map_or_else(|| "end of file".to_owned(), ToString::to_string);
    let mut expected: Vec<String> = error.expected().map(describe_pattern).collect();
    expected.sort();
    expected.dedup();
    match expected.as_slice() {
        [] => format!("unexpected {found}"),
        [only] => format!("expected {only}, found {found}"),
        [init @ .., last] => format!("expected {} or {last}, found {found}", init.join(", ")),
    }
}

/// One thing the parser expected, as the diagnostic names it.
fn describe_pattern(pattern: &RichPattern<'_, Token<'_>>) -> String {
    match pattern {
        RichPattern::Token(token) => token.to_string(),
        RichPattern::Label(label) => label.to_string(),
        RichPattern::Identifier(identifier) => format!("`{identifier}`"),
        RichPattern::Any => "something".to_owned(),
        RichPattern::EndOfInput => "end of file".to_owned(),
        _ => "something else".to_owned(),
    }
}
