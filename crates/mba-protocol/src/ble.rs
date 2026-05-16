use std::{error::Error, fmt};

use crate::MAX_MESSAGE_BYTES;

pub const SERVICE_UUID: &str = "1cef04f1-966e-43ad-860f-086db4f277d6";
pub const STATUS_UUID: &str = "bd539314-4637-416b-a3b5-804fecd5b792";
pub const RX_UUID: &str = "fbf39e22-bb07-49bf-bfa0-3dbdfc47769b";
pub const TX_UUID: &str = "fcc9055c-34e3-46d9-a010-bd8a4f180b0c";

pub const CHUNK_MAGIC: [u8; 2] = *b"MB";
pub const CHUNK_VERSION: u8 = 1;
pub const FLAG_FIRST_CHUNK: u8 = 0x01;
pub const FLAG_LAST_CHUNK: u8 = 0x02;
pub const KNOWN_FLAGS: u8 = FLAG_FIRST_CHUNK | FLAG_LAST_CHUNK;
pub const CHUNK_HEADER_BYTES: usize = 16;
pub const TARGET_GATT_VALUE_BYTES: usize = 244;
pub const TARGET_CHUNK_PAYLOAD_BYTES: usize = TARGET_GATT_VALUE_BYTES - CHUNK_HEADER_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub flags: u8,
    pub message_id: u32,
    pub chunk_index: u16,
    pub chunk_count: u16,
    pub total_message_len: usize,
    pub payload_fragment: Vec<u8>,
}

impl Chunk {
    pub fn is_first(&self) -> bool {
        self.flags & FLAG_FIRST_CHUNK != 0
    }

    pub fn is_last(&self) -> bool {
        self.flags & FLAG_LAST_CHUNK != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialMessage {
    message_id: u32,
    next_chunk_index: u16,
    chunk_count: u16,
    total_message_len: usize,
    payload: Vec<u8>,
}

#[derive(Debug, Default, Clone)]
pub struct ChunkReassembler {
    partial: Option<PartialMessage>,
}

impl ChunkReassembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, ChunkError> {
        let chunk = match parse_chunk(bytes) {
            Ok(chunk) => chunk,
            Err(error) => {
                self.reset();
                return Err(error);
            }
        };
        self.push_chunk(chunk)
    }

    pub fn push_chunk(&mut self, chunk: Chunk) -> Result<Option<Vec<u8>>, ChunkError> {
        if chunk.chunk_index == 0 {
            if self.partial.is_some() {
                self.reset();
                return Err(ChunkError::UnexpectedFirstChunk);
            }
            self.partial = Some(PartialMessage {
                message_id: chunk.message_id,
                next_chunk_index: 0,
                chunk_count: chunk.chunk_count,
                total_message_len: chunk.total_message_len,
                payload: Vec::with_capacity(chunk.total_message_len),
            });
        }

        let validation_error = match self.partial.as_ref() {
            Some(current) if current.message_id != chunk.message_id => {
                Some(ChunkError::MismatchedMessageId {
                    expected: current.message_id,
                    actual: chunk.message_id,
                })
            }
            Some(current) if current.chunk_count != chunk.chunk_count => {
                Some(ChunkError::MismatchedChunkCount {
                    expected: current.chunk_count,
                    actual: chunk.chunk_count,
                })
            }
            Some(current) if current.total_message_len != chunk.total_message_len => {
                Some(ChunkError::MismatchedMessageLength {
                    expected: current.total_message_len,
                    actual: chunk.total_message_len,
                })
            }
            Some(current) if current.next_chunk_index != chunk.chunk_index => {
                Some(ChunkError::OutOfOrderChunk {
                    expected: current.next_chunk_index,
                    actual: chunk.chunk_index,
                })
            }
            Some(_) => None,
            None => return Err(ChunkError::NonFirstChunkWithoutMessage),
        };
        if let Some(error) = validation_error {
            self.reset();
            return Err(error);
        }

        let current = self.partial.as_mut().expect("partial message exists");
        current.payload.extend_from_slice(&chunk.payload_fragment);
        current.next_chunk_index += 1;

        if current.payload.len() > current.total_message_len {
            let error = ChunkError::MessageLengthExceeded {
                len: current.payload.len(),
                expected: current.total_message_len,
            };
            self.reset();
            return Err(error);
        }

        if !chunk.is_last() {
            return Ok(None);
        }

        let completed = self.partial.take().expect("partial message exists");
        if completed.payload.len() != completed.total_message_len {
            return Err(ChunkError::CompletedLengthMismatch {
                len: completed.payload.len(),
                expected: completed.total_message_len,
            });
        }

        Ok(Some(completed.payload))
    }

    pub fn reset(&mut self) {
        self.partial = None;
    }

    pub fn has_partial_message(&self) -> bool {
        self.partial.is_some()
    }
}

pub fn encode_chunks(message_id: u32, payload: &[u8]) -> Result<Vec<Vec<u8>>, ChunkError> {
    encode_chunks_with_payload_size(message_id, payload, TARGET_CHUNK_PAYLOAD_BYTES)
}

pub fn encode_chunks_with_payload_size(
    message_id: u32,
    payload: &[u8],
    target_chunk_payload_bytes: usize,
) -> Result<Vec<Vec<u8>>, ChunkError> {
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(ChunkError::MessageTooLarge {
            len: payload.len(),
            max: MAX_MESSAGE_BYTES,
        });
    }
    if target_chunk_payload_bytes == 0 {
        return Err(ChunkError::InvalidPayloadSize);
    }

    let chunk_count = payload.len().div_ceil(target_chunk_payload_bytes).max(1);
    if chunk_count > u16::MAX as usize {
        return Err(ChunkError::TooManyChunks { count: chunk_count });
    }

    let mut chunks = Vec::with_capacity(chunk_count);
    for chunk_index in 0..chunk_count {
        let start = chunk_index * target_chunk_payload_bytes;
        let end = usize::min(start + target_chunk_payload_bytes, payload.len());
        let mut flags = 0u8;
        if chunk_index == 0 {
            flags |= FLAG_FIRST_CHUNK;
        }
        if chunk_index + 1 == chunk_count {
            flags |= FLAG_LAST_CHUNK;
        }

        let mut chunk = Vec::with_capacity(CHUNK_HEADER_BYTES + end.saturating_sub(start));
        chunk.extend_from_slice(&CHUNK_MAGIC);
        chunk.push(CHUNK_VERSION);
        chunk.push(flags);
        chunk.extend_from_slice(&message_id.to_le_bytes());
        chunk.extend_from_slice(&(chunk_index as u16).to_le_bytes());
        chunk.extend_from_slice(&(chunk_count as u16).to_le_bytes());
        chunk.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk.extend_from_slice(&payload[start..end]);
        chunks.push(chunk);
    }

    Ok(chunks)
}

pub fn parse_chunk(bytes: &[u8]) -> Result<Chunk, ChunkError> {
    if bytes.len() < CHUNK_HEADER_BYTES {
        return Err(ChunkError::ChunkTooShort { len: bytes.len() });
    }
    if bytes[0..2] != CHUNK_MAGIC {
        return Err(ChunkError::BadMagic);
    }
    if bytes[2] != CHUNK_VERSION {
        return Err(ChunkError::UnsupportedVersion { version: bytes[2] });
    }

    let flags = bytes[3];
    let unknown_flags = flags & !KNOWN_FLAGS;
    if unknown_flags != 0 {
        return Err(ChunkError::UnknownFlags {
            flags: unknown_flags,
        });
    }

    let message_id = u32::from_le_bytes(bytes[4..8].try_into().expect("slice length checked"));
    let chunk_index = u16::from_le_bytes(bytes[8..10].try_into().expect("slice length checked"));
    let chunk_count = u16::from_le_bytes(bytes[10..12].try_into().expect("slice length checked"));
    let total_message_len =
        u32::from_le_bytes(bytes[12..16].try_into().expect("slice length checked")) as usize;
    let payload_fragment = bytes[16..].to_vec();

    if chunk_count == 0 {
        return Err(ChunkError::ChunkCountZero);
    }
    if chunk_index >= chunk_count {
        return Err(ChunkError::ChunkIndexOutOfRange {
            chunk_index,
            chunk_count,
        });
    }
    if total_message_len > MAX_MESSAGE_BYTES {
        return Err(ChunkError::MessageTooLarge {
            len: total_message_len,
            max: MAX_MESSAGE_BYTES,
        });
    }
    if chunk_index == 0 && flags & FLAG_FIRST_CHUNK == 0 {
        return Err(ChunkError::MissingFirstChunkFlag);
    }
    if chunk_index != 0 && flags & FLAG_FIRST_CHUNK != 0 {
        return Err(ChunkError::UnexpectedFirstChunkFlag);
    }
    if chunk_index + 1 == chunk_count && flags & FLAG_LAST_CHUNK == 0 {
        return Err(ChunkError::MissingLastChunkFlag);
    }
    if chunk_index + 1 != chunk_count && flags & FLAG_LAST_CHUNK != 0 {
        return Err(ChunkError::UnexpectedLastChunkFlag);
    }

    Ok(Chunk {
        flags,
        message_id,
        chunk_index,
        chunk_count,
        total_message_len,
        payload_fragment,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkError {
    ChunkTooShort { len: usize },
    BadMagic,
    UnsupportedVersion { version: u8 },
    UnknownFlags { flags: u8 },
    ChunkCountZero,
    ChunkIndexOutOfRange { chunk_index: u16, chunk_count: u16 },
    MessageTooLarge { len: usize, max: usize },
    MissingFirstChunkFlag,
    UnexpectedFirstChunkFlag,
    MissingLastChunkFlag,
    UnexpectedLastChunkFlag,
    InvalidPayloadSize,
    TooManyChunks { count: usize },
    UnexpectedFirstChunk,
    NonFirstChunkWithoutMessage,
    MismatchedMessageId { expected: u32, actual: u32 },
    MismatchedChunkCount { expected: u16, actual: u16 },
    MismatchedMessageLength { expected: usize, actual: usize },
    OutOfOrderChunk { expected: u16, actual: u16 },
    MessageLengthExceeded { len: usize, expected: usize },
    CompletedLengthMismatch { len: usize, expected: usize },
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChunkTooShort { len } => write!(f, "chunk too short: {len} bytes"),
            Self::BadMagic => f.write_str("bad chunk magic"),
            Self::UnsupportedVersion { version } => {
                write!(f, "unsupported chunk version {version}")
            }
            Self::UnknownFlags { flags } => write!(f, "unknown chunk flags: 0x{flags:02x}"),
            Self::ChunkCountZero => f.write_str("chunk_count must be nonzero"),
            Self::ChunkIndexOutOfRange {
                chunk_index,
                chunk_count,
            } => write!(
                f,
                "chunk_index {chunk_index} must be less than chunk_count {chunk_count}"
            ),
            Self::MessageTooLarge { len, max } => write!(f, "message too large: {len} > {max}"),
            Self::MissingFirstChunkFlag => f.write_str("first chunk flag missing"),
            Self::UnexpectedFirstChunkFlag => {
                f.write_str("first chunk flag set on non-first chunk")
            }
            Self::MissingLastChunkFlag => f.write_str("last chunk flag missing"),
            Self::UnexpectedLastChunkFlag => f.write_str("last chunk flag set before final chunk"),
            Self::InvalidPayloadSize => f.write_str("target chunk payload size must be nonzero"),
            Self::TooManyChunks { count } => write!(f, "too many chunks: {count}"),
            Self::UnexpectedFirstChunk => {
                f.write_str("received first chunk while another message is incomplete")
            }
            Self::NonFirstChunkWithoutMessage => {
                f.write_str("received non-first chunk without active message")
            }
            Self::MismatchedMessageId { expected, actual } => {
                write!(
                    f,
                    "mismatched message_id: expected {expected}, got {actual}"
                )
            }
            Self::MismatchedChunkCount { expected, actual } => {
                write!(
                    f,
                    "mismatched chunk_count: expected {expected}, got {actual}"
                )
            }
            Self::MismatchedMessageLength { expected, actual } => write!(
                f,
                "mismatched total_message_len: expected {expected}, got {actual}"
            ),
            Self::OutOfOrderChunk { expected, actual } => {
                write!(f, "out-of-order chunk: expected {expected}, got {actual}")
            }
            Self::MessageLengthExceeded { len, expected } => {
                write!(
                    f,
                    "partial message length {len} exceeded expected {expected}"
                )
            }
            Self::CompletedLengthMismatch { len, expected } => {
                write!(
                    f,
                    "completed message length {len} did not match expected {expected}"
                )
            }
        }
    }
}

impl Error for ChunkError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_small_message_as_single_chunk() {
        let chunks = encode_chunks(12, b"hello").expect("encodes");
        assert_eq!(chunks.len(), 1);

        let chunk = parse_chunk(&chunks[0]).expect("parses");
        assert_eq!(chunk.flags, FLAG_FIRST_CHUNK | FLAG_LAST_CHUNK);
        assert_eq!(chunk.message_id, 12);
        assert_eq!(chunk.chunk_index, 0);
        assert_eq!(chunk.chunk_count, 1);
        assert_eq!(chunk.total_message_len, 5);
        assert_eq!(chunk.payload_fragment, b"hello");
    }

    #[test]
    fn encodes_large_message_as_multiple_chunks() {
        let payload = vec![0xaa; TARGET_CHUNK_PAYLOAD_BYTES + 7];
        let chunks = encode_chunks(99, &payload).expect("encodes");
        assert_eq!(chunks.len(), 2);

        let first = parse_chunk(&chunks[0]).expect("first parses");
        let second = parse_chunk(&chunks[1]).expect("second parses");

        assert!(first.is_first());
        assert!(!first.is_last());
        assert!(!second.is_first());
        assert!(second.is_last());
        assert_eq!(first.chunk_count, 2);
        assert_eq!(second.chunk_count, 2);
    }

    #[test]
    fn reassembles_multi_chunk_message() {
        let payload = b"this message is deliberately split into tiny chunks";
        let chunks = encode_chunks_with_payload_size(3, payload, 7).expect("encodes");
        let mut reassembler = ChunkReassembler::new();
        let mut completed = None;

        for chunk in chunks {
            completed = reassembler.push_bytes(&chunk).expect("chunk accepted");
        }

        assert_eq!(completed, Some(payload.to_vec()));
        assert!(!reassembler.has_partial_message());
    }

    #[test]
    fn reassembler_reports_incomplete_until_final_chunk() {
        let chunks = encode_chunks_with_payload_size(3, b"abcdef", 3).expect("encodes");
        let mut reassembler = ChunkReassembler::new();

        assert_eq!(
            reassembler.push_bytes(&chunks[0]).expect("chunk accepted"),
            None
        );
        assert!(reassembler.has_partial_message());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut chunk = encode_chunks(1, b"hello").expect("encodes").remove(0);
        chunk[0] = b'X';

        assert_eq!(parse_chunk(&chunk), Err(ChunkError::BadMagic));
    }

    #[test]
    fn rejects_oversized_message_on_encode() {
        let payload = vec![0; MAX_MESSAGE_BYTES + 1];

        assert_eq!(
            encode_chunks(1, &payload),
            Err(ChunkError::MessageTooLarge {
                len: MAX_MESSAGE_BYTES + 1,
                max: MAX_MESSAGE_BYTES
            })
        );
    }

    #[test]
    fn rejects_out_of_order_chunks() {
        let chunks = encode_chunks_with_payload_size(1, b"abcdefgh", 2).expect("encodes");
        let mut reassembler = ChunkReassembler::new();

        let error = reassembler
            .push_bytes(&chunks[1])
            .expect_err("second chunk cannot start a message");

        assert_eq!(error, ChunkError::NonFirstChunkWithoutMessage);
    }

    #[test]
    fn rejects_missing_middle_chunk() {
        let chunks = encode_chunks_with_payload_size(1, b"abcdefgh", 2).expect("encodes");
        let mut reassembler = ChunkReassembler::new();

        assert_eq!(
            reassembler.push_bytes(&chunks[0]).expect("first accepted"),
            None
        );
        let error = reassembler
            .push_bytes(&chunks[2])
            .expect_err("third chunk cannot follow first");

        assert_eq!(
            error,
            ChunkError::OutOfOrderChunk {
                expected: 1,
                actual: 2
            }
        );
    }

    #[test]
    fn rejects_missing_last_flag() {
        let mut chunk = encode_chunks(1, b"hello").expect("encodes").remove(0);
        chunk[3] &= !FLAG_LAST_CHUNK;

        assert_eq!(parse_chunk(&chunk), Err(ChunkError::MissingLastChunkFlag));
    }
}
