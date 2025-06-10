use anyhow::Result;

mod rustdoc;
mod server;

#[tokio::main]
async fn main() -> Result<()> {
    let server = server::RustdocMcpServer::new();
    server.run().await
}
