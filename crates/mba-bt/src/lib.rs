pub mod ble_transport;
pub mod control;
pub mod framed;
pub mod gatt;
pub mod player;
pub mod router;
pub mod session;
pub mod state;

pub use ble_transport::*;
pub use control::*;
pub use framed::*;
pub use gatt::*;
pub use player::*;
pub use router::*;
pub use session::*;
pub use state::*;

#[cfg(test)]
mod tests {
    use mba_protocol::{
        ble, ChunkReassembler, ErrorCode, ProtocolMessage, METHOD_EVENTS_SUBSCRIBE,
        METHOD_SYSTEM_HELLO, METHOD_SYSTEM_SNAPSHOT,
    };
    use serde_json::{json, Value};

    use crate::{
        encode_frame, FakePlayerBackend, FrameDecoder, InMemoryBleTransport, RequestRouter,
        SessionGate,
    };

    fn hello_request(id: u64) -> ProtocolMessage {
        ProtocolMessage::request(
            id,
            METHOD_SYSTEM_HELLO,
            Some(json!({
                "app": "mba-bt-test",
                "supported_protocol_versions": [1]
            })),
        )
    }

    async fn route_one(
        router: &RequestRouter<FakePlayerBackend>,
        message: ProtocolMessage,
    ) -> ProtocolMessage {
        let mut messages = router.route(message).await.messages();
        assert_eq!(messages.len(), 1);
        messages.remove(0)
    }

    fn response_result(message: ProtocolMessage) -> Value {
        match message {
            ProtocolMessage::Response {
                ok,
                result: Some(result),
                error,
                ..
            } => {
                assert!(ok);
                assert_eq!(error, None);
                result
            }
            other => panic!("expected successful response, got {other:?}"),
        }
    }

    fn response_error_code(message: ProtocolMessage) -> ErrorCode {
        match message {
            ProtocolMessage::Response {
                ok,
                error: Some(error),
                ..
            } => {
                assert!(!ok);
                error.code
            }
            other => panic!("expected error response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_router_handles_system_hello() {
        let router = RequestRouter::new(FakePlayerBackend::ready());

        let result = response_result(route_one(&router, hello_request(1)).await);

        assert_eq!(result["selected_protocol_version"], 1);
        assert_eq!(result["device_name"], "Matchbox Audio");
        assert_eq!(result["transport"]["kind"], "ble-gatt");
    }

    #[tokio::test]
    async fn request_router_handles_system_snapshot() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let request = ProtocolMessage::request(2, METHOD_SYSTEM_SNAPSHOT, None);

        let result = response_result(route_one(&router, request).await);

        assert_eq!(result["status"]["service"]["name"], "matchbox-audio");
        assert_eq!(result["status"]["playback"]["state"], "play");
    }

    #[tokio::test]
    async fn request_router_rejects_unsupported_method() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let request = ProtocolMessage::request(3, "debug.noop", None);

        let code = response_error_code(route_one(&router, request).await);

        assert_eq!(code, ErrorCode::UnsupportedMethod);
    }

    #[tokio::test]
    async fn request_router_returns_auth_required_for_untrusted_control() {
        let router = RequestRouter::new(FakePlayerBackend::ready()).with_trusted_client(false);
        let request = ProtocolMessage::request(4, METHOD_SYSTEM_SNAPSHOT, None);

        let code = response_error_code(route_one(&router, request).await);

        assert_eq!(code, ErrorCode::AuthRequired);
    }

    #[tokio::test]
    async fn request_router_reports_player_unavailable() {
        let router = RequestRouter::new(FakePlayerBackend::unavailable("mpd offline"));
        let request = ProtocolMessage::request(5, METHOD_SYSTEM_SNAPSHOT, None);

        let code = response_error_code(route_one(&router, request).await);

        assert_eq!(code, ErrorCode::PlayerUnavailable);
    }

    #[tokio::test]
    async fn request_router_delivers_fake_event_after_subscribe() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let request = ProtocolMessage::request(6, METHOD_EVENTS_SUBSCRIBE, None);

        let messages = router.route(request).await.messages();

        assert_eq!(messages.len(), 2);
        response_result(messages[0].clone());
        match &messages[1] {
            ProtocolMessage::Event { event, payload } => {
                assert_eq!(event, "playback.changed");
                assert_eq!(
                    payload.as_ref().expect("payload")["playback"]["state"],
                    "play"
                );
            }
            other => panic!("expected event, got {other:?}"),
        }
    }

    #[test]
    fn session_gate_rejects_second_client_and_cleans_up_on_drop() {
        let gate = SessionGate::new();
        let session = gate.connect(10).expect("first client connects");

        let busy = gate.connect(11).expect_err("second client is busy");
        assert_eq!(busy.code, ErrorCode::Busy);

        drop(session);
        assert!(gate.connect(11).is_ok());
    }

    #[test]
    fn frame_decoder_handles_partial_frames() {
        let frame = encode_frame(&hello_request(7)).expect("frame encodes");
        let split = frame.len() / 2;
        let mut decoder = FrameDecoder::new();

        assert!(decoder
            .push(&frame[..split])
            .expect("partial accepted")
            .is_empty());
        assert!(decoder.has_partial_frame());

        let messages = decoder.push(&frame[split..]).expect("frame completes");
        assert_eq!(messages, vec![hello_request(7)]);
        assert!(!decoder.has_partial_frame());
    }

    #[tokio::test]
    async fn framed_messages_can_route_through_request_router() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let frame = encode_frame(&hello_request(8)).expect("frame encodes");
        let mut decoder = FrameDecoder::new();
        let messages = decoder.push(&frame).expect("frame decodes");

        let result = response_result(route_one(&router, messages[0].clone()).await);

        assert_eq!(result["selected_protocol_version"], 1);
    }

    #[tokio::test]
    async fn ble_chunks_can_route_through_request_router() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let request_bytes = hello_request(9).to_json_bytes().expect("request encodes");
        let rx_chunks =
            ble::encode_chunks_with_payload_size(77, &request_bytes, 12).expect("chunks encode");
        let mut transport = InMemoryBleTransport::new();
        let mut tx_chunks = None;

        for chunk in rx_chunks {
            tx_chunks = transport
                .receive_chunk(&router, &chunk)
                .await
                .expect("chunk accepted");
        }

        let mut reassembler = ChunkReassembler::new();
        let mut response_bytes = None;
        for chunk in tx_chunks.expect("response chunks") {
            response_bytes = reassembler.push_bytes(&chunk).expect("tx chunk accepted");
        }

        let response = ProtocolMessage::from_json_bytes(&response_bytes.expect("response bytes"))
            .expect("response parses");
        let result = response_result(response);
        assert_eq!(result["transport"]["target_chunk_payload_bytes"], 228);
    }

    #[tokio::test]
    async fn ble_transport_rejects_malformed_chunk() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let mut chunk = ble::encode_chunks(1, b"{}")
            .expect("chunks encode")
            .remove(0);
        chunk[0] = b'X';
        let mut transport = InMemoryBleTransport::new();

        let error = transport
            .receive_chunk(&router, &chunk)
            .await
            .expect_err("bad chunk rejected");

        assert!(error.to_string().contains("bad chunk magic"));
    }

    #[tokio::test]
    async fn ble_transport_returns_bad_request_for_malformed_message() {
        let router = RequestRouter::new(FakePlayerBackend::ready());
        let rx_chunks = ble::encode_chunks(1, b"{").expect("chunks encode");
        let mut transport = InMemoryBleTransport::new();
        let tx_chunks = transport
            .receive_chunk(&router, &rx_chunks[0])
            .await
            .expect("chunk accepted")
            .expect("response chunks");

        let mut reassembler = ChunkReassembler::new();
        let mut response_bytes = None;
        for chunk in tx_chunks {
            response_bytes = reassembler.push_bytes(&chunk).expect("tx chunk accepted");
        }

        let response = ProtocolMessage::from_json_bytes(&response_bytes.expect("response bytes"))
            .expect("response parses");
        assert_eq!(response_error_code(response), ErrorCode::BadRequest);
    }
}
