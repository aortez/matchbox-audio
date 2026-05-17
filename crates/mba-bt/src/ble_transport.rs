use std::{error::Error, fmt};

use mba_protocol::{ble, ChunkError, ChunkReassembler, ErrorCode, ProtocolError, ProtocolMessage};

use crate::{PlayerBackend, RequestRouter};

#[derive(Debug, Clone)]
pub struct InMemoryBleTransport {
    rx_reassembler: ChunkReassembler,
    next_tx_message_id: u32,
}

impl Default for InMemoryBleTransport {
    fn default() -> Self {
        Self {
            rx_reassembler: ChunkReassembler::new(),
            next_tx_message_id: 1,
        }
    }
}

impl InMemoryBleTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn receive_chunk<P>(
        &mut self,
        router: &RequestRouter<P>,
        chunk: &[u8],
    ) -> Result<Option<Vec<Vec<u8>>>, BleTransportError>
    where
        P: PlayerBackend,
    {
        let Some(message_bytes) = self.rx_reassembler.push_bytes(chunk)? else {
            return Ok(None);
        };

        let output = router.route_bytes(&message_bytes).await;
        let mut tx_chunks = Vec::new();
        for message in output.messages() {
            let bytes = message
                .to_json_bytes()
                .map_err(BleTransportError::Message)?;
            let message_id = self.next_transport_message_id();
            tx_chunks.extend(ble::encode_chunks(message_id, &bytes)?);
        }

        Ok(Some(tx_chunks))
    }

    fn next_transport_message_id(&mut self) -> u32 {
        let message_id = self.next_tx_message_id;
        self.next_tx_message_id = self.next_tx_message_id.wrapping_add(1).max(1);
        message_id
    }

    pub fn reset(&mut self) {
        self.rx_reassembler.reset();
    }
}

#[derive(Debug)]
pub enum BleTransportError {
    Chunk(ChunkError),
    Message(mba_protocol::MessageError),
}

impl fmt::Display for BleTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chunk(error) => write!(f, "{error}"),
            Self::Message(error) => write!(f, "{error}"),
        }
    }
}

impl Error for BleTransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Chunk(error) => Some(error),
            Self::Message(error) => Some(error),
        }
    }
}

impl From<ChunkError> for BleTransportError {
    fn from(error: ChunkError) -> Self {
        Self::Chunk(error)
    }
}

pub fn bad_request_for_transport_error(error: BleTransportError) -> ProtocolMessage {
    ProtocolMessage::error_response(
        0,
        ProtocolError::new(ErrorCode::BadRequest, error.to_string()),
    )
}
