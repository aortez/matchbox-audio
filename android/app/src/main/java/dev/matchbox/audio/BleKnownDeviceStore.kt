package dev.matchbox.audio

import android.content.Context
import android.content.SharedPreferences

data class BleKnownDevice(
    val address: String,
    val name: String?,
)

interface BleKnownDeviceStore {
    fun load(): BleKnownDevice?
    fun save(device: BleKnownDevice)
    fun clear()
}

class SharedPreferencesBleKnownDeviceStore(
    context: Context,
) : BleKnownDeviceStore {
    private val preferences: SharedPreferences =
        context.applicationContext.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    override fun load(): BleKnownDevice? {
        val address = preferences.getString(KEY_ADDRESS, null)?.takeIf(BleDeviceAddresses::isValid)
            ?: return null
        val name = preferences.getString(KEY_NAME, null)?.takeIf { it.isNotBlank() }
        return BleKnownDevice(address = address, name = name)
    }

    override fun save(device: BleKnownDevice) {
        if (!BleDeviceAddresses.isValid(device.address)) {
            return
        }
        preferences.edit()
            .putString(KEY_ADDRESS, device.address)
            .putString(KEY_NAME, device.name?.takeIf { it.isNotBlank() })
            .apply()
    }

    override fun clear() {
        preferences.edit()
            .remove(KEY_ADDRESS)
            .remove(KEY_NAME)
            .apply()
    }

    private companion object {
        const val PREFERENCES_NAME = "matchbox_ble"
        const val KEY_ADDRESS = "known_device_address"
        const val KEY_NAME = "known_device_name"
    }
}

object BleDeviceAddresses {
    private val bluetoothAddress = Regex("(?i)^[0-9a-f]{2}(:[0-9a-f]{2}){5}$")

    fun isValid(address: String): Boolean = bluetoothAddress.matches(address)
}
