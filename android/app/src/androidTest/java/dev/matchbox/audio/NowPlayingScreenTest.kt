package dev.matchbox.audio

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
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
