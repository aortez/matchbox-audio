package dev.matchbox.audio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BleReconnectStateMachineTest {
    @Test
    fun retryDelaysBackOffUntilMaximum() {
        val stateMachine = BleReconnectStateMachine(
            initialBackoffMillis = 100,
            maxBackoffMillis = 400,
        )

        stateMachine.onConnectRequested()

        assertEquals(
            100L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )
        assertEquals(
            200L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )
        assertEquals(
            400L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )
        assertEquals(
            400L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )
    }

    @Test
    fun successfulConnectionResetsRetryDelay() {
        val stateMachine = BleReconnectStateMachine(
            initialBackoffMillis = 100,
            maxBackoffMillis = 400,
        )

        stateMachine.onConnectRequested()
        assertEquals(
            100L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )
        assertEquals(
            200L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )

        stateMachine.onConnectionReady()

        assertEquals(
            100L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Ready),
        )
    }

    @Test
    fun doesNotRetryWhenConnectionIsNotWanted() {
        val stateMachine = BleReconnectStateMachine(
            initialBackoffMillis = 100,
            maxBackoffMillis = 400,
        )

        assertNull(stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Disconnected))

        stateMachine.onConnectRequested()
        stateMachine.onConnectionClosed()

        assertNull(stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Disconnected))
    }

    @Test
    fun protocolTerminalStatesDoNotAutoRetry() {
        val stateMachine = BleReconnectStateMachine(
            initialBackoffMillis = 100,
            maxBackoffMillis = 400,
        )

        stateMachine.onConnectRequested()

        assertNull(stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.AuthRequired))
        assertNull(stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Busy))
    }

    @Test
    fun manualConnectAfterTerminalStateRestartsRetryPolicy() {
        val stateMachine = BleReconnectStateMachine(
            initialBackoffMillis = 100,
            maxBackoffMillis = 400,
        )

        stateMachine.onConnectRequested()
        stateMachine.onTerminalProtocolState()
        assertNull(stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning))

        stateMachine.onConnectRequested()

        assertEquals(
            100L,
            stateMachine.nextRetryDelayMillisAfterFailure(BleConnectionPhase.Scanning),
        )
    }
}
