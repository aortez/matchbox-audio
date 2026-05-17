package dev.matchbox.audio

interface MatchboxTransport {
    suspend fun requestSnapshot(): DeviceSnapshot
    suspend fun sendPlaybackCommand(command: PlaybackCommand)
    suspend fun setVolume(level: Int)
}

class FakeMatchboxTransport(
    snapshot: DeviceSnapshot = FakeSnapshots.nowPlaying,
) : MatchboxTransport {
    private var snapshot: DeviceSnapshot = snapshot

    val playbackCommands = mutableListOf<PlaybackCommand>()
    val volumeLevels = mutableListOf<Int>()

    override suspend fun requestSnapshot(): DeviceSnapshot = snapshot

    override suspend fun sendPlaybackCommand(command: PlaybackCommand) {
        playbackCommands.add(command)
        snapshot = snapshot.copy(
            playback = snapshot.playback.copy(
                state = when (command) {
                    PlaybackCommand.Play -> "play"
                    PlaybackCommand.Pause -> "pause"
                    PlaybackCommand.Toggle ->
                        if (snapshot.playback.state == "play") "pause" else "play"
                    PlaybackCommand.Stop -> "stop"
                    PlaybackCommand.Next,
                    PlaybackCommand.Previous,
                    -> snapshot.playback.state
                },
            ),
        )
    }

    override suspend fun setVolume(level: Int) {
        require(level in 0..100) { "level must be between 0 and 100" }
        volumeLevels.add(level)
        snapshot = snapshot.copy(
            playback = snapshot.playback.copy(volume = level),
        )
    }
}

enum class PlaybackCommand(val method: String) {
    Play("playback.play"),
    Pause("playback.pause"),
    Toggle("playback.toggle"),
    Stop("playback.stop"),
    Next("playback.next"),
    Previous("playback.previous"),
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
