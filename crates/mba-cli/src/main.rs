use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use mba_protocol::StatusResponse;
use reqwest::Url;

#[derive(Debug, Parser)]
#[command(author, version, about = "Matchbox Audio command-line client")]
struct Cli {
    /// Matchbox Audio daemon base URL.
    #[arg(long, global = true, default_value = "http://127.0.0.1:8090")]
    server: Url,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show daemon status.
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Status => show_status(cli.server).await,
    }
}

async fn show_status(server: Url) -> Result<()> {
    let url = api_url(server, "/api/v1/status");
    let status = reqwest::get(url.clone())
        .await
        .with_context(|| format!("failed to connect to {url}"))?
        .error_for_status()
        .with_context(|| format!("status request failed for {url}"))?
        .json::<StatusResponse>()
        .await
        .with_context(|| format!("failed to decode status response from {url}"))?;

    println!("service: {}", status.service.name);
    println!("state: {}", status.service.state);
    println!("api: {}", status.service.api_version);
    println!("version: {}", status.build.version);
    if let Some(git_sha) = status.build.git_sha {
        println!("git_sha: {git_sha}");
    }
    if let Some(network) = status.network {
        println!("network_mode: {}", network.mode);
        println!("network_active_connection: {}", network.active_connection);
        println!("network_ssid: {}", network.ssid);
        println!("network_ip4: {}", network.ip4);
        println!("hotspot_ssid: {}", network.hotspot_ssid);
    }

    Ok(())
}

fn api_url(mut server: Url, path: &str) -> Url {
    server.set_path(path);
    server.set_query(None);
    server
}
