package dev.matchbox.audio

import org.json.JSONObject

data class BleProtocolErrorDetails(
    val code: String?,
    val message: String,
)

object BleProtocolMessages {
    const val ERROR_AUTH_REQUIRED = "auth_required"
    const val ERROR_BUSY = "busy"

    fun errorFromResponse(root: JSONObject): BleProtocolErrorDetails? {
        if (root.optString("type") != "response" || root.optBoolean("ok", true)) {
            return null
        }

        val error = root.optJSONObject("error")
            ?: return BleProtocolErrorDetails(
                code = null,
                message = "BLE protocol request failed",
            )
        val code = error.optString("code").takeIf { it.isNotBlank() }
        val message = error.optString("message").takeIf { it.isNotBlank() }
        val displayMessage = listOfNotNull(code, message)
            .joinToString(": ")
            .ifBlank { "BLE protocol request failed" }

        return BleProtocolErrorDetails(
            code = code,
            message = displayMessage,
        )
    }

    fun userFacingMessage(error: BleProtocolErrorDetails): String =
        when (error.code) {
            ERROR_AUTH_REQUIRED -> "Open pairing mode on Matchbox Audio"
            ERROR_BUSY -> "Another app is connected"
            else -> error.message
        }
}
