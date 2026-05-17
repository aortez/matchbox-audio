package dev.matchbox.audio

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class NowPlayingScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun readyStateShowsNowPlayingSnapshot() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.ready(FakeSnapshots.nowPlaying),
                onRefresh = {},
            )
        }

        compose.onNodeWithTag("now-playing").assertIsDisplayed()
        compose.onNodeWithText("Speak to Me").assertIsDisplayed()
        compose.onNodeWithText("Pink Floyd").assertIsDisplayed()
        compose.onNodeWithText("3 / 12").assertIsDisplayed()
    }

    @Test
    fun readyStateShowsPlaybackControls() {
        var previousClicks = 0
        var pauseClicks = 0
        var nextClicks = 0
        var stopClicks = 0

        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.ready(FakeSnapshots.nowPlaying),
                onRefresh = {},
                onPrevious = { previousClicks += 1 },
                onPause = { pauseClicks += 1 },
                onNext = { nextClicks += 1 },
                onStop = { stopClicks += 1 },
            )
        }

        compose.onNodeWithText("Previous").performClick()
        compose.onNodeWithText("Pause").performClick()
        compose.onNodeWithText("Next").performClick()
        compose.onNodeWithText("Stop").performClick()

        assertEquals(1, previousClicks)
        assertEquals(1, pauseClicks)
        assertEquals(1, nextClicks)
        assertEquals(1, stopClicks)
    }

    @Test
    fun pausedStateShowsPlayControl() {
        var playClicks = 0
        val pausedSnapshot = FakeSnapshots.nowPlaying.copy(
            playback = FakeSnapshots.nowPlaying.playback.copy(state = "pause"),
        )

        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.ready(pausedSnapshot),
                onRefresh = {},
                onPlay = { playClicks += 1 },
            )
        }

        compose.onNodeWithText("Play").performClick()

        assertEquals(1, playClicks)
    }

    @Test
    fun loadingStateShowsLoadingText() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState(),
                onRefresh = {},
            )
        }

        compose.onNodeWithTag("loading").assertIsDisplayed()
    }

    @Test
    fun bleStateShowsConnectionStatus() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.ready(FakeSnapshots.nowPlaying),
                onRefresh = {},
                usingBle = true,
                bleConnectionState = BleConnectionState(
                    phase = BleConnectionPhase.Scanning,
                ),
            )
        }

        compose.onNodeWithTag("connection-status").assertIsDisplayed()
        compose.onAllNodesWithText("Scanning").assertCountEquals(2)
        compose.onNodeWithText("Demo").assertIsDisplayed()
    }

    @Test
    fun reconnectingStateShowsConnectionStatus() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.ready(FakeSnapshots.nowPlaying),
                onRefresh = {},
                usingBle = true,
                bleConnectionState = BleConnectionState(
                    phase = BleConnectionPhase.Reconnecting,
                    deviceName = "Matchbox Audio",
                ),
            )
        }

        compose.onNodeWithTag("connection-status").assertIsDisplayed()
        compose.onAllNodesWithText("Reconnecting").assertCountEquals(2)
        compose.onAllNodesWithText("Matchbox Audio").assertCountEquals(2)
    }

    @Test
    fun reconnectingStateShowsRetryDetail() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.failed("BLE connection timed out"),
                onRefresh = {},
                usingBle = true,
                bleConnectionState = BleConnectionState(
                    phase = BleConnectionPhase.Reconnecting,
                    errorMessage = "Retrying BLE in 1s: No Matchbox BLE device found",
                ),
            )
        }

        compose.onNodeWithTag("connection-status").assertIsDisplayed()
        compose.onAllNodesWithText("Reconnecting").assertCountEquals(2)
        compose.onNodeWithText("Retrying BLE in 1s: No Matchbox BLE device found").assertIsDisplayed()
    }

    @Test
    fun busyStateShowsDeviceBusyStatus() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.failed("Another app is connected"),
                onRefresh = {},
                usingBle = true,
                bleConnectionState = BleConnectionState(
                    phase = BleConnectionPhase.Busy,
                    errorMessage = "Another app is connected",
                ),
            )
        }

        compose.onNodeWithTag("connection-status").assertIsDisplayed()
        compose.onAllNodesWithText("Device busy").assertCountEquals(2)
        compose.onAllNodesWithText("Another app is connected").assertCountEquals(2)
    }

    @Test
    fun authRequiredStateShowsAuthorizationStatus() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.failed("Open pairing mode on Matchbox Audio"),
                onRefresh = {},
                usingBle = true,
                bleConnectionState = BleConnectionState(
                    phase = BleConnectionPhase.AuthRequired,
                    errorMessage = "Open pairing mode on Matchbox Audio",
                ),
            )
        }

        compose.onNodeWithTag("connection-status").assertIsDisplayed()
        compose.onAllNodesWithText("Authorization required").assertCountEquals(2)
        compose.onAllNodesWithText("Open pairing mode on Matchbox Audio").assertCountEquals(2)
    }

    @Test
    fun permissionDeniedShowsBluetoothStatus() {
        compose.setContent {
            NowPlayingScreen(
                state = NowPlayingUiState.ready(FakeSnapshots.nowPlaying),
                onRefresh = {},
                permissionDenied = true,
            )
        }

        compose.onNodeWithTag("permission-denied").assertIsDisplayed()
        compose.onNodeWithText("Bluetooth permission denied").assertIsDisplayed()
    }
}
