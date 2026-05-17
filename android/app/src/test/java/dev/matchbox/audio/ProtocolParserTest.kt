package dev.matchbox.audio

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

class ProtocolParserTest {
    @Test
    fun parsesSharedSystemSnapshotFixture() {
        val json = requireNotNull(
            javaClass.classLoader?.getResource("v1/system_snapshot_response.json"),
        ).readText()

        val snapshot = ProtocolParser.parseSnapshotResponse(json)

        assertEquals("ready", snapshot.serviceState)
        assertEquals("car", snapshot.networkMode)
        assertEquals("play", snapshot.playback.state)
        assertEquals(65, snapshot.playback.volume)
        assertEquals(12, snapshot.playback.queueLength)
        assertEquals(2, snapshot.playback.queuePosition)
        val track = snapshot.playback.track
        assertNotNull(track)
        assertEquals("Speak to Me", track?.displayTitle)
        assertEquals("Pink Floyd", track?.artist)
        assertEquals("Dark Side", track?.album)
    }
}
