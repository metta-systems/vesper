use zed_extension_api::{self as zed, LanguageServerId, Result};

struct Meddle;

impl zed::Extension for Meddle {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let path = worktree.which("if_lsp").ok_or("if_lsp not found on PATH")?;
        Ok(zed::Command {
            command: path,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(Meddle);
