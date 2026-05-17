package dev.matchbox.audio

internal class BleReconnectStateMachine(
    private val initialBackoffMillis: Long = BleProtocol.RECONNECT_INITIAL_BACKOFF_MILLIS,
    private val maxBackoffMillis: Long = BleProtocol.RECONNECT_MAX_BACKOFF_MILLIS,
) {
    private var connectionWanted = false
    private var nextBackoffMillis = initialBackoffMillis

    init {
        require(initialBackoffMillis > 0) { "initial backoff must be positive" }
        require(maxBackoffMillis >= initialBackoffMillis) {
            "max backoff must be greater than or equal to initial backoff"
        }
    }

    fun onConnectRequested() {
        connectionWanted = true
        resetBackoff()
    }

    fun onScheduledRetryStarted() {
        connectionWanted = true
    }

    fun onConnectionReady() {
        connectionWanted = true
        resetBackoff()
    }

    fun onConnectionClosed() {
        connectionWanted = false
        resetBackoff()
    }

    fun onTerminalProtocolState() {
        connectionWanted = false
        resetBackoff()
    }

    fun nextRetryDelayMillisAfterFailure(previousPhase: BleConnectionPhase): Long? {
        if (!connectionWanted || !previousPhase.allowsAutomaticReconnect()) {
            return null
        }

        val delay = nextBackoffMillis
        nextBackoffMillis = minOf(maxBackoffMillis, delay.saturatingDouble())
        return delay
    }

    private fun resetBackoff() {
        nextBackoffMillis = initialBackoffMillis
    }
}

internal fun BleConnectionPhase.isConnectionAttemptActive(): Boolean =
    when (this) {
        BleConnectionPhase.Reconnecting,
        BleConnectionPhase.Scanning,
        BleConnectionPhase.Connecting,
        BleConnectionPhase.RequestingMtu,
        BleConnectionPhase.DiscoveringServices,
        BleConnectionPhase.ReadingStatus,
        BleConnectionPhase.Subscribing,
        BleConnectionPhase.Ready,
        -> true

        BleConnectionPhase.Idle,
        BleConnectionPhase.Disconnected,
        BleConnectionPhase.Failed,
        BleConnectionPhase.AuthRequired,
        BleConnectionPhase.Busy,
        -> false
    }

private fun BleConnectionPhase.allowsAutomaticReconnect(): Boolean =
    when (this) {
        BleConnectionPhase.AuthRequired,
        BleConnectionPhase.Busy,
        -> false

        BleConnectionPhase.Idle,
        BleConnectionPhase.Reconnecting,
        BleConnectionPhase.Scanning,
        BleConnectionPhase.Connecting,
        BleConnectionPhase.RequestingMtu,
        BleConnectionPhase.DiscoveringServices,
        BleConnectionPhase.ReadingStatus,
        BleConnectionPhase.Subscribing,
        BleConnectionPhase.Ready,
        BleConnectionPhase.Disconnected,
        BleConnectionPhase.Failed,
        -> true
    }

private fun Long.saturatingDouble(): Long =
    if (this > Long.MAX_VALUE / 2) {
        Long.MAX_VALUE
    } else {
        this * 2
    }
