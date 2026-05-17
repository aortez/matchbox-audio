use std::{
    future::Future,
    os::unix::{fs::PermissionsExt, net::UnixStream as StdUnixStream},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};

use anyhow::{anyhow, Context};
use mba_protocol::{BtControlRequest, BtControlResponse};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    task::JoinHandle,
};
use tracing::{debug, info, warn};

pub type ControlFuture = Pin<Box<dyn Future<Output = BtControlResponse> + Send>>;
pub type ControlHandler = Arc<dyn Fn(BtControlRequest) -> ControlFuture + Send + Sync>;

#[derive(Debug)]
pub struct ControlSocketGuard {
    path: PathBuf,
    task: JoinHandle<()>,
}

impl Drop for ControlSocketGuard {
    fn drop(&mut self) {
        self.task.abort();
        if let Err(error) = std::fs::remove_file(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(
                    socket = %self.path.display(),
                    %error,
                    "failed to remove control socket"
                );
            }
        }
    }
}

pub fn start_control_socket(
    path: impl Into<PathBuf>,
    handler: ControlHandler,
) -> anyhow::Result<ControlSocketGuard> {
    let path = path.into();
    prepare_socket_path(&path)?;
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind control socket {}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660))
        .with_context(|| format!("failed to chmod control socket {}", path.display()))?;

    let task_path = path.clone();
    let task = tokio::spawn(async move {
        serve_control_socket(listener, task_path, handler).await;
    });

    info!(socket = %path.display(), "control socket listening");
    Ok(ControlSocketGuard { path, task })
}

fn prepare_socket_path(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create control socket dir {}", parent.display()))?;
    }

    if !path.exists() {
        return Ok(());
    }

    match StdUnixStream::connect(path) {
        Ok(_) => Err(anyhow!(
            "control socket {} already has a listener",
            path.display()
        )),
        Err(_) => std::fs::remove_file(path)
            .with_context(|| format!("failed to remove stale control socket {}", path.display())),
    }
}

async fn serve_control_socket(listener: UnixListener, path: PathBuf, handler: ControlHandler) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    handle_control_connection(stream, handler).await;
                });
            }
            Err(error) => {
                warn!(
                    socket = %path.display(),
                    %error,
                    "control socket accept failed"
                );
                break;
            }
        }
    }

    debug!(socket = %path.display(), "control socket accept loop exited");
}

async fn handle_control_connection(mut stream: UnixStream, handler: ControlHandler) {
    let mut request_bytes = Vec::new();
    if let Err(error) = stream.read_to_end(&mut request_bytes).await {
        warn!(%error, "failed to read control request");
        return;
    }

    let response = match serde_json::from_slice::<BtControlRequest>(&request_bytes) {
        Ok(request) => handler(request).await,
        Err(error) => BtControlResponse::error(
            "bad_request",
            format!("invalid control request JSON: {error}"),
        ),
    };

    let response_bytes = match serde_json::to_vec(&response) {
        Ok(response_bytes) => response_bytes,
        Err(error) => {
            warn!(%error, "failed to encode control response");
            return;
        }
    };

    if let Err(error) = stream.write_all(&response_bytes).await {
        warn!(%error, "failed to write control response");
        return;
    }
    if let Err(error) = stream.shutdown().await {
        warn!(%error, "failed to close control response");
    }
}

#[cfg(test)]
mod tests {
    use mba_protocol::{
        BtAdapterStatus, BtControlResponse, BtStatus, BT_CONTROL_METHOD_STATUS, SERVICE_NAME,
    };

    use super::*;

    #[tokio::test]
    async fn control_socket_serves_status_response() {
        let path = unique_test_socket_path();
        let handler: ControlHandler = Arc::new(|request| {
            Box::pin(async move {
                if request.method != BT_CONTROL_METHOD_STATUS {
                    return BtControlResponse::error("unsupported_method", request.method);
                }
                BtControlResponse::ok_status(BtStatus {
                    service: SERVICE_NAME.to_string(),
                    transport: "test".to_string(),
                    device_name: "Matchbox Audio".to_string(),
                    adapter: Some(BtAdapterStatus {
                        name: "hci0".to_string(),
                        address: "88:A2:9E:B1:87:91".to_string(),
                    }),
                    advertising: true,
                    service_uuid: "service-uuid".to_string(),
                    pairing_state: "local".to_string(),
                    busy: false,
                    active_client: None,
                    rx_chunk_writes: 0,
                    tx_chunks_sent: 0,
                })
            })
        });
        let _guard = start_control_socket(&path, handler).expect("socket starts");

        let mut stream = UnixStream::connect(&path).await.expect("client connects");
        let request = serde_json::to_vec(&BtControlRequest::status()).expect("request encodes");
        stream.write_all(&request).await.expect("request writes");
        stream.shutdown().await.expect("client write closes");

        let mut response_bytes = Vec::new();
        stream
            .read_to_end(&mut response_bytes)
            .await
            .expect("response reads");
        let response: BtControlResponse =
            serde_json::from_slice(&response_bytes).expect("response decodes");

        assert!(response.ok);
        assert_eq!(
            response
                .status
                .expect("status")
                .adapter
                .expect("adapter")
                .name,
            "hci0"
        );
    }

    fn unique_test_socket_path() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "mba-bt-control-test-{}-{nanos}.sock",
            std::process::id()
        ))
    }
}
