package dev.matchbox.audio

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BleProtocolMessagesTest {
    @Test
    fun extractsBusyErrorResponse() {
        val root = JSONObject(
            """
            {
              "type": "response",
              "id": 0,
              "ok": false,
              "error": {
                "code": "busy",
                "message": "client 01:02:03:04:05:06 is already connected"
              }
            }
            """.trimIndent(),
        )

        val error = BleProtocolMessages.errorFromResponse(root)

        assertEquals("busy", error?.code)
        assertEquals(
            "busy: client 01:02:03:04:05:06 is already connected",
            error?.message,
        )
        assertEquals("Another app is connected", error?.let(BleProtocolMessages::userFacingMessage))
    }

    @Test
    fun extractsAuthRequiredErrorResponse() {
        val root = JSONObject(
            """
            {
              "type": "response",
              "id": 2,
              "ok": false,
              "error": {
                "code": "auth_required",
                "message": "pairing mode is required"
              }
            }
            """.trimIndent(),
        )

        val error = BleProtocolMessages.errorFromResponse(root)

        assertEquals("auth_required", error?.code)
        assertEquals("auth_required: pairing mode is required", error?.message)
        assertEquals(
            "Open pairing mode on Matchbox Audio",
            error?.let(BleProtocolMessages::userFacingMessage),
        )
    }

    @Test
    fun ignoresSuccessfulResponses() {
        val root = JSONObject("""{"type":"response","id":1,"ok":true,"result":{}}""")

        assertNull(BleProtocolMessages.errorFromResponse(root))
    }
}
