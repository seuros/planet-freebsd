//! `planet`: polls a list of feeds and writes a static "planet" site.

mod app;
mod cli;
mod config;
mod fetch;
mod ingest;
mod refresh;
mod render;
mod state;
mod text;

use cli::Planet;
use usage_rs::RunAsyncWith;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Planet::parse();
    let app = app::App { config: cli.config };
    cli.command.run_async_with(&app).await
}
