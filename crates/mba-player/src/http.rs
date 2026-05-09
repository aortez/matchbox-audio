use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use axum::{extract::State, response::Html, routing::get, Json, Router};
use mba_protocol::{NetworkInfo, NetworkMode, StatusResponse};
use tokio::{process::Command, time::timeout};
use tracing::warn;

#[derive(Debug, Clone)]
pub struct AppState {
    pub status: StatusResponse,
    pub network_script: PathBuf,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/v1/status", get(status))
        .with_state(state)
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let network = read_network_status(&state.network_script).await;
    let network_mode = network
        .as_ref()
        .map(|network| network.mode.as_str())
        .unwrap_or("unknown");
    Html(format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Matchbox Audio</title>
</head>
<body>
  <main>
    <h1>Matchbox Audio</h1>
    <p>Service state: {state}</p>
    <p>Version: {version}</p>
    <p>Network: {network_mode}</p>
    <p>API: /api/v1/status</p>
  </main>
</body>
</html>
"#,
        state = html_escape(&state.status.service.state.to_string()),
        version = html_escape(&state.status.build.version),
        network_mode = html_escape(network_mode),
    ))
}

fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    let mut status = state.status.clone();
    status.network = read_network_status(&state.network_script).await;
    Json(status)
}

async fn read_network_status(network_script: &Path) -> Option<NetworkInfo> {
    let stdout = match run_network_status_command(network_script).await {
        Ok(stdout) => stdout,
        Err(error) => {
            warn!(
                %error,
                path = %network_script.display(),
                "network status helper failed"
            );
            return None;
        }
    };

    Some(NetworkInfo {
        mode: NetworkMode::parse(parse_field(&stdout, "mode").unwrap_or("unknown")),
        active_connection: parse_field(&stdout, "active_connection")
            .unwrap_or("none")
            .to_string(),
        ssid: parse_field(&stdout, "ssid").unwrap_or("none").to_string(),
        ip4: parse_field(&stdout, "ip4").unwrap_or("none").to_string(),
        hotspot_ssid: parse_field(&stdout, "hotspot_ssid")
            .unwrap_or("matchbox-audio")
            .to_string(),
    })
}

async fn run_network_status_command(network_script: &Path) -> Result<String, String> {
    let mut command = Command::new(network_script);
    command.arg("status");

    let output = match timeout(Duration::from_secs(5), command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(format!("failed to run helper: {error}")),
        Err(_) => return Err("helper timed out".to_string()),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "helper exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_field<'a>(output: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}=");
    output
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
