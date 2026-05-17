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
