package dev.matchbox.ble_smoke;

import android.Manifest;
import android.app.Activity;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothGatt;
import android.bluetooth.BluetoothGattCallback;
import android.bluetooth.BluetoothGattCharacteristic;
import android.bluetooth.BluetoothGattDescriptor;
import android.bluetooth.BluetoothGattService;
import android.bluetooth.BluetoothManager;
import android.bluetooth.BluetoothProfile;
import android.bluetooth.BluetoothStatusCodes;
import android.bluetooth.le.BluetoothLeScanner;
import android.bluetooth.le.ScanCallback;
import android.bluetooth.le.ScanFilter;
import android.bluetooth.le.ScanResult;
import android.bluetooth.le.ScanSettings;
import android.content.Context;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ParcelUuid;
import android.text.method.ScrollingMovementMethod;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import org.json.JSONArray;
import org.json.JSONObject;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.UUID;

public class MainActivity extends Activity {
    private static final UUID SERVICE_UUID = UUID.fromString("1cef04f1-966e-43ad-860f-086db4f277d6");
    private static final UUID STATUS_UUID = UUID.fromString("bd539314-4637-416b-a3b5-804fecd5b792");
    private static final UUID RX_UUID = UUID.fromString("fbf39e22-bb07-49bf-bfa0-3dbdfc47769b");
    private static final UUID TX_UUID = UUID.fromString("fcc9055c-34e3-46d9-a010-bd8a4f180b0c");
    private static final UUID CCCD_UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb");

    private static final byte[] CHUNK_MAGIC = new byte[]{'M', 'B'};
    private static final int CHUNK_VERSION = 1;
    private static final int FLAG_FIRST_CHUNK = 0x01;
    private static final int FLAG_LAST_CHUNK = 0x02;
    private static final int CHUNK_HEADER_BYTES = 16;
    private static final int TARGET_GATT_VALUE_BYTES = 244;
    private static final int TARGET_CHUNK_PAYLOAD_BYTES = TARGET_GATT_VALUE_BYTES - CHUNK_HEADER_BYTES;

    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final ArrayDeque<byte[]> pendingWrites = new ArrayDeque<>();

    private TextView logView;
    private BluetoothLeScanner scanner;
    private BluetoothGatt gatt;
    private BluetoothGattCharacteristic rxCharacteristic;
    private BluetoothGattCharacteristic txCharacteristic;
    private PartialMessage partialTx;
    private int nextTransportMessageId = 1;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(buildUi());
        requestNeededPermissions();
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        stopScan();
        if (gatt != null) {
            if (hasConnectPermission()) {
                gatt.close();
            }
            gatt = null;
        }
    }

    private LinearLayout buildUi() {
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(16);
        root.setPadding(pad, pad, pad, pad);

        Button start = new Button(this);
        start.setText("Start BLE Smoke");
        start.setOnClickListener(v -> startSmoke());
        root.addView(start, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT));

        Button clear = new Button(this);
        clear.setText("Clear Log");
        clear.setOnClickListener(v -> logView.setText(""));
        root.addView(clear, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT));

        logView = new TextView(this);
        logView.setTextSize(13);
        logView.setMovementMethod(new ScrollingMovementMethod());

        ScrollView scroll = new ScrollView(this);
        scroll.addView(logView);
        root.addView(scroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                0,
                1f));

        return root;
    }

    private void startSmoke() {
        if (!hasBlePermissions()) {
            appendLog("Missing Bluetooth permissions; requesting again.");
            requestNeededPermissions();
            return;
        }

        BluetoothManager manager = (BluetoothManager) getSystemService(Context.BLUETOOTH_SERVICE);
        BluetoothAdapter adapter = manager != null ? manager.getAdapter() : null;
        if (adapter == null || !adapter.isEnabled()) {
            appendLog("Bluetooth adapter unavailable or disabled.");
            return;
        }

        scanner = adapter.getBluetoothLeScanner();
        if (scanner == null) {
            appendLog("BluetoothLeScanner unavailable.");
            return;
        }

        appendLog("Scanning for Matchbox service " + SERVICE_UUID);
        ScanFilter filter = new ScanFilter.Builder()
                .setServiceUuid(new ParcelUuid(SERVICE_UUID))
                .build();
        ScanSettings settings = new ScanSettings.Builder()
                .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
                .build();

        scanner.startScan(Collections.singletonList(filter), settings, scanCallback);
        mainHandler.postDelayed(this::stopScan, 10_000);
    }

    private void stopScan() {
        if (scanner != null && hasScanPermission()) {
            scanner.stopScan(scanCallback);
        }
    }

    private final ScanCallback scanCallback = new ScanCallback() {
        @Override
        public void onScanResult(int callbackType, ScanResult result) {
            BluetoothDevice device = result.getDevice();
            appendLog("Found device: " + safeDeviceName(device) + " " + device.getAddress());
            stopScan();
            connect(device);
        }

        @Override
        public void onScanFailed(int errorCode) {
            appendLog("Scan failed: " + errorCode);
        }
    };

    private void connect(BluetoothDevice device) {
        if (!hasConnectPermission()) {
            appendLog("Missing BLUETOOTH_CONNECT permission.");
            return;
        }

        appendLog("Connecting GATT...");
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            gatt = device.connectGatt(this, false, gattCallback, BluetoothDevice.TRANSPORT_LE);
        } else {
            gatt = device.connectGatt(this, false, gattCallback);
        }
    }

    private final BluetoothGattCallback gattCallback = new BluetoothGattCallback() {
        @Override
        public void onConnectionStateChange(BluetoothGatt gatt, int status, int newState) {
            appendLog("Connection state status=" + status + " newState=" + newState);
            if (status == BluetoothGatt.GATT_SUCCESS && newState == BluetoothProfile.STATE_CONNECTED) {
                appendLog("Requesting MTU 517");
                if (!gatt.requestMtu(517)) {
                    appendLog("MTU request returned false; discovering services now.");
                    gatt.discoverServices();
                }
            }
        }

        @Override
        public void onMtuChanged(BluetoothGatt gatt, int mtu, int status) {
            appendLog("MTU changed status=" + status + " mtu=" + mtu);
            gatt.discoverServices();
        }

        @Override
        public void onServicesDiscovered(BluetoothGatt gatt, int status) {
            appendLog("Services discovered status=" + status);
            if (status != BluetoothGatt.GATT_SUCCESS) {
                return;
            }

            BluetoothGattService service = gatt.getService(SERVICE_UUID);
            if (service == null) {
                appendLog("Matchbox service not found.");
                return;
            }

            BluetoothGattCharacteristic statusCharacteristic = service.getCharacteristic(STATUS_UUID);
            rxCharacteristic = service.getCharacteristic(RX_UUID);
            txCharacteristic = service.getCharacteristic(TX_UUID);

            if (statusCharacteristic == null || rxCharacteristic == null || txCharacteristic == null) {
                appendLog("Missing one or more Matchbox characteristics.");
                return;
            }

            appendLog("Matchbox characteristics found.");
            if (!gatt.readCharacteristic(statusCharacteristic)) {
                appendLog("Status read did not queue; enabling notifications.");
                enableNotifications(gatt, txCharacteristic);
            }
        }

        @Override
        public void onCharacteristicRead(
                BluetoothGatt gatt,
                BluetoothGattCharacteristic characteristic,
                byte[] value,
                int status
        ) {
            handleCharacteristicRead(characteristic, value, status);
        }

        @SuppressWarnings("deprecation")
        @Override
        public void onCharacteristicRead(
                BluetoothGatt gatt,
                BluetoothGattCharacteristic characteristic,
                int status
        ) {
            handleCharacteristicRead(characteristic, characteristic.getValue(), status);
        }

        @Override
        public void onDescriptorWrite(BluetoothGatt gatt, BluetoothGattDescriptor descriptor, int status) {
            appendLog("Descriptor write status=" + status);
            if (status == BluetoothGatt.GATT_SUCCESS && CCCD_UUID.equals(descriptor.getUuid())) {
                sendHello();
            }
        }

        @Override
        public void onCharacteristicWrite(
                BluetoothGatt gatt,
                BluetoothGattCharacteristic characteristic,
                int status
        ) {
            appendLog("RX write status=" + status);
            if (status == BluetoothGatt.GATT_SUCCESS) {
                writeNextChunk();
            }
        }

        @Override
        public void onCharacteristicChanged(
                BluetoothGatt gatt,
                BluetoothGattCharacteristic characteristic,
                byte[] value
        ) {
            handleNotification(value);
        }

        @SuppressWarnings("deprecation")
        @Override
        public void onCharacteristicChanged(
                BluetoothGatt gatt,
                BluetoothGattCharacteristic characteristic
        ) {
            handleNotification(characteristic.getValue());
        }
    };

    private void handleCharacteristicRead(
            BluetoothGattCharacteristic characteristic,
            byte[] value,
            int status
    ) {
        if (!STATUS_UUID.equals(characteristic.getUuid())) {
            return;
        }
        appendLog("Status read status=" + status);
        if (status == BluetoothGatt.GATT_SUCCESS && value != null) {
            appendLog("Status: " + new String(value, StandardCharsets.UTF_8));
        }
        if (txCharacteristic != null) {
            enableNotifications(gatt, txCharacteristic);
        }
    }

    private void enableNotifications(BluetoothGatt gatt, BluetoothGattCharacteristic txCharacteristic) {
        appendLog("Enabling TX notifications.");
        gatt.setCharacteristicNotification(txCharacteristic, true);

        BluetoothGattDescriptor cccd = txCharacteristic.getDescriptor(CCCD_UUID);
        if (cccd == null) {
            appendLog("TX CCCD descriptor missing.");
            return;
        }

        if (Build.VERSION.SDK_INT >= 33) {
            int result = gatt.writeDescriptor(cccd, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE);
            appendLog("CCCD write queued result=" + result);
        } else {
            @SuppressWarnings("deprecation")
            boolean ignored = cccd.setValue(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE);
            @SuppressWarnings("deprecation")
            boolean queued = gatt.writeDescriptor(cccd);
            appendLog("CCCD write queued=" + queued);
        }
    }

    private void sendHello() {
        try {
            JSONObject params = new JSONObject()
                    .put("app", "android-ble-smoke")
                    .put("supported_protocol_versions", new JSONArray().put(1));
            JSONObject request = new JSONObject()
                    .put("type", "request")
                    .put("id", 1)
                    .put("method", "system.hello")
                    .put("params", params);

            byte[] payload = request.toString().getBytes(StandardCharsets.UTF_8);
            int messageId = nextTransportMessageId++;
            pendingWrites.clear();
            pendingWrites.addAll(encodeChunks(messageId, payload));
            appendLog("Writing system.hello in " + pendingWrites.size() + " chunk(s).");
            writeNextChunk();
        } catch (Exception e) {
            appendLog("Failed to build system.hello: " + e);
        }
    }

    private void writeNextChunk() {
        if (gatt == null || rxCharacteristic == null || pendingWrites.isEmpty()) {
            if (pendingWrites.isEmpty()) {
                appendLog("All RX chunks written.");
            }
            return;
        }

        byte[] chunk = pendingWrites.removeFirst();
        rxCharacteristic.setWriteType(BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT);
        if (Build.VERSION.SDK_INT >= 33) {
            int result = gatt.writeCharacteristic(
                    rxCharacteristic,
                    chunk,
                    BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT);
            appendLog("Queued RX chunk bytes=" + chunk.length + " result=" + result);
            if (result != BluetoothStatusCodes.SUCCESS) {
                appendLog("RX write did not queue; stopping.");
            }
        } else {
            @SuppressWarnings("deprecation")
            boolean ignored = rxCharacteristic.setValue(chunk);
            @SuppressWarnings("deprecation")
            boolean queued = gatt.writeCharacteristic(rxCharacteristic);
            appendLog("Queued RX chunk bytes=" + chunk.length + " queued=" + queued);
        }
    }

    private void handleNotification(byte[] value) {
        if (value == null) {
            appendLog("TX notification had null value.");
            return;
        }
        appendLog("TX notification bytes=" + value.length);

        try {
            byte[] completed = reassembleChunk(value);
            if (completed != null) {
                appendLog("Complete TX message:");
                appendLog(new String(completed, StandardCharsets.UTF_8));
            }
        } catch (Exception e) {
            appendLog("Failed to parse TX chunk: " + e.getMessage());
            partialTx = null;
        }
    }

    private List<byte[]> encodeChunks(int messageId, byte[] payload) {
        int chunkCount = Math.max(1, (payload.length + TARGET_CHUNK_PAYLOAD_BYTES - 1) / TARGET_CHUNK_PAYLOAD_BYTES);
        List<byte[]> chunks = new ArrayList<>(chunkCount);

        for (int chunkIndex = 0; chunkIndex < chunkCount; chunkIndex++) {
            int start = chunkIndex * TARGET_CHUNK_PAYLOAD_BYTES;
            int end = Math.min(start + TARGET_CHUNK_PAYLOAD_BYTES, payload.length);
            int fragmentLen = end - start;
            int flags = 0;
            if (chunkIndex == 0) {
                flags |= FLAG_FIRST_CHUNK;
            }
            if (chunkIndex + 1 == chunkCount) {
                flags |= FLAG_LAST_CHUNK;
            }

            ByteBuffer buffer = ByteBuffer.allocate(CHUNK_HEADER_BYTES + fragmentLen)
                    .order(ByteOrder.LITTLE_ENDIAN);
            buffer.put(CHUNK_MAGIC);
            buffer.put((byte) CHUNK_VERSION);
            buffer.put((byte) flags);
            buffer.putInt(messageId);
            buffer.putShort((short) chunkIndex);
            buffer.putShort((short) chunkCount);
            buffer.putInt(payload.length);
            buffer.put(payload, start, fragmentLen);
            chunks.add(buffer.array());
        }

        return chunks;
    }

    private byte[] reassembleChunk(byte[] chunk) throws Exception {
        if (chunk.length < CHUNK_HEADER_BYTES) {
            throw new Exception("chunk too short");
        }
        ByteBuffer buffer = ByteBuffer.wrap(chunk).order(ByteOrder.LITTLE_ENDIAN);
        if (buffer.get() != CHUNK_MAGIC[0] || buffer.get() != CHUNK_MAGIC[1]) {
            throw new Exception("bad magic");
        }
        int version = Byte.toUnsignedInt(buffer.get());
        if (version != CHUNK_VERSION) {
            throw new Exception("unsupported chunk version " + version);
        }
        int flags = Byte.toUnsignedInt(buffer.get());
        int messageId = buffer.getInt();
        int chunkIndex = Short.toUnsignedInt(buffer.getShort());
        int chunkCount = Short.toUnsignedInt(buffer.getShort());
        int totalLen = buffer.getInt();
        byte[] fragment = new byte[buffer.remaining()];
        buffer.get(fragment);

        if (chunkIndex == 0) {
            partialTx = new PartialMessage(messageId, chunkCount, totalLen);
        }
        if (partialTx == null || !partialTx.matches(messageId, chunkIndex, chunkCount, totalLen)) {
            throw new Exception("out-of-order or mismatched chunk");
        }
        partialTx.append(fragment);

        if ((flags & FLAG_LAST_CHUNK) == 0) {
            return null;
        }

        byte[] completed = partialTx.complete();
        partialTx = null;
        return completed;
    }

    private static class PartialMessage {
        final int messageId;
        final int chunkCount;
        final int totalLen;
        int nextChunkIndex = 0;
        final ByteBuffer payload;

        PartialMessage(int messageId, int chunkCount, int totalLen) {
            this.messageId = messageId;
            this.chunkCount = chunkCount;
            this.totalLen = totalLen;
            this.payload = ByteBuffer.allocate(totalLen);
        }

        boolean matches(int messageId, int chunkIndex, int chunkCount, int totalLen) {
            return this.messageId == messageId
                    && this.nextChunkIndex == chunkIndex
                    && this.chunkCount == chunkCount
                    && this.totalLen == totalLen;
        }

        void append(byte[] fragment) throws Exception {
            if (payload.remaining() < fragment.length) {
                throw new Exception("payload overflow");
            }
            payload.put(fragment);
            nextChunkIndex++;
        }

        byte[] complete() throws Exception {
            if (payload.position() != totalLen) {
                throw new Exception("message length mismatch");
            }
            return payload.array();
        }
    }

    private boolean hasBlePermissions() {
        return hasScanPermission() && hasConnectPermission();
    }

    private boolean hasScanPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            return checkSelfPermission(Manifest.permission.BLUETOOTH_SCAN) == PackageManager.PERMISSION_GRANTED;
        }
        return checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED;
    }

    private boolean hasConnectPermission() {
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.S
                || checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED;
    }

    private void requestNeededPermissions() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            requestPermissions(new String[]{
                    Manifest.permission.BLUETOOTH_SCAN,
                    Manifest.permission.BLUETOOTH_CONNECT
            }, 7);
        } else {
            requestPermissions(new String[]{Manifest.permission.ACCESS_FINE_LOCATION}, 7);
        }
    }

    private String safeDeviceName(BluetoothDevice device) {
        if (!hasConnectPermission()) {
            return "(name unavailable)";
        }
        String name = device.getName();
        return name != null ? name : "(unnamed)";
    }

    private void appendLog(String line) {
        mainHandler.post(() -> {
            logView.append(line + "\n");
            int scrollAmount = logView.getLayout() == null
                    ? 0
                    : logView.getLayout().getLineTop(logView.getLineCount()) - logView.getHeight();
            if (scrollAmount > 0) {
                logView.scrollTo(0, scrollAmount);
            }
        });
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
