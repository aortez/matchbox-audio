use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ble, DEFAULT_PAGE_LIMIT, DEVICE_DISPLAY_NAME, MAX_MESSAGE_BYTES, MAX_PAGE_LIMIT,
    TARGET_RESPONSE_BYTES,
};
use crate::{LibraryDirectory, LibraryTrack, QueueItem, StatusResponse};

pub const METHOD_SYSTEM_HELLO: &str = "system.hello";
pub const METHOD_SYSTEM_SNAPSHOT: &str = "system.snapshot";
pub const METHOD_EVENTS_SUBSCRIBE: &str = "events.subscribe";
pub const METHOD_PLAYBACK_PLAY: &str = "playback.play";
pub const METHOD_PLAYBACK_PAUSE: &str = "playback.pause";
pub const METHOD_PLAYBACK_TOGGLE: &str = "playback.toggle";
pub const METHOD_PLAYBACK_STOP: &str = "playback.stop";
pub const METHOD_PLAYBACK_NEXT: &str = "playback.next";
pub const METHOD_PLAYBACK_PREVIOUS: &str = "playback.previous";
pub const METHOD_PLAYBACK_SEEK: &str = "playback.seek";
pub const METHOD_PLAYBACK_VOLUME: &str = "playback.volume";
pub const METHOD_LIBRARY_LIST: &str = "library.list";
pub const METHOD_LIBRARY_RESCAN: &str = "library.rescan";
pub const METHOD_QUEUE_LIST: &str = "queue.list";
pub const METHOD_QUEUE_ADD_FILE: &str = "queue.addFile";
pub const METHOD_QUEUE_ADD_DIRECTORY: &str = "queue.addDirectory";
pub const METHOD_QUEUE_PLAY: &str = "queue.play";
pub const METHOD_QUEUE_PLAY_NEXT: &str = "queue.playNext";
pub const METHOD_QUEUE_REMOVE: &str = "queue.remove";
pub const METHOD_QUEUE_MOVE: &str = "queue.move";
pub const METHOD_QUEUE_CLEAR: &str = "queue.clear";

pub const EVENT_PLAYBACK_CHANGED: &str = "playback.changed";
pub const EVENT_QUEUE_CHANGED: &str = "queue.changed";
pub const EVENT_LIBRARY_SCAN_STARTED: &str = "library.scan.started";
pub const EVENT_LIBRARY_SCAN_PROGRESS: &str = "library.scan.progress";
pub const EVENT_LIBRARY_SCAN_FINISHED: &str = "library.scan.finished";

pub const CAPABILITY_BLE_CHUNK_V1: &str = "ble.chunk.v1";

pub type RequestId = u64;

pub const KNOWN_METHODS: &[&str] = &[
    METHOD_SYSTEM_HELLO,
    METHOD_SYSTEM_SNAPSHOT,
    METHOD_EVENTS_SUBSCRIBE,
    METHOD_PLAYBACK_PLAY,
    METHOD_PLAYBACK_PAUSE,
    METHOD_PLAYBACK_TOGGLE,
    METHOD_PLAYBACK_STOP,
    METHOD_PLAYBACK_NEXT,
    METHOD_PLAYBACK_PREVIOUS,
    METHOD_PLAYBACK_SEEK,
    METHOD_PLAYBACK_VOLUME,
    METHOD_LIBRARY_LIST,
    METHOD_LIBRARY_RESCAN,
    METHOD_QUEUE_LIST,
    METHOD_QUEUE_ADD_FILE,
    METHOD_QUEUE_ADD_DIRECTORY,
    METHOD_QUEUE_PLAY,
    METHOD_QUEUE_PLAY_NEXT,
    METHOD_QUEUE_REMOVE,
    METHOD_QUEUE_MOVE,
    METHOD_QUEUE_CLEAR,
];

pub fn is_known_method(method: &str) -> bool {
    KNOWN_METHODS.contains(&method)
}

pub fn select_protocol_version(supported_versions: &[u16]) -> Result<u16, ProtocolError> {
    if supported_versions.contains(&crate::APP_PROTOCOL_VERSION) {
        return Ok(crate::APP_PROTOCOL_VERSION);
    }

    Err(ProtocolError::new(
        ErrorCode::UnsupportedVersion,
        format!(
            "no compatible protocol version; device supports {}",
            crate::APP_PROTOCOL_VERSION
        ),
    ))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolMessage {
    Request {
        id: RequestId,
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        params: Option<Value>,
    },
    Response {
        id: RequestId,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ProtocolError>,
    },
    Event {
        event: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload: Option<Value>,
    },
}

impl ProtocolMessage {
    pub fn request(id: RequestId, method: impl Into<String>, params: Option<Value>) -> Self {
        Self::Request {
            id,
            method: method.into(),
            params,
        }
    }

    pub fn ok_response(id: RequestId, result: Option<Value>) -> Self {
        Self::Response {
            id,
            ok: true,
            result,
            error: None,
        }
    }

    pub fn error_response(id: RequestId, error: ProtocolError) -> Self {
        Self::Response {
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }

    pub fn event(event: impl Into<String>, payload: Option<Value>) -> Self {
        Self::Event {
            event: event.into(),
            payload,
        }
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, MessageError> {
        validate_message_len(bytes.len())?;
        Ok(serde_json::from_slice(bytes)?)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, MessageError> {
        let bytes = serde_json::to_vec(self)?;
        validate_message_len(bytes.len())?;
        Ok(bytes)
    }
}

fn validate_message_len(len: usize) -> Result<(), MessageError> {
    if len > MAX_MESSAGE_BYTES {
        return Err(MessageError::TooLarge {
            len,
            max: MAX_MESSAGE_BYTES,
        });
    }
    Ok(())
}

#[derive(Debug)]
pub enum MessageError {
    Json(serde_json::Error),
    TooLarge { len: usize, max: usize },
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid protocol JSON: {error}"),
            Self::TooLarge { len, max } => write!(f, "message too large: {len} > {max}"),
        }
    }
}

impl Error for MessageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::TooLarge { .. } => None,
        }
    }
}

impl From<serde_json::Error> for MessageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    Unauthorized,
    AuthRequired,
    Busy,
    NotFound,
    UnsupportedMethod,
    UnsupportedVersion,
    PlayerUnavailable,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::AuthRequired => "auth_required",
            Self::Busy => "busy",
            Self::NotFound => "not_found",
            Self::UnsupportedMethod => "unsupported_method",
            Self::UnsupportedVersion => "unsupported_version",
            Self::PlayerUnavailable => "player_unavailable",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloParams {
    pub app: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    pub supported_protocol_versions: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResult {
    pub selected_protocol_version: u16,
    pub device_name: String,
    pub build_version: String,
    pub capabilities: Vec<String>,
    pub transport: TransportInfo,
    pub pairing: PairingInfo,
}

impl HelloResult {
    pub fn trusted(build_version: impl Into<String>, capabilities: Vec<String>) -> Self {
        Self {
            selected_protocol_version: crate::APP_PROTOCOL_VERSION,
            device_name: DEVICE_DISPLAY_NAME.to_string(),
            build_version: build_version.into(),
            capabilities,
            transport: TransportInfo::ble_gatt_default(),
            pairing: PairingInfo {
                state: PairingState::Trusted,
                trusted: true,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportInfo {
    pub kind: TransportKind,
    pub chunk_protocol_version: u8,
    pub max_message_bytes: u32,
    pub target_response_bytes: u32,
    pub target_gatt_value_bytes: u16,
    pub chunk_header_bytes: u16,
    pub target_chunk_payload_bytes: u16,
    pub one_in_flight_per_direction: bool,
}

impl TransportInfo {
    pub fn ble_gatt_default() -> Self {
        Self {
            kind: TransportKind::BleGatt,
            chunk_protocol_version: ble::CHUNK_VERSION,
            max_message_bytes: MAX_MESSAGE_BYTES as u32,
            target_response_bytes: TARGET_RESPONSE_BYTES as u32,
            target_gatt_value_bytes: ble::TARGET_GATT_VALUE_BYTES as u16,
            chunk_header_bytes: ble::CHUNK_HEADER_BYTES as u16,
            target_chunk_payload_bytes: ble::TARGET_CHUNK_PAYLOAD_BYTES as u16,
            one_in_flight_per_direction: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportKind {
    BleGatt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingInfo {
    pub state: PairingState,
    pub trusted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    Trusted,
    Untrusted,
    PairingRequired,
    Spike,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotResult {
    pub status: StatusResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_page_limit")]
    pub limit: u16,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

impl PageRequest {
    pub fn bounded_limit(&self) -> u16 {
        self.limit.clamp(1, MAX_PAGE_LIMIT)
    }
}

fn default_page_limit() -> u16 {
    DEFAULT_PAGE_LIMIT
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryListParams {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_page_limit")]
    pub limit: u16,
}

impl Default for LibraryListParams {
    fn default() -> Self {
        Self {
            path: String::new(),
            cursor: None,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueListParams {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default = "default_page_limit")]
    pub limit: u16,
}

impl Default for QueueListParams {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagedLibraryListing {
    pub path: String,
    pub directories: Vec<LibraryDirectory>,
    pub tracks: Vec<LibraryTrack>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagedQueueListing {
    pub items: Vec<QueueItem>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlaybackInfo;
    use serde_json::json;
    use std::{fs, path::PathBuf};

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("protocol")
            .join("fixtures")
            .join("v1")
    }

    fn read_fixture(name: &str) -> String {
        fs::read_to_string(fixture_dir().join(name)).expect("fixture is readable")
    }

    fn read_message_fixture(name: &str) -> ProtocolMessage {
        serde_json::from_str(&read_fixture(name)).expect("fixture parses as protocol message")
    }

    #[test]
    fn error_code_serializes_as_snake_case() {
        let value = serde_json::to_value(ErrorCode::UnsupportedMethod).expect("serializes");
        assert_eq!(value, json!("unsupported_method"));
    }

    #[test]
    fn protocol_message_round_trips_request() {
        let message = ProtocolMessage::request(
            7,
            METHOD_SYSTEM_HELLO,
            Some(json!({
                "app": "android-ble-smoke",
                "supported_protocol_versions": [1]
            })),
        );

        let bytes = message.to_json_bytes().expect("encodes");
        let decoded = ProtocolMessage::from_json_bytes(&bytes).expect("decodes");

        assert_eq!(decoded, message);
    }

    #[test]
    fn protocol_message_rejects_oversized_json() {
        let message = ProtocolMessage::request(
            1,
            METHOD_SYSTEM_HELLO,
            Some(json!({ "padding": "x".repeat(MAX_MESSAGE_BYTES) })),
        );

        let error = message.to_json_bytes().expect_err("message is too large");

        assert!(matches!(error, MessageError::TooLarge { .. }));
    }

    #[test]
    fn protocol_message_rejects_malformed_json() {
        let error = ProtocolMessage::from_json_bytes(b"{").expect_err("malformed JSON fails");

        assert!(matches!(error, MessageError::Json(_)));
    }

    #[test]
    fn known_method_list_accepts_defined_methods() {
        assert!(is_known_method(METHOD_SYSTEM_HELLO));
        assert!(is_known_method(METHOD_QUEUE_PLAY_NEXT));
        assert!(!is_known_method("debug.noop"));
    }

    #[test]
    fn protocol_version_selection_rejects_unsupported_versions() {
        assert_eq!(select_protocol_version(&[1]).expect("v1 is supported"), 1);

        let error = select_protocol_version(&[99]).expect_err("v99 is unsupported");
        assert_eq!(error.code, ErrorCode::UnsupportedVersion);
    }

    #[test]
    fn page_request_bounds_limit() {
        assert_eq!(
            PageRequest {
                cursor: None,
                limit: 0
            }
            .bounded_limit(),
            1
        );
        assert_eq!(
            PageRequest {
                cursor: None,
                limit: 500
            }
            .bounded_limit(),
            MAX_PAGE_LIMIT
        );
    }

    #[test]
    fn v1_fixtures_parse_as_protocol_messages() {
        for fixture in [
            "system_hello_request.json",
            "system_hello_response.json",
            "system_snapshot_request.json",
            "system_snapshot_response.json",
            "playback_changed_event.json",
            "unsupported_method_error_response.json",
            "library_list_request.json",
            "library_list_response.json",
            "queue_list_request.json",
            "queue_list_response.json",
        ] {
            read_message_fixture(fixture);
        }
    }

    #[test]
    fn v1_request_fixtures_parse_into_typed_params() {
        match read_message_fixture("system_hello_request.json") {
            ProtocolMessage::Request { method, params, .. } => {
                assert_eq!(method, METHOD_SYSTEM_HELLO);
                let params: HelloParams =
                    serde_json::from_value(params.expect("params")).expect("hello params parse");
                assert_eq!(params.app, "android-ble-smoke");
                assert_eq!(params.supported_protocol_versions, vec![1]);
            }
            _ => panic!("expected request"),
        }

        match read_message_fixture("library_list_request.json") {
            ProtocolMessage::Request { method, params, .. } => {
                assert_eq!(method, METHOD_LIBRARY_LIST);
                let params: LibraryListParams =
                    serde_json::from_value(params.expect("params")).expect("library params parse");
                assert_eq!(params.path, "Pink Floyd");
                assert_eq!(params.limit, DEFAULT_PAGE_LIMIT);
                assert_eq!(params.cursor, None);
            }
            _ => panic!("expected request"),
        }

        match read_message_fixture("queue_list_request.json") {
            ProtocolMessage::Request { method, params, .. } => {
                assert_eq!(method, METHOD_QUEUE_LIST);
                let params: QueueListParams =
                    serde_json::from_value(params.expect("params")).expect("queue params parse");
                assert_eq!(params.limit, DEFAULT_PAGE_LIMIT);
                assert_eq!(params.cursor, None);
            }
            _ => panic!("expected request"),
        }
    }

    #[test]
    fn v1_response_fixtures_parse_into_typed_results() {
        match read_message_fixture("system_hello_response.json") {
            ProtocolMessage::Response {
                ok, result, error, ..
            } => {
                assert!(ok);
                assert_eq!(error, None);
                let result: HelloResult =
                    serde_json::from_value(result.expect("result")).expect("hello result parse");
                assert_eq!(result.transport.kind, TransportKind::BleGatt);
                assert_eq!(result.transport.target_chunk_payload_bytes, 228);
                assert!(result.pairing.trusted);
            }
            _ => panic!("expected response"),
        }

        match read_message_fixture("system_snapshot_response.json") {
            ProtocolMessage::Response { ok, result, .. } => {
                assert!(ok);
                let result: SnapshotResult =
                    serde_json::from_value(result.expect("result")).expect("snapshot parse");
                assert_eq!(result.status.service.name, crate::SERVICE_NAME);
                assert_eq!(
                    result
                        .status
                        .playback
                        .expect("playback")
                        .track
                        .expect("track")
                        .title,
                    Some("Speak to Me".to_string())
                );
            }
            _ => panic!("expected response"),
        }

        match read_message_fixture("library_list_response.json") {
            ProtocolMessage::Response { ok, result, .. } => {
                assert!(ok);
                let result: PagedLibraryListing =
                    serde_json::from_value(result.expect("result")).expect("library result parse");
                assert!(result.has_more);
                assert_eq!(result.next_cursor.as_deref(), Some("Pink Floyd:50"));
                assert_eq!(result.directories[0].name, "Dark Side");
            }
            _ => panic!("expected response"),
        }

        match read_message_fixture("queue_list_response.json") {
            ProtocolMessage::Response { ok, result, .. } => {
                assert!(ok);
                let result: PagedQueueListing =
                    serde_json::from_value(result.expect("result")).expect("queue result parse");
                assert!(!result.has_more);
                assert_eq!(result.items[0].id, Some(12));
            }
            _ => panic!("expected response"),
        }
    }

    #[test]
    fn v1_error_fixture_uses_stable_error_code() {
        match read_message_fixture("unsupported_method_error_response.json") {
            ProtocolMessage::Response {
                ok,
                error: Some(error),
                ..
            } => {
                assert!(!ok);
                assert_eq!(error.code, ErrorCode::UnsupportedMethod);
                assert_eq!(error.code.as_str(), "unsupported_method");
            }
            _ => panic!("expected error response"),
        }
    }

    #[test]
    fn v1_event_fixture_parses_payload() {
        match read_message_fixture("playback_changed_event.json") {
            ProtocolMessage::Event { event, payload } => {
                assert_eq!(event, EVENT_PLAYBACK_CHANGED);
                let payload = payload.expect("payload");
                let playback: PlaybackInfo =
                    serde_json::from_value(payload["playback"].clone()).expect("playback parses");
                assert_eq!(playback.state, crate::PlaybackState::Pause);
                assert_eq!(playback.track.expect("track").elapsed_s, Some(15));
            }
            _ => panic!("expected event"),
        }
    }
}
