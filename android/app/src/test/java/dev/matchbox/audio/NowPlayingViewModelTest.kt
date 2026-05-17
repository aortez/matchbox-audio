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
            },
        )

        viewModel.loadSnapshot()

        val state = viewModel.uiState
        assertFalse(state.loading)
        assertEquals("offline", state.error)
        assertTrue(state.device == null)
    }
}
