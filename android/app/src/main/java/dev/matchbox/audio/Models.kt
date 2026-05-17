package dev.matchbox.audio

data class Track(
    val uri: String,
    val title: String?,
    val artist: String?,
    val album: String?,
    val durationSeconds: Int?,
    val elapsedSeconds: Int?,
) {
    val displayTitle: String = title ?: uri.substringAfterLast('/').ifBlank { uri }
}

data class PlaybackSnapshot(
    val state: String,
    val volume: Int,
    val queuePosition: Int?,
    val queueLength: Int,
    val track: Track?,
)

data class DeviceSnapshot(
    val serviceState: String,
    val networkMode: String?,
    val activeConnection: String?,
    val playback: PlaybackSnapshot,
)

data class NowPlayingUiState(
    val loading: Boolean = true,
    val error: String? = null,
    val device: DeviceSnapshot? = null,
) {
    companion object {
        fun ready(device: DeviceSnapshot): NowPlayingUiState =
            NowPlayingUiState(loading = false, device = device)

        fun failed(message: String): NowPlayingUiState =
            NowPlayingUiState(loading = false, error = message)
    }
}
