package dev.matchbox.audio

import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NowPlayingViewModelTest {
    @Test
    fun loadSnapshotMapsTransportResultIntoUiState() = runTest {
        val viewModel = NowPlayingViewModel(FakeMatchboxTransport(FakeSnapshots.nowPlaying))

        viewModel.loadSnapshot()

        val state = viewModel.uiState
        assertFalse(state.loading)
        assertNull(state.error)
        assertEquals("ready", state.device?.serviceState)
        assertEquals("Speak to Me", state.device?.playback?.track?.displayTitle)
        assertEquals(65, state.device?.playback?.volume)
    }

    @Test
    fun loadSnapshotMapsTransportFailureIntoErrorState() = runTest {
        val viewModel = NowPlayingViewModel(
            object : MatchboxTransport {
                override suspend fun requestSnapshot(): DeviceSnapshot {
                    error("offline")
                }

                override suspend fun sendPlaybackCommand(command: PlaybackCommand) {
                    error("offline")
                }

                override suspend fun setVolume(level: Int) {
                    error("offline")
                }
            },
        )

        viewModel.loadSnapshot()

        val state = viewModel.uiState
        assertFalse(state.loading)
        assertEquals("offline", state.error)
        assertTrue(state.device == null)
    }

    @Test
    fun sendPlaybackCommandSendsCommandAndRefreshesSnapshot() = runTest {
        val transport = FakeMatchboxTransport(FakeSnapshots.nowPlaying)
        val viewModel = NowPlayingViewModel(transport)

        viewModel.sendPlaybackCommand(PlaybackCommand.Pause)

        assertEquals(listOf(PlaybackCommand.Pause), transport.playbackCommands)
        val state = viewModel.uiState
        assertFalse(state.loading)
        assertNull(state.error)
        assertEquals("pause", state.device?.playback?.state)
    }

    @Test
    fun sendVolumeSetsLevelAndRefreshesSnapshot() = runTest {
        val transport = FakeMatchboxTransport(FakeSnapshots.nowPlaying)
        val viewModel = NowPlayingViewModel(transport)

        viewModel.sendVolume(42)

        assertEquals(listOf(42), transport.volumeLevels)
        val state = viewModel.uiState
        assertFalse(state.loading)
        assertNull(state.error)
        assertEquals(42, state.device?.playback?.volume)
    }

    @Test
    fun sendVolumeClampsLevel() = runTest {
        val transport = FakeMatchboxTransport(FakeSnapshots.nowPlaying)
        val viewModel = NowPlayingViewModel(transport)

        viewModel.sendVolume(101)

        assertEquals(listOf(100), transport.volumeLevels)
        assertEquals(100, viewModel.uiState.device?.playback?.volume)
    }

    @Test
    fun sendPlaybackCommandMapsFailureIntoErrorState() = runTest {
        val viewModel = NowPlayingViewModel(
            object : MatchboxTransport {
                override suspend fun requestSnapshot(): DeviceSnapshot = FakeSnapshots.nowPlaying

                override suspend fun sendPlaybackCommand(command: PlaybackCommand) {
                    error("command failed")
                }

                override suspend fun setVolume(level: Int) {
                    error("command failed")
                }
            },
        )

        viewModel.sendPlaybackCommand(PlaybackCommand.Next)

        val state = viewModel.uiState
        assertFalse(state.loading)
        assertEquals("command failed", state.error)
        assertTrue(state.device == null)
    }

    @Test
    fun sendVolumeMapsFailureIntoErrorState() = runTest {
        val viewModel = NowPlayingViewModel(
            object : MatchboxTransport {
                override suspend fun requestSnapshot(): DeviceSnapshot = FakeSnapshots.nowPlaying

                override suspend fun sendPlaybackCommand(command: PlaybackCommand) {
                    error("command failed")
                }

                override suspend fun setVolume(level: Int) {
                    error("volume failed")
                }
            },
        )

        viewModel.sendVolume(42)

        val state = viewModel.uiState
        assertFalse(state.loading)
        assertEquals("volume failed", state.error)
        assertTrue(state.device == null)
    }
}
