//! Language server for the Meddle interface definition language (`.if`).

mod analysis;
mod server;
mod text;

use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    let (service, socket) = LspService::new(server::Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
