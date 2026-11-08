use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = dontbrowse::cli::Cli::parse();
    dontbrowse::run(cli).await
}
