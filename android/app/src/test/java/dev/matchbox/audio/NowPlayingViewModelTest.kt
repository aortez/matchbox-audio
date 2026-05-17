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
    fun sendPlaybackCommandMapsFailureIntoErrorState() = runTest {
        val viewModel = NowPlayingViewModel(
            object : MatchboxTransport {
                override suspend fun requestSnapshot(): DeviceSnapshot = FakeSnapshots.nowPlaying

                override suspend fun sendPlaybackCommand(command: PlaybackCommand) {
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
}
