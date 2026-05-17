use mba_protocol::{
    select_protocol_version, ErrorCode, HelloParams, HelloResult, PairingInfo, PairingState,
    ProtocolError, ProtocolMessage, RequestId, SnapshotResult, TransportInfo,
    CAPABILITY_BLE_CHUNK_V1, EVENT_PLAYBACK_CHANGED, METHOD_EVENTS_SUBSCRIBE, METHOD_SYSTEM_HELLO,
    METHOD_SYSTEM_SNAPSHOT,
};
use serde_json::{json, Value};
use tracing::info;

use crate::{PlayerBackend, PlayerError};

#[derive(Debug, Clone)]
pub struct RequestRouter<P> {
    player: P,
    build_version: String,
    trusted: bool,
}

impl<P> RequestRouter<P>
where
    P: PlayerBackend,
{
    pub fn new(player: P) -> Self {
        Self {
            player,
            build_version: "mba-bt-local".to_string(),
            trusted: true,
        }
    }

    pub fn with_build_version(mut self, build_version: impl Into<String>) -> Self {
        self.build_version = build_version.into();
        self
    }

    pub fn with_trusted_client(mut self, trusted: bool) -> Self {
        self.trusted = trusted;
        self
    }

    pub async fn route(&self, message: ProtocolMessage) -> RouteOutput {
        match message {
            ProtocolMessage::Request { id, method, params } => {
                self.route_request(id, &method, params).await
            }
            ProtocolMessage::Response { .. } | ProtocolMessage::Event { .. } => {
                RouteOutput::single(ProtocolMessage::error_response(
                    0,
                    ProtocolError::new(ErrorCode::BadRequest, "expected request message"),
                ))
            }
        }
    }

    pub async fn route_bytes(&self, bytes: &[u8]) -> RouteOutput {
        match ProtocolMessage::from_json_bytes(bytes) {
            Ok(message) => self.route(message).await,
            Err(error) => RouteOutput::single(ProtocolMessage::error_response(
                0,
                ProtocolError::new(ErrorCode::BadRequest, error.to_string()),
            )),
        }
    }

    async fn route_request(
        &self,
        id: RequestId,
        method: &str,
        params: Option<Value>,
    ) -> RouteOutput {
        info!(request_id = id, %method, "routing protocol request");
        match method {
            METHOD_SYSTEM_HELLO => self.system_hello(id, params),
            _ if !self.trusted => RouteOutput::single(ProtocolMessage::error_response(
                id,
                ProtocolError::new(
                    ErrorCode::AuthRequired,
                    "pairing is required before control",
                ),
            )),
            METHOD_SYSTEM_SNAPSHOT => self.system_snapshot(id).await,
            METHOD_EVENTS_SUBSCRIBE => self.events_subscribe(id).await,
            _ => RouteOutput::single(ProtocolMessage::error_response(
                id,
                ProtocolError::new(
                    ErrorCode::UnsupportedMethod,
                    format!("unsupported method: {method}"),
                ),
            )),
        }
    }

    fn system_hello(&self, id: RequestId, params: Option<Value>) -> RouteOutput {
        let params: HelloParams = match params {
            Some(params) => match serde_json::from_value(params) {
                Ok(params) => params,
                Err(error) => {
                    return RouteOutput::single(ProtocolMessage::error_response(
                        id,
                        ProtocolError::new(
                            ErrorCode::BadRequest,
                            format!("invalid system.hello params: {error}"),
                        ),
                    ));
                }
            },
            None => {
                return RouteOutput::single(ProtocolMessage::error_response(
                    id,
                    ProtocolError::new(ErrorCode::BadRequest, "system.hello params are required"),
                ));
            }
        };

        let selected_protocol_version =
            match select_protocol_version(&params.supported_protocol_versions) {
                Ok(version) => version,
                Err(error) => {
                    return RouteOutput::single(ProtocolMessage::error_response(id, error));
                }
            };

        let result = HelloResult {
            selected_protocol_version,
            device_name: mba_protocol::DEVICE_DISPLAY_NAME.to_string(),
            build_version: self.build_version.clone(),
            capabilities: vec![
                METHOD_SYSTEM_HELLO.to_string(),
                METHOD_SYSTEM_SNAPSHOT.to_string(),
                METHOD_EVENTS_SUBSCRIBE.to_string(),
                CAPABILITY_BLE_CHUNK_V1.to_string(),
            ],
            transport: TransportInfo::ble_gatt_default(),
            pairing: PairingInfo {
                state: if self.trusted {
                    PairingState::Trusted
                } else {
                    PairingState::PairingRequired
                },
                trusted: self.trusted,
            },
        };

        RouteOutput::single(success_response(id, result))
    }

    async fn system_snapshot(&self, id: RequestId) -> RouteOutput {
        match self.player.snapshot().await {
            Ok(status) => RouteOutput::single(success_response(id, SnapshotResult { status })),
            Err(error) => RouteOutput::single(ProtocolMessage::error_response(
                id,
                protocol_error_for_player_error(error),
            )),
        }
    }

    async fn events_subscribe(&self, id: RequestId) -> RouteOutput {
        let response = ProtocolMessage::ok_response(id, Some(json!({ "subscribed": true })));
        let mut output = RouteOutput::single(response);

        if let Ok(status) = self.player.snapshot().await {
            if let Some(playback) = status.playback {
                output.events.push(ProtocolMessage::event(
                    EVENT_PLAYBACK_CHANGED,
                    Some(json!({ "playback": playback })),
                ));
            }
        }

        output
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteOutput {
    pub response: Option<ProtocolMessage>,
    pub events: Vec<ProtocolMessage>,
}

impl RouteOutput {
    pub fn single(response: ProtocolMessage) -> Self {
        Self {
            response: Some(response),
            events: Vec::new(),
        }
    }

    pub fn messages(self) -> Vec<ProtocolMessage> {
        let mut messages =
            Vec::with_capacity(usize::from(self.response.is_some()) + self.events.len());
        if let Some(response) = self.response {
            messages.push(response);
        }
        messages.extend(self.events);
        messages
    }
}

fn success_response<T>(id: RequestId, result: T) -> ProtocolMessage
where
    T: serde::Serialize,
{
    let result = serde_json::to_value(result).expect("protocol result serializes");
    ProtocolMessage::ok_response(id, Some(result))
}

fn protocol_error_for_player_error(error: PlayerError) -> ProtocolError {
    match error {
        PlayerError::Unavailable(message) => {
            ProtocolError::new(ErrorCode::PlayerUnavailable, message)
        }
        PlayerError::Internal(message) => ProtocolError::new(ErrorCode::Internal, message),
    }
}
