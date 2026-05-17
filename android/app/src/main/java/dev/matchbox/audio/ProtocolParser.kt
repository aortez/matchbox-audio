package dev.matchbox.audio

import org.json.JSONObject

object ProtocolParser {
    fun parseSnapshotResponse(json: String): DeviceSnapshot {
        val root = JSONObject(json)
        require(root.optString("type") == "response") { "expected response envelope" }
        require(root.optBoolean("ok")) { "snapshot response was not ok" }

        val status = root
            .getJSONObject("result")
            .getJSONObject("status")
        val service = status.getJSONObject("service")
        val network = status.optJSONObject("network")
        val playback = status.getJSONObject("playback")
        val track = playback.optJSONObject("track")?.let(::parseTrack)

        return DeviceSnapshot(
            serviceState = service.getString("state"),
            networkMode = network?.optStringOrNull("mode"),
            activeConnection = network?.optStringOrNull("active_connection"),
            playback = PlaybackSnapshot(
                state = playback.getString("state"),
                volume = playback.getInt("volume"),
                queuePosition = playback.optIntOrNull("queue_position"),
                queueLength = playback.optInt("queue_length", 0),
                track = track,
            ),
        )
    }

    private fun parseTrack(track: JSONObject): Track =
        Track(
            uri = track.getString("uri"),
            title = track.optStringOrNull("title"),
            artist = track.optStringOrNull("artist"),
            album = track.optStringOrNull("album"),
            durationSeconds = track.optIntOrNull("duration_s"),
            elapsedSeconds = track.optIntOrNull("elapsed_s"),
        )
}

private fun JSONObject.optStringOrNull(name: String): String? =
    if (has(name) && !isNull(name)) optString(name) else null

private fun JSONObject.optIntOrNull(name: String): Int? =
    if (has(name) && !isNull(name)) optInt(name) else null
