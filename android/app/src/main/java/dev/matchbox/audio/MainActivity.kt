package dev.matchbox.audio

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.compose.foundation.BorderStroke
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
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

class MainActivity : ComponentActivity() {
    private lateinit var bleTransport: BleMatchboxTransport

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        bleTransport = BleMatchboxTransport(this)
        setContent {
            MatchboxApp(bleTransport = bleTransport)
        }
    }

    override fun onDestroy() {
        if (::bleTransport.isInitialized) {
            bleTransport.close()
        }
        super.onDestroy()
    }
}

@Composable
fun MatchboxApp(
    bleTransport: BleMatchboxTransport? = null,
) {
    MaterialTheme {
        val context = LocalContext.current
        val activeBleTransport = remember(bleTransport) {
            bleTransport ?: BleMatchboxTransport(context)
        }
        val viewModel = remember { NowPlayingViewModel() }
        val bleState by activeBleTransport.connectionState.collectAsState()
        var usingBle by rememberSaveable { mutableStateOf(false) }
        var permissionDenied by rememberSaveable { mutableStateOf(false) }
        val permissionLauncher = rememberLauncherForActivityResult(
            ActivityResultContracts.RequestMultiplePermissions(),
        ) { permissions ->
            if (hasAllBlePermissions(context, permissions)) {
                permissionDenied = false
                usingBle = true
                viewModel.useTransport(activeBleTransport)
                activeBleTransport.connect()
                viewModel.refresh()
            } else {
                permissionDenied = true
            }
        }

        LaunchedEffect(viewModel) {
            viewModel.loadSnapshot()
        }

        Surface(modifier = Modifier.fillMaxSize()) {
            NowPlayingScreen(
                state = viewModel.uiState,
                usingBle = usingBle,
                bleConnectionState = bleState,
                permissionDenied = permissionDenied,
                onRefresh = viewModel::refresh,
                onConnectBle = {
                    val missingPermissions = missingBlePermissions(context)
                    if (missingPermissions.isEmpty()) {
                        permissionDenied = false
                        usingBle = true
                        viewModel.useTransport(activeBleTransport)
                        activeBleTransport.connect()
                        viewModel.refresh()
                    } else {
                        permissionLauncher.launch(missingPermissions.toTypedArray())
                    }
                },
                onUseDemo = {
                    usingBle = false
                    permissionDenied = false
                    activeBleTransport.close()
                    viewModel.useTransport(FakeMatchboxTransport())
                    viewModel.refresh()
                },
            )
        }
    }
}

@Composable
fun NowPlayingScreen(
    state: NowPlayingUiState,
    onRefresh: () -> Unit,
    modifier: Modifier = Modifier,
    usingBle: Boolean = false,
    bleConnectionState: BleConnectionState = BleConnectionState(),
    permissionDenied: Boolean = false,
    onConnectBle: () -> Unit = {},
    onUseDemo: () -> Unit = {},
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(horizontal = 20.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth(),
            verticalArrangement = Arrangement.spacedBy(10.dp),
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
                        text = headerStatusText(
                            state = state,
                            usingBle = usingBle,
                            bleConnectionState = bleConnectionState,
                            permissionDenied = permissionDenied,
                        ),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
            ) {
                if (usingBle) {
                    OutlinedButton(onClick = onUseDemo) {
                        Text("Demo")
                    }
                } else {
                    OutlinedButton(onClick = onConnectBle) {
                        Text("Connect BLE")
                    }
                }
                Button(onClick = onRefresh) {
                    Text("Refresh")
                }
            }
        }

        ConnectionStatusRow(
            usingBle = usingBle,
            bleConnectionState = bleConnectionState,
            permissionDenied = permissionDenied,
        )

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
private fun ConnectionStatusRow(
    usingBle: Boolean,
    bleConnectionState: BleConnectionState,
    permissionDenied: Boolean,
) {
    val label = when {
        permissionDenied -> "Bluetooth permission denied"
        usingBle -> blePhaseLabel(bleConnectionState.phase)
        else -> "Demo transport"
    }
    val detail = when {
        permissionDenied -> "Grant Bluetooth access to connect"
        usingBle && bleConnectionState.phase == BleConnectionPhase.Busy -> "Another app is connected"
        usingBle && bleConnectionState.errorMessage != null -> bleConnectionState.errorMessage
        usingBle && bleConnectionState.deviceName != null -> bleConnectionState.deviceName
        usingBle && bleConnectionState.deviceAddress != null -> bleConnectionState.deviceAddress
        usingBle && bleConnectionState.mtu != null -> "MTU ${bleConnectionState.mtu}"
        usingBle -> "Matchbox BLE"
        else -> "Fake snapshot"
    }

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .testTag(if (permissionDenied) "permission-denied" else "connection-status"),
        shape = MaterialTheme.shapes.small,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Medium,
            )
            Spacer(modifier = Modifier.width(12.dp))
            Text(
                text = detail,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
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

private fun headerStatusText(
    state: NowPlayingUiState,
    usingBle: Boolean,
    bleConnectionState: BleConnectionState,
    permissionDenied: Boolean,
): String =
    when {
        permissionDenied -> "permission needed"
        usingBle -> blePhaseLabel(bleConnectionState.phase)
        else -> state.device?.serviceState ?: "connecting"
    }

private fun blePhaseLabel(phase: BleConnectionPhase): String =
    when (phase) {
        BleConnectionPhase.Idle -> "BLE idle"
        BleConnectionPhase.Reconnecting -> "Reconnecting"
        BleConnectionPhase.Scanning -> "Scanning"
        BleConnectionPhase.Connecting -> "Connecting"
        BleConnectionPhase.RequestingMtu -> "Requesting MTU"
        BleConnectionPhase.DiscoveringServices -> "Discovering services"
        BleConnectionPhase.ReadingStatus -> "Reading status"
        BleConnectionPhase.Subscribing -> "Subscribing"
        BleConnectionPhase.Ready -> "BLE ready"
        BleConnectionPhase.Disconnected -> "BLE disconnected"
        BleConnectionPhase.Failed -> "BLE failed"
        BleConnectionPhase.Busy -> "Device busy"
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

private fun requiredBlePermissions(): List<String> =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        listOf(
            Manifest.permission.BLUETOOTH_SCAN,
            Manifest.permission.BLUETOOTH_CONNECT,
        )
    } else {
        listOf(Manifest.permission.ACCESS_FINE_LOCATION)
    }

private fun missingBlePermissions(context: Context): List<String> =
    requiredBlePermissions().filter { permission ->
        context.checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED
    }

private fun hasAllBlePermissions(
    context: Context,
    permissionResults: Map<String, Boolean>,
): Boolean =
    requiredBlePermissions().all { permission ->
        permissionResults[permission] == true ||
            context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
    }
