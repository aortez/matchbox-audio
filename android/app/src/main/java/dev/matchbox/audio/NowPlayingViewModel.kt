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

    suspend fun loadSnapshot() {
        uiState = NowPlayingUiState(loading = true)
        uiState = try {
            NowPlayingUiState.ready(transport.requestSnapshot())
        } catch (error: Exception) {
            NowPlayingUiState.failed(error.message ?: "Snapshot request failed")
        }
    }
}
