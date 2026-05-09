use axum::{extract::State, response::Html, routing::get, Json, Router};
use mba_protocol::StatusResponse;

#[derive(Debug, Clone)]
pub struct AppState {
    pub status: StatusResponse,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/v1/status", get(status))
        .with_state(state)
}

async fn index(State(state): State<AppState>) -> Html<String> {
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
    <p>API: /api/v1/status</p>
  </main>
</body>
</html>
"#,
        state = state.status.service.state,
        version = state.status.build.version,
    ))
}

async fn status(State(state): State<AppState>) -> Json<StatusResponse> {
    Json(state.status)
}
