package dev.matchbox.audio

import android.Manifest
import android.annotation.SuppressLint
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import org.json.JSONArray
import org.json.JSONObject
import java.nio.charset.StandardCharsets

enum class BleConnectionPhase {
    Idle,
    Reconnecting,
    Scanning,
    Connecting,
    RequestingMtu,
    DiscoveringServices,
    ReadingStatus,
    Subscribing,
    Ready,
    Disconnected,
    Failed,
    AuthRequired,
    Busy,
}

data class BleConnectionState(
    val phase: BleConnectionPhase = BleConnectionPhase.Idle,
    val deviceName: String? = null,
    val deviceAddress: String? = null,
    val mtu: Int? = null,
    val statusJson: String? = null,
    val errorMessage: String? = null,
)

class BleTransportException(message: String) : IllegalStateException(message)

class BleMatchboxTransport(
    context: Context,
    private val mainDispatcher: CoroutineDispatcher = Dispatchers.Main.immediate,
    private val knownDeviceStore: BleKnownDeviceStore = SharedPreferencesBleKnownDeviceStore(context),
) : MatchboxTransport {
    private val appContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())
    private val requestMutex = Mutex()
    private val helloMutex = Mutex()

    private val _connectionState = MutableStateFlow(BleConnectionState())
    val connectionState: StateFlow<BleConnectionState> = _connectionState.asStateFlow()

    private var scanner: BluetoothLeScanner? = null
    private var gatt: BluetoothGatt? = null
    private var rxCharacteristic: BluetoothGattCharacteristic? = null
    private var txCharacteristic: BluetoothGattCharacteristic? = null
    private var writeInFlight = false
    private var helloCompleted = false
    private var nextTransportMessageId = 1L
    private var nextAppRequestId = 1
    private val pendingWrites = ArrayDeque<ByteArray>()
    private val pendingResponses = mutableMapOf<Int, CompletableDeferred<String>>()
    private val txReassembler = BleChunkReassembler()

    private val scanTimeoutRunnable = Runnable {
        if (_connectionState.value.phase == BleConnectionPhase.Scanning) {
            failOnMain("No Matchbox BLE device found before scan timeout")
        }
    }
    private val reconnectTimeoutRunnable = Runnable {
        if (_connectionState.value.phase == BleConnectionPhase.Reconnecting) {
            startScanFallbackOnMain()
        }
    }

    fun connect() {
        runOnMain {
            connectOnMain()
        }
    }

    fun close() {
        runOnMain {
            completePendingRequests(BleTransportException("BLE transport closed"))
            stopScanOnMain()
            closeGattOnMain(updateState = false)
            resetProtocolStateOnMain()
            _connectionState.value = BleConnectionState(phase = BleConnectionPhase.Disconnected)
        }
    }

    override suspend fun requestSnapshot(): DeviceSnapshot {
        ensureConnected()
        ensureHello()
        val response = sendRequest("system.snapshot")
        return ProtocolParser.parseSnapshotResponse(response)
    }

    private suspend fun ensureConnected() {
        withContext(mainDispatcher) {
            when (_connectionState.value.phase) {
                BleConnectionPhase.Ready,
                BleConnectionPhase.Reconnecting,
                BleConnectionPhase.Scanning,
                BleConnectionPhase.Connecting,
                BleConnectionPhase.RequestingMtu,
                BleConnectionPhase.DiscoveringServices,
                BleConnectionPhase.ReadingStatus,
                BleConnectionPhase.Subscribing,
                -> Unit

                BleConnectionPhase.Idle,
                BleConnectionPhase.Disconnected,
                BleConnectionPhase.Failed,
                BleConnectionPhase.AuthRequired,
                BleConnectionPhase.Busy,
                -> connectOnMain()
            }
        }

        val state = withTimeout(BleProtocol.CONNECT_TIMEOUT_MILLIS) {
            connectionState.first {
                it.phase == BleConnectionPhase.Ready ||
                    it.phase == BleConnectionPhase.Failed ||
                    it.phase == BleConnectionPhase.AuthRequired ||
                    it.phase == BleConnectionPhase.Busy ||
                    it.phase == BleConnectionPhase.Disconnected
            }
        }
        if (state.phase != BleConnectionPhase.Ready) {
            throw BleTransportException(state.errorMessage ?: "BLE transport is not connected")
        }
    }

    private suspend fun ensureHello() {
        helloMutex.withLock {
            val alreadyCompleted = withContext(mainDispatcher) { helloCompleted }
            if (alreadyCompleted) {
                return@withLock
            }

            val params = JSONObject()
                .put("app", "matchbox-android")
                .put("app_version", "0.1.0")
                .put(
                    "supported_protocol_versions",
                    JSONArray().put(BleProtocol.APP_PROTOCOL_VERSION),
                )
            val response = sendRequest("system.hello", params)
            val root = JSONObject(response)
            BleProtocolMessages.errorFromResponse(root)?.let { error ->
                throw BleTransportException(BleProtocolMessages.userFacingMessage(error))
            }
            withContext(mainDispatcher) {
                helloCompleted = true
            }
        }
    }

    private suspend fun sendRequest(method: String, params: JSONObject? = null): String =
        requestMutex.withLock {
            val deferred = CompletableDeferred<String>()
            var requestId: Int? = null
            try {
                requestId = withContext(mainDispatcher) {
                    enqueueRequestOnMain(method, params, deferred)
                }
                try {
                    withTimeout(BleProtocol.RESPONSE_TIMEOUT_MILLIS) {
                        deferred.await()
                    }
                } catch (_: TimeoutCancellationException) {
                    val message = "BLE request timed out waiting for $method response"
                    withContext(mainDispatcher) {
                        failOnMain(message)
                    }
                    throw BleTransportException(message)
                }
            } finally {
                withContext(mainDispatcher) {
                    requestId?.let { id ->
                        if (pendingResponses[id] === deferred) {
                            pendingResponses.remove(id)
                        }
                    }
                }
            }
        }

    private fun connectOnMain() {
        when (_connectionState.value.phase) {
            BleConnectionPhase.Reconnecting,
            BleConnectionPhase.Scanning,
            BleConnectionPhase.Connecting,
            BleConnectionPhase.RequestingMtu,
            BleConnectionPhase.DiscoveringServices,
            BleConnectionPhase.ReadingStatus,
            BleConnectionPhase.Subscribing,
            BleConnectionPhase.Ready,
            -> return

            BleConnectionPhase.Idle,
            BleConnectionPhase.Disconnected,
            BleConnectionPhase.Failed,
            BleConnectionPhase.AuthRequired,
            BleConnectionPhase.Busy,
            -> Unit
        }

        if (!hasScanPermission()) {
            failOnMain("Missing Android Bluetooth scan permission")
            return
        }
        if (!hasConnectPermission()) {
            failOnMain("Missing Android Bluetooth connect permission")
            return
        }

        val adapter = bluetoothAdapter()
        if (adapter == null || !adapter.isEnabled) {
            failOnMain("Bluetooth adapter unavailable or disabled")
            return
        }

        stopScanOnMain()
        closeGattOnMain(updateState = false)
        resetProtocolStateOnMain()

        val knownDevice = knownDeviceStore.load()
        if (knownDevice != null && connectToKnownDeviceOnMain(adapter, knownDevice)) {
            return
        }

        startScanOnMain(adapter)
    }

    private fun connectToKnownDeviceOnMain(
        adapter: BluetoothAdapter,
        knownDevice: BleKnownDevice,
    ): Boolean {
        val device = try {
            adapter.getRemoteDevice(knownDevice.address)
        } catch (_: IllegalArgumentException) {
            knownDeviceStore.clear()
            null
        } ?: return false

        return connectToDeviceOnMain(
            device = device,
            phase = BleConnectionPhase.Reconnecting,
            fallbackName = knownDevice.name,
        )
    }

    private fun startScanFallbackOnMain() {
        closeGattOnMain(updateState = false)
        resetProtocolStateOnMain()
        val adapter = bluetoothAdapter()
        if (adapter == null || !adapter.isEnabled) {
            failOnMain("Bluetooth adapter unavailable or disabled")
            return
        }
        startScanOnMain(adapter)
    }

    private fun startScanOnMain(adapter: BluetoothAdapter) {
        val nextScanner = adapter.bluetoothLeScanner
        if (nextScanner == null) {
            failOnMain("Bluetooth LE scanner unavailable")
            return
        }

        scanner = nextScanner
        _connectionState.value = BleConnectionState(phase = BleConnectionPhase.Scanning)

        val filter = ScanFilter.Builder()
            .setServiceUuid(ParcelUuid(BleProtocol.SERVICE_UUID))
            .build()
        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .build()

        try {
            nextScanner.startScan(listOf(filter), settings, scanCallback)
            mainHandler.postDelayed(scanTimeoutRunnable, BleProtocol.SCAN_TIMEOUT_MILLIS)
        } catch (error: SecurityException) {
            failOnMain("Android denied BLE scan: ${error.message ?: "missing permission"}")
        } catch (error: RuntimeException) {
            failOnMain("BLE scan could not start: ${error.message ?: error.javaClass.simpleName}")
        }
    }

    @SuppressLint("MissingPermission")
    private fun connectToDeviceOnMain(
        device: BluetoothDevice,
        phase: BleConnectionPhase = BleConnectionPhase.Connecting,
        fallbackName: String? = null,
    ): Boolean {
        if (!hasConnectPermission()) {
            failOnMain("Missing Android Bluetooth connect permission")
            return false
        }

        val deviceName = safeDeviceName(device) ?: fallbackName
        val deviceAddress = safeDeviceAddress(device)
        _connectionState.value = BleConnectionState(
            phase = phase,
            deviceName = deviceName,
            deviceAddress = deviceAddress,
        )
        gatt = try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                device.connectGatt(appContext, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
            } else {
                device.connectGatt(appContext, false, gattCallback)
            }
        } catch (error: SecurityException) {
            failOnMain("Android denied GATT connect: ${error.message ?: "missing permission"}")
            null
        }
        if (gatt == null) {
            return false
        }
        if (phase == BleConnectionPhase.Reconnecting) {
            mainHandler.postDelayed(reconnectTimeoutRunnable, BleProtocol.RECONNECT_TIMEOUT_MILLIS)
        }
        return true
    }

    @SuppressLint("MissingPermission")
    private fun discoverServicesOnMain(gatt: BluetoothGatt) {
        _connectionState.value = _connectionState.value.copy(
            phase = BleConnectionPhase.DiscoveringServices,
            errorMessage = null,
        )
        if (!gatt.discoverServices()) {
            failOnMain("GATT service discovery did not queue")
        }
    }

    @SuppressLint("MissingPermission")
    private fun enableNotificationsOnMain(gatt: BluetoothGatt, tx: BluetoothGattCharacteristic) {
        _connectionState.value = _connectionState.value.copy(
            phase = BleConnectionPhase.Subscribing,
            errorMessage = null,
        )

        if (!gatt.setCharacteristicNotification(tx, true)) {
            failOnMain("TX notification subscription did not queue")
            return
        }

        val cccd = tx.getDescriptor(BleProtocol.CCCD_UUID)
        if (cccd == null) {
            failOnMain("TX CCCD descriptor missing")
            return
        }

        if (Build.VERSION.SDK_INT >= 33) {
            val result = gatt.writeDescriptor(
                cccd,
                BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE,
            )
            if (result != BluetoothStatusCodes.SUCCESS) {
                failOnMain("TX CCCD descriptor write did not queue: $result")
            }
        } else {
            @Suppress("DEPRECATION")
            cccd.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
            @Suppress("DEPRECATION")
            if (!gatt.writeDescriptor(cccd)) {
                failOnMain("TX CCCD descriptor write did not queue")
            }
        }
    }

    private fun handleServicesDiscoveredOnMain(gatt: BluetoothGatt, status: Int) {
        if (status != BluetoothGatt.GATT_SUCCESS) {
            failOnMain("GATT service discovery failed: $status")
            return
        }

        val service = gatt.getService(BleProtocol.SERVICE_UUID)
        if (service == null) {
            failOnMain("Matchbox BLE service not found")
            return
        }

        val statusCharacteristic = service.characteristicOrFail(BleProtocol.STATUS_UUID)
        rxCharacteristic = service.characteristicOrFail(BleProtocol.RX_UUID)
        txCharacteristic = service.characteristicOrFail(BleProtocol.TX_UUID)

        if (statusCharacteristic == null || rxCharacteristic == null || txCharacteristic == null) {
            failOnMain("Matchbox BLE characteristic set is incomplete")
            return
        }

        _connectionState.value = _connectionState.value.copy(
            phase = BleConnectionPhase.ReadingStatus,
            errorMessage = null,
        )
        if (!readStatusOnMain(gatt, statusCharacteristic)) {
            enableNotificationsOnMain(gatt, requireNotNull(txCharacteristic))
        }
    }

    @SuppressLint("MissingPermission")
    private fun readStatusOnMain(
        gatt: BluetoothGatt,
        statusCharacteristic: BluetoothGattCharacteristic,
    ): Boolean =
        try {
            gatt.readCharacteristic(statusCharacteristic)
        } catch (_: SecurityException) {
            false
        }

    private fun handleCharacteristicReadOnMain(
        characteristic: BluetoothGattCharacteristic,
        value: ByteArray?,
        status: Int,
    ) {
        if (characteristic.uuid != BleProtocol.STATUS_UUID) {
            return
        }

        val tx = txCharacteristic
        if (status == BluetoothGatt.GATT_SUCCESS && value != null) {
            _connectionState.value = _connectionState.value.copy(
                statusJson = value.toString(StandardCharsets.UTF_8),
                errorMessage = null,
            )
        }
        if (tx != null && gatt != null) {
            enableNotificationsOnMain(requireNotNull(gatt), tx)
        } else {
            failOnMain("Cannot subscribe to TX notifications without a GATT connection")
        }
    }

    private fun handleDescriptorWriteOnMain(descriptor: BluetoothGattDescriptor, status: Int) {
        if (descriptor.uuid != BleProtocol.CCCD_UUID) {
            return
        }
        if (status != BluetoothGatt.GATT_SUCCESS) {
            failOnMain("TX notification subscription failed: $status")
            return
        }
        _connectionState.value = _connectionState.value.copy(
            phase = BleConnectionPhase.Ready,
            errorMessage = null,
        )
        rememberConnectedDeviceOnMain()
    }

    private fun enqueueRequestOnMain(
        method: String,
        params: JSONObject?,
        deferred: CompletableDeferred<String>,
    ): Int {
        if (_connectionState.value.phase != BleConnectionPhase.Ready) {
            throw BleTransportException("BLE transport is not ready")
        }
        if (gatt == null || rxCharacteristic == null) {
            throw BleTransportException("BLE RX characteristic is unavailable")
        }

        val requestId = nextAppRequestId()
        val request = JSONObject()
            .put("type", "request")
            .put("id", requestId)
            .put("method", method)
        if (params != null) {
            request.put("params", params)
        }

        val payload = request.toString().toByteArray(StandardCharsets.UTF_8)
        val chunks = BleChunkCodec.encode(nextTransportMessageId(), payload)
        pendingResponses[requestId] = deferred
        pendingWrites.addAll(chunks)
        writeNextChunkOnMain()
        return requestId
    }

    @SuppressLint("MissingPermission")
    private fun writeNextChunkOnMain() {
        if (writeInFlight || pendingWrites.isEmpty()) {
            return
        }

        val currentGatt = gatt
        val rx = rxCharacteristic
        if (currentGatt == null || rx == null) {
            failOnMain("Cannot write BLE chunk without an active RX characteristic")
            return
        }

        val chunk = pendingWrites.removeFirst()
        rx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
        if (Build.VERSION.SDK_INT >= 33) {
            val result = currentGatt.writeCharacteristic(
                rx,
                chunk,
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT,
            )
            if (result == BluetoothStatusCodes.SUCCESS) {
                writeInFlight = true
            } else {
                failOnMain("RX chunk write did not queue: $result")
            }
        } else {
            @Suppress("DEPRECATION")
            rx.value = chunk
            @Suppress("DEPRECATION")
            val queued = currentGatt.writeCharacteristic(rx)
            writeInFlight = queued
            if (!queued) {
                failOnMain("RX chunk write did not queue")
            }
        }
    }

    private fun handleCharacteristicWriteOnMain(status: Int) {
        writeInFlight = false
        if (status != BluetoothGatt.GATT_SUCCESS) {
            failOnMain("RX chunk write failed: $status")
            return
        }
        writeNextChunkOnMain()
    }

    private fun handleNotificationOnMain(characteristic: BluetoothGattCharacteristic, value: ByteArray?) {
        if (characteristic.uuid != BleProtocol.TX_UUID || value == null) {
            return
        }

        val completed = try {
            txReassembler.push(value)
        } catch (error: BleChunkException) {
            failOnMain("Rejected TX BLE chunk: ${error.message}")
            return
        }

        if (completed != null) {
            handleProtocolMessageOnMain(completed.toString(StandardCharsets.UTF_8))
        }
    }

    private fun handleProtocolMessageOnMain(json: String) {
        val root = try {
            JSONObject(json)
        } catch (error: Exception) {
            failOnMain("Invalid BLE protocol JSON: ${error.message}")
            return
        }

        if (root.optString("type") != "response") {
            return
        }

        val protocolError = BleProtocolMessages.errorFromResponse(root)
        if (protocolError?.code == BleProtocolMessages.ERROR_AUTH_REQUIRED) {
            markAuthRequiredOnMain()
            return
        }
        if (protocolError?.code == BleProtocolMessages.ERROR_BUSY) {
            markBusyOnMain()
            return
        }

        val requestId = root.optInt("id", -1)
        val pending = pendingResponses[requestId] ?: return
        if (root.optBoolean("ok")) {
            pending.complete(json)
        } else {
            val message = protocolError?.let(BleProtocolMessages::userFacingMessage)
                ?: "BLE protocol request failed"
            pending.completeExceptionally(BleTransportException(message))
        }
    }

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            runOnMain {
                stopScanOnMain()
                if (!connectToDeviceOnMain(result.device)) {
                    failOnMain("GATT connection did not queue")
                }
            }
        }

        override fun onScanFailed(errorCode: Int) {
            runOnMain {
                failOnMain("BLE scan failed: $errorCode")
            }
        }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        @SuppressLint("MissingPermission")
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                if (status == BluetoothGatt.GATT_SUCCESS && newState == BluetoothProfile.STATE_CONNECTED) {
                    mainHandler.removeCallbacks(reconnectTimeoutRunnable)
                    this@BleMatchboxTransport.gatt = gatt
                    _connectionState.value = _connectionState.value.copy(
                        phase = BleConnectionPhase.RequestingMtu,
                        errorMessage = null,
                    )
                    if (!gatt.requestMtu(BleProtocol.REQUESTED_MTU)) {
                        discoverServicesOnMain(gatt)
                    }
                } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                    val wasReconnecting =
                        _connectionState.value.phase == BleConnectionPhase.Reconnecting
                    completePendingRequests(BleTransportException("BLE GATT disconnected"))
                    closeGattOnMain(updateState = false)
                    resetProtocolStateOnMain()
                    if (wasReconnecting) {
                        startScanFallbackOnMain()
                    } else {
                        _connectionState.value = _connectionState.value.copy(
                            phase = BleConnectionPhase.Disconnected,
                            errorMessage = if (status == BluetoothGatt.GATT_SUCCESS) {
                                null
                            } else {
                                "BLE GATT disconnected with status $status"
                            },
                        )
                    }
                } else if (status != BluetoothGatt.GATT_SUCCESS) {
                    if (_connectionState.value.phase == BleConnectionPhase.Reconnecting) {
                        startScanFallbackOnMain()
                    } else {
                        failOnMain("BLE GATT connection failed: $status")
                    }
                }
            }
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                _connectionState.value = _connectionState.value.copy(
                    mtu = if (status == BluetoothGatt.GATT_SUCCESS) mtu else null,
                    errorMessage = null,
                )
                discoverServicesOnMain(gatt)
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                handleServicesDiscoveredOnMain(gatt, status)
            }
        }

        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
            status: Int,
        ) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                handleCharacteristicReadOnMain(characteristic, value, status)
            }
        }

        @Suppress("DEPRECATION")
        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                handleCharacteristicReadOnMain(characteristic, characteristic.value, status)
            }
        }

        override fun onDescriptorWrite(
            gatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
            status: Int,
        ) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                handleDescriptorWriteOnMain(descriptor, status)
            }
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                if (characteristic.uuid == BleProtocol.RX_UUID) {
                    handleCharacteristicWriteOnMain(status)
                }
            }
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                handleNotificationOnMain(characteristic, value)
            }
        }

        @Suppress("DEPRECATION")
        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
        ) {
            runOnMain {
                if (!isCurrentGattOnMain(gatt)) {
                    return@runOnMain
                }
                handleNotificationOnMain(characteristic, characteristic.value)
            }
        }
    }

    private fun resetProtocolStateOnMain() {
        writeInFlight = false
        helloCompleted = false
        nextTransportMessageId = 1L
        nextAppRequestId = 1
        pendingWrites.clear()
        txReassembler.reset()
        rxCharacteristic = null
        txCharacteristic = null
    }

    private fun rememberConnectedDeviceOnMain() {
        val state = _connectionState.value
        val address = state.deviceAddress?.takeIf(BleDeviceAddresses::isValid) ?: return
        knownDeviceStore.save(
            BleKnownDevice(
                address = address,
                name = state.deviceName,
            ),
        )
    }

    private fun markBusyOnMain() {
        val message = "Another app is connected"
        markProtocolTerminalOnMain(
            phase = BleConnectionPhase.Busy,
            message = message,
        )
    }

    private fun markAuthRequiredOnMain() {
        markProtocolTerminalOnMain(
            phase = BleConnectionPhase.AuthRequired,
            message = "Open pairing mode on Matchbox Audio",
        )
    }

    private fun markProtocolTerminalOnMain(phase: BleConnectionPhase, message: String) {
        val error = BleTransportException(message)
        val current = _connectionState.value
        mainHandler.removeCallbacks(reconnectTimeoutRunnable)
        stopScanOnMain()
        closeGattOnMain(updateState = false)
        resetProtocolStateOnMain()
        completePendingRequests(error)
        _connectionState.value = BleConnectionState(
            phase = phase,
            deviceName = current.deviceName,
            deviceAddress = current.deviceAddress,
            mtu = current.mtu,
            statusJson = current.statusJson,
            errorMessage = message,
        )
    }

    private fun failOnMain(message: String) {
        val error = BleTransportException(message)
        mainHandler.removeCallbacks(reconnectTimeoutRunnable)
        stopScanOnMain()
        closeGattOnMain(updateState = false)
        resetProtocolStateOnMain()
        completePendingRequests(error)
        _connectionState.value = _connectionState.value.copy(
            phase = BleConnectionPhase.Failed,
            errorMessage = message,
        )
    }

    private fun completePendingRequests(error: Throwable) {
        pendingResponses.values.forEach { it.completeExceptionally(error) }
        pendingResponses.clear()
    }

    @SuppressLint("MissingPermission")
    private fun stopScanOnMain() {
        mainHandler.removeCallbacks(scanTimeoutRunnable)
        val currentScanner = scanner ?: return
        scanner = null
        if (!hasScanPermission()) {
            return
        }
        try {
            currentScanner.stopScan(scanCallback)
        } catch (_: RuntimeException) {
        } catch (_: SecurityException) {
        }
    }

    @SuppressLint("MissingPermission")
    private fun closeGattOnMain(updateState: Boolean) {
        mainHandler.removeCallbacks(reconnectTimeoutRunnable)
        val currentGatt = gatt ?: return
        gatt = null
        if (hasConnectPermission()) {
            try {
                currentGatt.disconnect()
            } catch (_: RuntimeException) {
            } catch (_: SecurityException) {
            }
            try {
                currentGatt.close()
            } catch (_: RuntimeException) {
            }
        }
        if (updateState) {
            _connectionState.value = _connectionState.value.copy(phase = BleConnectionPhase.Disconnected)
        }
    }

    private fun isCurrentGattOnMain(callbackGatt: BluetoothGatt): Boolean =
        gatt === callbackGatt

    private fun nextTransportMessageId(): Long {
        val messageId = nextTransportMessageId
        nextTransportMessageId += 1
        if (nextTransportMessageId > 0xffff_ffffL) {
            nextTransportMessageId = 1L
        }
        return messageId
    }

    private fun nextAppRequestId(): Int {
        val requestId = nextAppRequestId
        nextAppRequestId = if (nextAppRequestId == Int.MAX_VALUE) 1 else nextAppRequestId + 1
        return requestId
    }

    private fun bluetoothAdapter(): BluetoothAdapter? =
        appContext.getSystemService(BluetoothManager::class.java)?.adapter

    private fun hasPermission(permission: String): Boolean =
        appContext.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    private fun hasScanPermission(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            hasPermission(Manifest.permission.BLUETOOTH_SCAN)
        } else {
            hasPermission(Manifest.permission.ACCESS_FINE_LOCATION)
        }

    private fun hasConnectPermission(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
            hasPermission(Manifest.permission.BLUETOOTH_CONNECT)

    @SuppressLint("MissingPermission")
    private fun safeDeviceName(device: BluetoothDevice): String? =
        if (!hasConnectPermission()) {
            null
        } else {
            try {
                device.name
            } catch (_: SecurityException) {
                null
            }
        }

    @SuppressLint("MissingPermission")
    private fun safeDeviceAddress(device: BluetoothDevice): String? =
        try {
            device.address
        } catch (_: SecurityException) {
            null
        }

    private fun runOnMain(block: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            block()
        } else {
            mainHandler.post(block)
        }
    }

    private fun BluetoothGattService.characteristicOrFail(
        uuid: java.util.UUID,
    ): BluetoothGattCharacteristic? = getCharacteristic(uuid)
}
