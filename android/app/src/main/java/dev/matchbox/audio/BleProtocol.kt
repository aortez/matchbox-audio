package dev.matchbox.audio

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

object BleProtocol {
    val SERVICE_UUID: UUID = UUID.fromString("1cef04f1-966e-43ad-860f-086db4f277d6")
    val STATUS_UUID: UUID = UUID.fromString("bd539314-4637-416b-a3b5-804fecd5b792")
    val RX_UUID: UUID = UUID.fromString("fbf39e22-bb07-49bf-bfa0-3dbdfc47769b")
    val TX_UUID: UUID = UUID.fromString("fcc9055c-34e3-46d9-a010-bd8a4f180b0c")
    val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

    const val APP_PROTOCOL_VERSION = 1
    const val MAX_MESSAGE_BYTES = 16 * 1024
    const val CHUNK_VERSION = 1
    const val FLAG_FIRST_CHUNK = 0x01
    const val FLAG_LAST_CHUNK = 0x02
    const val KNOWN_FLAGS = FLAG_FIRST_CHUNK or FLAG_LAST_CHUNK
    const val CHUNK_HEADER_BYTES = 16
    const val TARGET_GATT_VALUE_BYTES = 244
    const val TARGET_CHUNK_PAYLOAD_BYTES = TARGET_GATT_VALUE_BYTES - CHUNK_HEADER_BYTES
    const val REQUESTED_MTU = 517
    const val SCAN_TIMEOUT_MILLIS = 10_000L
    const val RECONNECT_TIMEOUT_MILLIS = 6_000L
    const val CONNECT_TIMEOUT_MILLIS = 30_000L
    const val RESPONSE_TIMEOUT_MILLIS = 10_000L

    internal const val CHUNK_MAGIC_0 = 'M'.code.toByte()
    internal const val CHUNK_MAGIC_1 = 'B'.code.toByte()
}

data class BleChunk(
    val flags: Int,
    val messageId: Long,
    val chunkIndex: Int,
    val chunkCount: Int,
    val totalMessageLength: Int,
    val payloadFragment: ByteArray,
) {
    val isFirst: Boolean
        get() = flags and BleProtocol.FLAG_FIRST_CHUNK != 0

    val isLast: Boolean
        get() = flags and BleProtocol.FLAG_LAST_CHUNK != 0
}

class BleChunkException(message: String) : IllegalArgumentException(message)

object BleChunkCodec {
    fun encode(
        messageId: Long,
        payload: ByteArray,
        targetChunkPayloadBytes: Int = BleProtocol.TARGET_CHUNK_PAYLOAD_BYTES,
    ): List<ByteArray> {
        if (messageId !in 0..0xffff_ffffL) {
            throw BleChunkException("message_id out of u32 range: $messageId")
        }
        if (payload.size > BleProtocol.MAX_MESSAGE_BYTES) {
            throw BleChunkException(
                "message too large: ${payload.size} > ${BleProtocol.MAX_MESSAGE_BYTES}",
            )
        }
        if (targetChunkPayloadBytes <= 0) {
            throw BleChunkException("target chunk payload size must be nonzero")
        }

        val chunkCount = maxOf(1, ceilDiv(payload.size, targetChunkPayloadBytes))
        if (chunkCount > 0xffff) {
            throw BleChunkException("too many chunks: $chunkCount")
        }

        return List(chunkCount) { chunkIndex ->
            val start = chunkIndex * targetChunkPayloadBytes
            val end = minOf(start + targetChunkPayloadBytes, payload.size)
            var flags = 0
            if (chunkIndex == 0) {
                flags = flags or BleProtocol.FLAG_FIRST_CHUNK
            }
            if (chunkIndex + 1 == chunkCount) {
                flags = flags or BleProtocol.FLAG_LAST_CHUNK
            }

            ByteBuffer
                .allocate(BleProtocol.CHUNK_HEADER_BYTES + end - start)
                .order(ByteOrder.LITTLE_ENDIAN)
                .put(BleProtocol.CHUNK_MAGIC_0)
                .put(BleProtocol.CHUNK_MAGIC_1)
                .put(BleProtocol.CHUNK_VERSION.toByte())
                .put(flags.toByte())
                .putInt(messageId.toInt())
                .putShort(chunkIndex.toShort())
                .putShort(chunkCount.toShort())
                .putInt(payload.size)
                .put(payload, start, end - start)
                .array()
        }
    }

    fun parse(bytes: ByteArray): BleChunk {
        if (bytes.size < BleProtocol.CHUNK_HEADER_BYTES) {
            throw BleChunkException("chunk too short: ${bytes.size} bytes")
        }
        if (bytes[0] != BleProtocol.CHUNK_MAGIC_0 || bytes[1] != BleProtocol.CHUNK_MAGIC_1) {
            throw BleChunkException("bad chunk magic")
        }

        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        buffer.position(2)
        val version = buffer.get().toInt() and 0xff
        if (version != BleProtocol.CHUNK_VERSION) {
            throw BleChunkException("unsupported chunk version $version")
        }

        val flags = buffer.get().toInt() and 0xff
        val unknownFlags = flags and BleProtocol.KNOWN_FLAGS.inv()
        if (unknownFlags != 0) {
            throw BleChunkException("unknown chunk flags: 0x${unknownFlags.toString(16)}")
        }

        val messageId = buffer.int.toLong() and 0xffff_ffffL
        val chunkIndex = buffer.short.toInt() and 0xffff
        val chunkCount = buffer.short.toInt() and 0xffff
        val totalMessageLength = buffer.int.toLong() and 0xffff_ffffL
        val payloadFragment = ByteArray(buffer.remaining())
        buffer.get(payloadFragment)

        if (chunkCount == 0) {
            throw BleChunkException("chunk_count must be nonzero")
        }
        if (chunkIndex >= chunkCount) {
            throw BleChunkException(
                "chunk_index $chunkIndex must be less than chunk_count $chunkCount",
            )
        }
        if (totalMessageLength > BleProtocol.MAX_MESSAGE_BYTES) {
            throw BleChunkException(
                "message too large: $totalMessageLength > ${BleProtocol.MAX_MESSAGE_BYTES}",
            )
        }
        if (chunkIndex == 0 && flags and BleProtocol.FLAG_FIRST_CHUNK == 0) {
            throw BleChunkException("first chunk flag missing")
        }
        if (chunkIndex != 0 && flags and BleProtocol.FLAG_FIRST_CHUNK != 0) {
            throw BleChunkException("first chunk flag set on non-first chunk")
        }
        if (chunkIndex + 1 == chunkCount && flags and BleProtocol.FLAG_LAST_CHUNK == 0) {
            throw BleChunkException("last chunk flag missing")
        }
        if (chunkIndex + 1 != chunkCount && flags and BleProtocol.FLAG_LAST_CHUNK != 0) {
            throw BleChunkException("last chunk flag set before final chunk")
        }

        return BleChunk(
            flags = flags,
            messageId = messageId,
            chunkIndex = chunkIndex,
            chunkCount = chunkCount,
            totalMessageLength = totalMessageLength.toInt(),
            payloadFragment = payloadFragment,
        )
    }

    private fun ceilDiv(value: Int, divisor: Int): Int =
        if (value == 0) 0 else ((value - 1) / divisor) + 1
}

class BleChunkReassembler {
    private var partial: PartialMessage? = null

    fun push(bytes: ByteArray): ByteArray? {
        val chunk = try {
            BleChunkCodec.parse(bytes)
        } catch (error: BleChunkException) {
            reset()
            throw error
        }
        return push(chunk)
    }

    fun push(chunk: BleChunk): ByteArray? {
        if (chunk.chunkIndex == 0) {
            if (partial != null) {
                reset()
                throw BleChunkException("received first chunk while another message is incomplete")
            }
            partial = PartialMessage(
                messageId = chunk.messageId,
                nextChunkIndex = 0,
                chunkCount = chunk.chunkCount,
                totalMessageLength = chunk.totalMessageLength,
            )
        }

        val current = partial ?: throw BleChunkException("received non-first chunk without active message")
        when {
            current.messageId != chunk.messageId -> {
                reset()
                throw BleChunkException(
                    "mismatched message_id: expected ${current.messageId}, got ${chunk.messageId}",
                )
            }

            current.chunkCount != chunk.chunkCount -> {
                reset()
                throw BleChunkException(
                    "mismatched chunk_count: expected ${current.chunkCount}, got ${chunk.chunkCount}",
                )
            }

            current.totalMessageLength != chunk.totalMessageLength -> {
                reset()
                throw BleChunkException(
                    "mismatched total_message_len: expected ${current.totalMessageLength}, got ${chunk.totalMessageLength}",
                )
            }

            current.nextChunkIndex != chunk.chunkIndex -> {
                reset()
                throw BleChunkException(
                    "out-of-order chunk: expected ${current.nextChunkIndex}, got ${chunk.chunkIndex}",
                )
            }
        }

        current.payload.addAll(chunk.payloadFragment.asIterable())
        current.nextChunkIndex += 1
        if (current.payload.size > current.totalMessageLength) {
            val len = current.payload.size
            val expected = current.totalMessageLength
            reset()
            throw BleChunkException("partial message length $len exceeded expected $expected")
        }

        if (!chunk.isLast) {
            return null
        }

        val completed = partial ?: throw BleChunkException("received final chunk without active message")
        partial = null
        if (completed.payload.size != completed.totalMessageLength) {
            throw BleChunkException(
                "completed message length ${completed.payload.size} did not match expected ${completed.totalMessageLength}",
            )
        }

        return completed.payload.toByteArray()
    }

    fun reset() {
        partial = null
    }

    fun hasPartialMessage(): Boolean = partial != null

    private data class PartialMessage(
        val messageId: Long,
        var nextChunkIndex: Int,
        val chunkCount: Int,
        val totalMessageLength: Int,
        val payload: MutableList<Byte> = ArrayList(totalMessageLength),
    )
}
