package dev.matchbox.audio

interface MatchboxTransport {
    suspend fun requestSnapshot(): DeviceSnapshot
}

class FakeMatchboxTransport(
    private val snapshot: DeviceSnapshot = FakeSnapshots.nowPlaying,
) : MatchboxTransport {
    override suspend fun requestSnapshot(): DeviceSnapshot = snapshot
}

object FakeSnapshots {
    val nowPlaying = DeviceSnapshot(
        serviceState = "ready",
        networkMode = "car",
        activeConnection = "matchbox-car-hotspot",
        playback = PlaybackSnapshot(
            state = "play",
            volume = 65,
            queuePosition = 2,
            queueLength = 12,
            track = Track(
                uri = "Pink Floyd/Dark Side/01 Speak to Me.flac",
                title = "Speak to Me",
                artist = "Pink Floyd",
                album = "Dark Side",
                durationSeconds = 91,
                elapsedSeconds = 12,
            ),
        ),
    )
}
