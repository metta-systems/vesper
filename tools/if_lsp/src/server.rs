//! The LSP front end: keeps open documents and answers requests from
//! [`crate::analysis`].

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use ignore::WalkBuilder;

use tower_lsp_server::{
    Client, LanguageServer,
    jsonrpc::Result,
    ls_types::{
        CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams,
        CompletionResponse, Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity,
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
        GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
        InitializeParams, InitializeResult, Location, MarkupContent, MarkupKind, OneOf,
        ReferenceParams, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
        SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
        SemanticTokensParams, SemanticTokensResult, SemanticTokensServerCapabilities,
        ServerCapabilities, ServerInfo, SymbolKind, TextDocumentSyncCapability,
        TextDocumentSyncKind, Uri,
    },
};

use crate::{
    analysis::{self, Analysis, CandidateKind, HighlightClass, NameClass},
    text::{offset_at, position_at, range_of},
};

pub struct Backend {
    client: Client,
    documents: Mutex<HashMap<Uri, String>>,
    /// Every directory of the project holding `.if` files, searched for the
    /// interfaces an `installs` clause names
    include: Mutex<Vec<PathBuf>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
            include: Mutex::new(Vec::new()),
        }
    }

    fn store(&self, uri: &Uri, source: String) {
        self.documents
            .lock()
            .expect("document lock poisoned")
            .insert(uri.clone(), source);
    }

    fn source_of(&self, uri: &Uri) -> Option<String> {
        self.documents
            .lock()
            .expect("document lock poisoned")
            .get(uri)
            .cloned()
    }

    async fn publish(&self, uri: Uri, source: &str) {
        // The document's own directory first, then the project's other
        // directories with interface files: the checks are lenient about where
        // an installed interface lives.
        let mut include: Vec<PathBuf> = uri
            .to_file_path()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .into_iter()
            .collect();
        include.extend(
            self.include
                .lock()
                .expect("include lock poisoned")
                .iter()
                .cloned(),
        );
        let analysis = analysis::analyze(source, &include);
        let diagnostics = analysis
            .diagnostics
            .iter()
            .map(|diagnostic| to_lsp_diagnostic(&uri, source, diagnostic))
            .collect();
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

fn to_lsp_diagnostic(
    uri: &Uri,
    source: &str,
    diagnostic: &libmeddle::diagnostic::Diagnostic,
) -> Diagnostic {
    let related = diagnostic
        .related
        .iter()
        .map(|(span, note)| DiagnosticRelatedInformation {
            location: Location::new(uri.clone(), range_of(source, span)),
            message: note.clone(),
        })
        .collect::<Vec<_>>();
    Diagnostic {
        range: range_of(source, &diagnostic.span),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("meddle".to_owned()),
        message: diagnostic.message.clone(),
        related_information: (!related.is_empty()).then_some(related),
        ..Diagnostic::default()
    }
}

fn symbol_kind(kind: analysis::SymbolKind) -> SymbolKind {
    use analysis::SymbolKind as Kind;
    match kind {
        Kind::Interface => SymbolKind::INTERFACE,
        Kind::Type => SymbolKind::TYPE_PARAMETER,
        Kind::Enum => SymbolKind::ENUM,
        Kind::Record => SymbolKind::STRUCT,
        Kind::Exception => SymbolKind::CLASS,
        Kind::Method => SymbolKind::METHOD,
        Kind::Field => SymbolKind::FIELD,
        Kind::EnumMember => SymbolKind::ENUM_MEMBER,
    }
}

#[allow(deprecated)] // `deprecated` must be spelled out in the struct literal
fn to_document_symbol(source: &str, symbol: &analysis::Symbol) -> DocumentSymbol {
    let range = range_of(source, &symbol.selection);
    DocumentSymbol {
        name: symbol.name.clone(),
        detail: symbol.detail.clone(),
        kind: symbol_kind(symbol.kind),
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: Some(
            symbol
                .children
                .iter()
                .map(|child| to_document_symbol(source, child))
                .collect(),
        ),
    }
}

const TOKEN_TYPES: [SemanticTokenType; 11] = [
    SemanticTokenType::KEYWORD,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
    SemanticTokenType::COMMENT,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::ENUM,
    SemanticTokenType::CLASS,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::PROPERTY,
];

/// Modifier bit 0 is `documentation`, bit 1 is `defaultLibrary`.
const TOKEN_MODIFIERS: [SemanticTokenModifier; 2] = [
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

/// Index into [`TOKEN_TYPES`] and the modifier bits of a highlight class.
const fn token_style(class: HighlightClass) -> (u32, u32) {
    match class {
        HighlightClass::Keyword => (0, 0),
        HighlightClass::Number => (1, 0),
        HighlightClass::String => (2, 0),
        HighlightClass::Documentation => (3, 1),
        HighlightClass::Parameter => (4, 0),
        HighlightClass::Name(name) => match name {
            NameClass::Interface | NameClass::Component => (5, 0),
            NameClass::Type => (6, 0),
            NameClass::BuiltinType => (6, 2),
            NameClass::Enum => (7, 0),
            NameClass::Exception => (8, 0),
            NameClass::Method => (9, 0),
            NameClass::Property => (10, 0),
        },
    }
}

/// Highlights in the LSP's delta encoding.
fn encode_tokens(source: &str, highlights: &[analysis::Highlight]) -> Vec<SemanticToken> {
    let mut previous = (0, 0);
    highlights
        .iter()
        .map(|highlight| {
            let start = position_at(source, highlight.span.start);
            let length = source[highlight.span.clone()].encode_utf16().count() as u32;
            let (token_type, token_modifiers_bitset) = token_style(highlight.class);
            let delta_line = start.line - previous.0;
            let delta_start = if delta_line == 0 {
                start.character - previous.1
            } else {
                start.character
            };
            previous = (start.line, start.character);
            SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type,
                token_modifiers_bitset,
            }
        })
        .collect()
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let roots = workspace_roots(&params);
        let found = tokio::task::spawn_blocking(move || interface_directories(&roots))
            .await
            .unwrap_or_default();
        *self.include.lock().expect("include lock poisoned") = found;
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "if_lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: TOKEN_TYPES.to_vec(),
                                token_modifiers: TOKEN_MODIFIERS.to_vec(),
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..SemanticTokensOptions::default()
                        },
                    ),
                ),
                completion_provider: Some(CompletionOptions::default()),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let source = params.text_document.text;
        self.store(&uri, source.clone());
        self.publish(uri, &source).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Full sync: the last change holds the whole text.
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let uri = params.text_document.uri;
        self.store(&uri, change.text.clone());
        self.publish(uri, &change.text).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents
            .lock()
            .expect("document lock poisoned")
            .remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let position = params.text_document_position_params;
        let Some(source) = self.source_of(&position.text_document.uri) else {
            return Ok(None);
        };
        let offset = offset_at(&source, position.position);
        Ok(Analysis::of(&source)
            .occurrence_at(offset)
            .map(|occurrence| Hover {
                range: Some(range_of(&source, &occurrence.span)),
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: occurrence.hover,
                }),
            }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let position = params.text_document_position_params;
        let uri = position.text_document.uri;
        let Some(source) = self.source_of(&uri) else {
            return Ok(None);
        };
        let offset = offset_at(&source, position.position);
        Ok(Analysis::of(&source)
            .occurrence_at(offset)
            .and_then(|occurrence| occurrence.definition)
            .map(|span| {
                GotoDefinitionResponse::Scalar(Location::new(uri, range_of(&source, &span)))
            }))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let position = params.text_document_position;
        let uri = position.text_document.uri;
        let Some(source) = self.source_of(&uri) else {
            return Ok(None);
        };
        let offset = offset_at(&source, position.position);
        let analysis = Analysis::of(&source);
        let declaration = analysis
            .occurrence_at(offset)
            .and_then(|occurrence| occurrence.definition);
        Ok(Some(
            analysis
                .references(offset)
                .into_iter()
                .filter(|span| {
                    params.context.include_declaration || Some(span) != declaration.as_ref()
                })
                .map(|span| Location::new(uri.clone(), range_of(&source, &span)))
                .collect(),
        ))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some(source) = self.source_of(&params.text_document.uri) else {
            return Ok(None);
        };
        let symbols = Analysis::of(&source)
            .symbols()
            .iter()
            .map(|symbol| to_document_symbol(&source, symbol))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let Some(source) = self.source_of(&params.text_document.uri) else {
            return Ok(None);
        };
        let highlights = Analysis::of(&source).highlights(&source);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: encode_tokens(&source, &highlights),
        })))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let position = params.text_document_position;
        let Some(source) = self.source_of(&position.text_document.uri) else {
            return Ok(None);
        };
        let offset = offset_at(&source, position.position);
        let items = Analysis::of(&source)
            .completions(&source, offset)
            .into_iter()
            .map(|candidate| CompletionItem {
                label: candidate.label,
                detail: Some(candidate.detail),
                kind: Some(match candidate.kind {
                    CandidateKind::Keyword => CompletionItemKind::KEYWORD,
                    CandidateKind::BuiltinType => CompletionItemKind::TYPE_PARAMETER,
                    CandidateKind::Type => CompletionItemKind::STRUCT,
                    CandidateKind::Exception => CompletionItemKind::CLASS,
                    CandidateKind::LendKind => CompletionItemKind::ENUM_MEMBER,
                }),
                ..CompletionItem::default()
            })
            .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_delta_encoded() {
        let source = "interface a_v1 {\n  f();\n}";
        let tokens = encode_tokens(source, &Analysis::of(source).highlights(source));
        let deltas: Vec<(u32, u32, u32)> = tokens
            .iter()
            .map(|t| (t.delta_line, t.delta_start, t.length))
            .collect();
        // `interface`, `a_v1`, then `f` on the next line, indented by two.
        assert_eq!(deltas, [(0, 0, 9), (0, 10, 4), (1, 2, 1)]);
    }
}

/// The project folders the client opened: its workspace folders, or else its
/// root.
#[allow(deprecated)] // `root_uri` is the only root older clients send
fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    let folders = params.workspace_folders.iter().flatten();
    folders
        .map(|folder| &folder.uri)
        .chain(params.root_uri.iter())
        .filter_map(Uri::to_file_path)
        .map(std::borrow::Cow::into_owned)
        .collect()
}

/// Every directory below `roots` that holds a `.if` file, so the checks can
/// find an interface another one installs wherever it lives. Hidden
/// directories and everything a `.gitignore` (or `.ignore`) excludes are
/// skipped, whether or not the root is inside a git repository.
fn interface_directories(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for root in roots {
        let walk = WalkBuilder::new(root).require_git(false).build();
        for entry in walk.filter_map(std::result::Result::ok) {
            let is_interface = entry.file_type().is_some_and(|kind| kind.is_file())
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "if");
            if is_interface
                && let Some(directory) = entry.path().parent()
                && !directories.iter().any(|known| known == directory)
            {
                directories.push(directory.to_path_buf());
            }
        }
    }
    directories
}

#[cfg(test)]
mod walk_tests {
    use super::interface_directories;

    #[test]
    fn directories_with_interface_files_are_found_skipping_ignored_ones() {
        let root = std::env::temp_dir().join(format!("if_lsp_walk_{}", std::process::id()));
        for (directory, file) in [
            ("drivers/a/interfaces", "a_v1.if"),
            ("drivers/b/interfaces", "b_v1.if"),
            ("drivers/b/interfaces", "imports.if"),
            ("generated/out", "generated.if"),
            (".", ".gitignore"),
            (".git/hooks", "hidden.if"),
            ("docs", "notes.md"),
        ] {
            std::fs::create_dir_all(root.join(directory)).expect("a scratch directory");
            std::fs::write(
                root.join(directory).join(file),
                if file == ".gitignore" {
                    "generated/\n"
                } else {
                    ""
                },
            )
            .expect("a scratch file");
        }
        let mut found = interface_directories(std::slice::from_ref(&root));
        found.sort();
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(
            found,
            [
                root.join("drivers/a/interfaces"),
                root.join("drivers/b/interfaces")
            ]
        );
    }
}
