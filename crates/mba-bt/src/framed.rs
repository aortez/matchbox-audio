use std::{error::Error, fmt};

use mba_protocol::{MessageError, ProtocolMessage, MAX_MESSAGE_BYTES};

pub const FRAME_HEADER_BYTES: usize = 4;

#[derive(Debug, Default, Clone)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<ProtocolMessage>, FrameError> {
        self.buffer.extend_from_slice(bytes);

        let mut messages = Vec::new();
        loop {
            if self.buffer.len() < FRAME_HEADER_BYTES {
                break;
            }

            let len = u32::from_le_bytes(
                self.buffer[0..FRAME_HEADER_BYTES]
                    .try_into()
                    .expect("slice length checked"),
            ) as usize;
            if len > MAX_MESSAGE_BYTES {
                self.buffer.clear();
                return Err(FrameError::FrameTooLarge {
                    len,
                    max: MAX_MESSAGE_BYTES,
                });
            }
            if self.buffer.len() < FRAME_HEADER_BYTES + len {
                break;
            }

            let payload = self.buffer[FRAME_HEADER_BYTES..FRAME_HEADER_BYTES + len].to_vec();
            self.buffer.drain(..FRAME_HEADER_BYTES + len);
            messages.push(ProtocolMessage::from_json_bytes(&payload)?);
        }

        Ok(messages)
    }

    pub fn has_partial_frame(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
}

pub fn encode_frame(message: &ProtocolMessage) -> Result<Vec<u8>, FrameError> {
    let payload = message.to_json_bytes()?;
    let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[derive(Debug)]
pub enum FrameError {
    Message(MessageError),
    FrameTooLarge { len: usize, max: usize },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(error) => write!(f, "{error}"),
            Self::FrameTooLarge { len, max } => write!(f, "frame too large: {len} > {max}"),
        }
    }
}

impl Error for FrameError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Message(error) => Some(error),
            Self::FrameTooLarge { .. } => None,
        }
    }
}

impl From<MessageError> for FrameError {
    fn from(error: MessageError) -> Self {
        Self::Message(error)
    }
}
