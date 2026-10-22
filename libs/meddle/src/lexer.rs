//! Tokens of a Meddle source file.

use {core::fmt, logos::Logos};

/// One token. `#` starts a comment running to the end of the line; `##`
/// starts a documentation comment, kept for the item that follows it.
#[derive(Logos, Clone, Debug, PartialEq, Eq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"#([^#\n][^\n]*)?", allow_greedy = true))]
pub enum Token<'source> {
    /// A character no token starts with
    Error,

    #[regex(r"##[^\n]*", documentation, allow_greedy = true)]
    Doc(&'source str),

    #[token("local")]
    Local,
    #[token("interface")]
    Interface,
    #[token("exception")]
    Exception,
    #[token("type")]
    Type,
    #[token("enum")]
    Enum,
    #[token("record")]
    Record,
    #[token("returns")]
    Returns,
    #[token("raises")]
    Raises,
    #[token("compatible")]
    Compatible,
    #[token("data")]
    Data,
    #[token("lends")]
    Lends,
    #[token("installs")]
    Installs,
    #[token("badged")]
    Badged,
    #[token("channel")]
    Channel,
    #[token("placement")]
    Placement,
    #[token("via")]
    Via,
    #[token("uses")]
    Uses,

    #[regex(r#""[^"\n]*""#, |lexer| lexer.slice().trim_matches('"'))]
    Text(&'source str),

    #[regex(r"[0-9]+", |lexer| lexer.slice().parse::<u64>().ok())]
    Number(u64),

    #[regex(r"[A-Za-z][A-Za-z0-9_]*")]
    Identifier(&'source str),

    #[token("{")]
    LeftBrace,
    #[token("}")]
    RightBrace,
    #[token("(")]
    LeftParen,
    #[token(")")]
    RightParen,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token("&")]
    Ampersand,
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Error => "an invalid character",
            Self::Doc(_) => "a documentation comment",
            Self::Local => "`local`",
            Self::Interface => "`interface`",
            Self::Exception => "`exception`",
            Self::Type => "`type`",
            Self::Enum => "`enum`",
            Self::Record => "`record`",
            Self::Returns => "`returns`",
            Self::Raises => "`raises`",
            Self::Uses => "`uses`",
            Self::Via => "`via`",
            Self::Data => "`data`",
            Self::Lends => "`lends`",
            Self::Installs => "`installs`",
            Self::Badged => "`badged`",
            Self::Channel => "`channel`",
            Self::Placement => "`placement`",
            Self::Compatible => "`compatible`",
            Self::Number(number) => return write!(formatter, "{number}"),
            Self::Identifier(name) => return write!(formatter, "`{name}`"),
            Self::Text(text) => return write!(formatter, "\"{text}\""),
            Self::LeftBrace => "`{`",
            Self::RightBrace => "`}`",
            Self::LeftParen => "`(`",
            Self::RightParen => "`)`",
            Self::Comma => "`,`",
            Self::Semicolon => "`;`",
            Self::Ampersand => "`&`",
        };
        formatter.write_str(text)
    }
}

/// The text of a `##` comment, without the marker and one following space.
fn documentation<'source>(lexer: &logos::Lexer<'source, Token<'source>>) -> &'source str {
    let text = lexer.slice().strip_prefix("##").unwrap_or_default();
    text.strip_prefix(' ').unwrap_or(text)
}
