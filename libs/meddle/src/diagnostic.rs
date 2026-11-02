//! Errors found in a Meddle source file, rendered with their source lines.

use {
    crate::ast::Span,
    ariadne::{Config, IndexType, Label, Report, ReportKind, Source},
};

/// One error: a message, the span it is about and optional secondary notes
/// pointing at related spans ("first defined here").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub related: Vec<(Span, String)>,
}

impl Diagnostic {
    /// An error about `span`.
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            related: Vec::new(),
        }
    }

    /// This error with a note about `span`.
    #[must_use]
    pub fn with_related(mut self, span: Span, note: impl Into<String>) -> Self {
        self.related.push((span, note.into()));
        self
    }

    /// Render the error against `source`, read from `path`, with or without
    /// terminal colors.
    pub fn render(&self, path: &str, source: &str, color: bool) -> String {
        let config = Config::new()
            .with_color(color)
            .with_index_type(IndexType::Byte);
        let report = Report::build(ReportKind::Error, (path, self.span.clone()))
            .with_config(config)
            .with_message(&self.message)
            .with_label(Label::new((path, self.span.clone())).with_message(&self.message))
            .with_labels(
                self.related
                    .iter()
                    .map(|(span, note)| Label::new((path, span.clone())).with_message(note)),
            )
            .finish();
        let mut rendered = Vec::new();
        // Writing to a Vec cannot fail; a failure would only lose the excerpt.
        if report
            .write((path, Source::from(source)), &mut rendered)
            .is_err()
        {
            return format!("{path}: {}", self.message);
        }
        String::from_utf8_lossy(&rendered).into_owned()
    }
}
