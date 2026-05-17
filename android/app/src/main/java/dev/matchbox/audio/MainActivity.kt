package dev.matchbox.audio

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MatchboxApp()
        }
    }
}

@Composable
fun MatchboxApp() {
    MaterialTheme {
        val viewModel = remember { NowPlayingViewModel() }
        LaunchedEffect(viewModel) {
            viewModel.loadSnapshot()
        }

        Surface(modifier = Modifier.fillMaxSize()) {
            NowPlayingScreen(
                state = viewModel.uiState,
                onRefresh = viewModel::refresh,
            )
        }
    }
}

@Composable
fun NowPlayingScreen(
    state: NowPlayingUiState,
    onRefresh: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(horizontal = 20.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Matchbox Audio",
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = state.device?.serviceState ?: "connecting",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(modifier = Modifier.width(12.dp))
            Button(onClick = onRefresh) {
                Text("Refresh")
            }
        }

        when {
            state.loading -> Text(
                text = "Loading snapshot",
                modifier = Modifier.testTag("loading"),
                style = MaterialTheme.typography.bodyLarge,
            )

            state.error != null -> Text(
                text = state.error,
                modifier = Modifier.testTag("error"),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyLarge,
            )

            state.device != null -> NowPlayingContent(state.device)
        }
    }
}

@Composable
private fun NowPlayingContent(device: DeviceSnapshot) {
    val playback = device.playback
    val track = playback.track

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .testTag("now-playing"),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = playback.state.uppercase(),
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            text = track?.displayTitle ?: "No track",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        if (track?.artist != null) {
            Text(
                text = track.artist,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (track?.album != null) {
            Text(
                text = track.album,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Spacer(modifier = Modifier.height(6.dp))

        DetailRow(label = "Volume", value = playback.volume.toString())
        DetailRow(
            label = "Queue",
            value = queueLabel(playback.queuePosition, playback.queueLength),
        )
        DetailRow(label = "Network", value = device.networkMode ?: "unknown")
        DetailRow(label = "Connection", value = device.activeConnection ?: "none")
    }
}

@Composable
private fun DetailRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
    }
}

private fun queueLabel(position: Int?, length: Int): String {
    if (length <= 0) {
        return "empty"
    }
    return when (position) {
        null -> length.toString()
        else -> "${position + 1} / $length"
    }
}
