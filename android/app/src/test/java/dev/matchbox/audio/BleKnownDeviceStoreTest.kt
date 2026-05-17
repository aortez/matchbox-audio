package dev.matchbox.audio

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BleKnownDeviceStoreTest {
    @Test
    fun bluetoothAddressValidationAcceptsCanonicalAddresses() {
        assertTrue(BleDeviceAddresses.isValid("88:A2:9E:B1:87:91"))
        assertTrue(BleDeviceAddresses.isValid("88:a2:9e:b1:87:91"))
    }

    @Test
    fun bluetoothAddressValidationRejectsInvalidAddresses() {
        assertFalse(BleDeviceAddresses.isValid(""))
        assertFalse(BleDeviceAddresses.isValid("88:A2:9E:B1:87"))
        assertFalse(BleDeviceAddresses.isValid("88-A2-9E-B1-87-91"))
        assertFalse(BleDeviceAddresses.isValid("../not-a-device"))
    }
}
