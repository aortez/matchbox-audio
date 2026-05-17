package dev.matchbox.audio

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test
import java.nio.charset.StandardCharsets

class BleChunkCodecTest {
    @Test
    fun encodesSmallMessageAsSingleChunk() {
        val chunks = BleChunkCodec.encode(12, "hello".toByteArray(StandardCharsets.UTF_8))

        assertEquals(1, chunks.size)
        assertArrayEquals(
            byteArrayOf(
                'M'.code.toByte(),
                'B'.code.toByte(),
                1,
                3,
                12,
                0,
                0,
                0,
                0,
                0,
                1,
                0,
                5,
                0,
                0,
                0,
                'h'.code.toByte(),
                'e'.code.toByte(),
                'l'.code.toByte(),
                'l'.code.toByte(),
                'o'.code.toByte(),
            ),
            chunks.single(),
        )
        val chunk = BleChunkCodec.parse(chunks.single())
        assertEquals(BleProtocol.FLAG_FIRST_CHUNK or BleProtocol.FLAG_LAST_CHUNK, chunk.flags)
        assertEquals(12L, chunk.messageId)
        assertEquals(0, chunk.chunkIndex)
        assertEquals(1, chunk.chunkCount)
        assertEquals(5, chunk.totalMessageLength)
        assertArrayEquals("hello".toByteArray(StandardCharsets.UTF_8), chunk.payloadFragment)
    }

    @Test
    fun reassemblesMultiChunkMessage() {
        val payload = ByteArray(BleProtocol.TARGET_CHUNK_PAYLOAD_BYTES + 7) {
            (it and 0xff).toByte()
        }
        val chunks = BleChunkCodec.encode(
            messageId = 99,
            payload = payload,
            targetChunkPayloadBytes = 13,
        )
        val reassembler = BleChunkReassembler()
        var completed: ByteArray? = null

        chunks.forEach { chunk ->
            completed = reassembler.push(chunk)
        }

        assertArrayEquals(payload, completed)
        assertFalse(reassembler.hasPartialMessage())
    }

    @Test
    fun roundTripsSharedHelloFixture() {
        val json = requireNotNull(
            javaClass.classLoader?.getResource("v1/system_hello_request.json"),
        ).readText()
        val payload = json.toByteArray(StandardCharsets.UTF_8)
        val reassembler = BleChunkReassembler()
        var completed: ByteArray? = null

        BleChunkCodec.encode(1, payload).forEach { chunk ->
            completed = reassembler.push(chunk)
        }

        assertEquals(json, completed?.toString(StandardCharsets.UTF_8))
    }

    @Test
    fun rejectsOutOfOrderChunkAndResetsPartialMessage() {
        val payload = "split across several chunks".toByteArray(StandardCharsets.UTF_8)
        val chunks = BleChunkCodec.encode(
            messageId = 7,
            payload = payload,
            targetChunkPayloadBytes = 5,
        )
        val reassembler = BleChunkReassembler()

        reassembler.push(chunks[0])
        assertThrows(BleChunkException::class.java) {
            reassembler.push(chunks[2])
        }

        assertFalse(reassembler.hasPartialMessage())
    }

    @Test
    fun rejectsOversizedMessageBeforeChunking() {
        val payload = ByteArray(BleProtocol.MAX_MESSAGE_BYTES + 1)

        assertThrows(BleChunkException::class.java) {
            BleChunkCodec.encode(1, payload)
        }
    }

    @Test
    fun rejectsUnknownChunkFlags() {
        val chunk = BleChunkCodec.encode(1, "{}".toByteArray(StandardCharsets.UTF_8)).single()
        chunk[3] = (chunk[3].toInt() or 0x04).toByte()

        assertThrows(BleChunkException::class.java) {
            BleChunkCodec.parse(chunk)
        }
    }
}
