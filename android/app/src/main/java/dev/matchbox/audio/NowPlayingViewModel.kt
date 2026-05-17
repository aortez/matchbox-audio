package dev.matchbox.audio

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.launch

class NowPlayingViewModel(
    private var transport: MatchboxTransport = FakeMatchboxTransport(),
) : ViewModel() {
    var uiState by mutableStateOf(NowPlayingUiState())
        private set

    fun useTransport(transport: MatchboxTransport) {
        this.transport = transport
    }

    fun refresh() {
        viewModelScope.launch {
            loadSnapshot()
        }
    }

    fun play() {
        runPlaybackCommand(PlaybackCommand.Play)
    }

    fun pause() {
        runPlaybackCommand(PlaybackCommand.Pause)
    }

    fun stop() {
        runPlaybackCommand(PlaybackCommand.Stop)
    }

    fun next() {
        runPlaybackCommand(PlaybackCommand.Next)
    }

    fun previous() {
        runPlaybackCommand(PlaybackCommand.Previous)
    }

    fun setVolume(level: Int) {
        viewModelScope.launch {
            sendVolume(level)
        }
    }

    suspend fun loadSnapshot() {
        uiState = NowPlayingUiState(loading = true)
        uiState = try {
            NowPlayingUiState.ready(transport.requestSnapshot())
        } catch (error: Exception) {
            NowPlayingUiState.failed(error.message ?: "Snapshot request failed")
        }
    }

    suspend fun sendPlaybackCommand(command: PlaybackCommand) {
        uiState = uiState.copy(error = null)
        uiState = try {
            transport.sendPlaybackCommand(command)
            NowPlayingUiState.ready(transport.requestSnapshot())
        } catch (error: Exception) {
            NowPlayingUiState.failed(error.message ?: "Playback command failed")
        }
    }

    suspend fun sendVolume(level: Int) {
        val clampedLevel = level.coerceIn(0, 100)
        uiState = uiState.copy(error = null)
        uiState = try {
            transport.setVolume(clampedLevel)
            NowPlayingUiState.ready(transport.requestSnapshot())
        } catch (error: Exception) {
            NowPlayingUiState.failed(error.message ?: "Volume command failed")
        }
    }

    private fun runPlaybackCommand(command: PlaybackCommand) {
        viewModelScope.launch {
            sendPlaybackCommand(command)
        }
    }
}
