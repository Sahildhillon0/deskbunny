// One BLE GATT connection with strictly serialised GATT operations (Android
// allows one outstanding op per connection). All state lives on the link
// thread; Binder callbacks are posted there.
package com.zenithratech.nixxa.link

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.content.Context
import android.os.Build
import android.os.Handler
import android.util.Log
import com.zenithratech.nixxa.proto.LinkError
import com.zenithratech.nixxa.proto.Proto
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeout
import java.util.UUID

@SuppressLint("MissingPermission")
class GattSession(
    private val ctx: Context,
    val device: BluetoothDevice,
    private val h: Handler,
    private val onNotify: (ByteArray) -> Unit,
) {
    private var gatt: BluetoothGatt? = null
    private val connectedD = CompletableDeferred<Unit>()
    /** Completes with the GATT status when the link drops (or close() is called). */
    val disconnected = CompletableDeferred<Int>()
    private val opLock = Mutex()
    private var pending: CompletableDeferred<ByteArray?>? = null

    var mtu = 23
        private set
    var rx: BluetoothGattCharacteristic? = null
        private set
    var tx: BluetoothGattCharacteristic? = null
        private set
    var info: BluetoothGattCharacteristic? = null
        private set

    /** Set once INFO, AUTH and the setup messages are done. */
    var ready = false

    val isUp: Boolean get() = connectedD.isCompleted && !disconnected.isCompleted

    private val cb = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
            h.post {
                Log.i(TAG, "connection state $newState status $status")
                if (newState == BluetoothProfile.STATE_CONNECTED && status == BluetoothGatt.GATT_SUCCESS) {
                    connectedD.complete(Unit)
                } else if (newState == BluetoothProfile.STATE_DISCONNECTED || status != BluetoothGatt.GATT_SUCCESS) {
                    drop(status)
                }
            }
        }

        override fun onMtuChanged(g: BluetoothGatt, m: Int, status: Int) {
            h.post {
                if (status == BluetoothGatt.GATT_SUCCESS) mtu = m
                complete(status, null)
            }
        }

        override fun onServicesDiscovered(g: BluetoothGatt, status: Int) {
            h.post { complete(status, null) }
        }

        override fun onCharacteristicRead(g: BluetoothGatt, c: BluetoothGattCharacteristic, value: ByteArray, status: Int) {
            h.post { complete(status, value) }
        }

        @Deprecated("Deprecated in Java")
        @Suppress("DEPRECATION")
        override fun onCharacteristicRead(g: BluetoothGatt, c: BluetoothGattCharacteristic, status: Int) {
            if (Build.VERSION.SDK_INT >= 33) return
            val v = c.value?.clone()
            h.post { complete(status, v) }
        }

        override fun onCharacteristicWrite(g: BluetoothGatt, c: BluetoothGattCharacteristic, status: Int) {
            h.post { complete(status, null) }
        }

        override fun onDescriptorWrite(g: BluetoothGatt, d: BluetoothGattDescriptor, status: Int) {
            h.post { complete(status, null) }
        }

        override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic, value: ByteArray) {
            val v = value.clone()
            h.post { onNotify(v) }
        }

        @Deprecated("Deprecated in Java")
        @Suppress("DEPRECATION")
        override fun onCharacteristicChanged(g: BluetoothGatt, c: BluetoothGattCharacteristic) {
            if (Build.VERSION.SDK_INT >= 33) return
            val v = c.value?.clone() ?: return
            h.post { onNotify(v) }
        }
    }

    private fun complete(status: Int, value: ByteArray?) {
        val p = pending ?: return
        pending = null
        if (status == BluetoothGatt.GATT_SUCCESS) p.complete(value)
        else p.completeExceptionally(LinkError(gattErrorCode(status), "GATT error $status"))
    }

    private fun drop(status: Int) {
        if (disconnected.isCompleted) return
        disconnected.complete(status)
        ready = false
        val err = LinkError("NOT_CONNECTED", "Disconnected from the bunny (status $status)")
        connectedD.completeExceptionally(err)
        pending?.completeExceptionally(err)
        pending = null
        try { gatt?.close() } catch (_: Exception) {}
        gatt = null
    }

    /** Connect; with autoConnect the wait has no time limit (the bunny may be off). */
    suspend fun connect(autoConnect: Boolean, timeoutMs: Long = 20_000) {
        gatt = device.connectGatt(ctx, autoConnect, cb, BluetoothDevice.TRANSPORT_LE)
            ?: throw LinkError("NOT_CONNECTED", "connectGatt failed")
        if (autoConnect) {
            connectedD.await()
        } else {
            try {
                withTimeout(timeoutMs) { connectedD.await() }
            } catch (e: TimeoutCancellationException) {
                close()
                throw LinkError("NOT_FOUND", "Couldn't connect to the bunny")
            }
        }
    }

    /** Call on the link thread. */
    fun close() {
        try { gatt?.disconnect() } catch (_: Exception) {}
        drop(-1) // also closes the handle
    }

    /** Run one GATT op: start it, then wait for its callback (or the link to drop). */
    private suspend fun op(name: String, timeoutMs: Long = 10_000, start: (BluetoothGatt) -> Boolean): ByteArray? =
        opLock.withLock {
            val g = gatt ?: throw LinkError("NOT_CONNECTED", "Not connected")
            if (disconnected.isCompleted) throw LinkError("NOT_CONNECTED", "Not connected")
            val d = CompletableDeferred<ByteArray?>()
            pending = d
            val started = try { start(g) } catch (e: SecurityException) {
                pending = null
                throw LinkError("PERMISSION", "Bluetooth permission missing")
            }
            if (!started) {
                pending = null
                throw LinkError("NOT_CONNECTED", "GATT $name could not start")
            }
            try {
                withTimeout(timeoutMs) { d.await() }
            } catch (e: TimeoutCancellationException) {
                if (pending === d) pending = null
                throw LinkError("TIMEOUT", "GATT $name timed out")
            }
        }

    suspend fun requestMtu(m: Int) {
        try {
            op("mtu", 5_000) { it.requestMtu(m) }
        } catch (e: LinkError) {
            if (disconnected.isCompleted) throw e
            Log.w(TAG, "MTU request failed (${e.message}); using $mtu")
        }
    }

    suspend fun discover() {
        op("discover", 15_000) { it.discoverServices() }
        val s = gatt?.getService(UUID.fromString(Proto.SERVICE_UUID))
            ?: throw LinkError("NOT_FOUND", "This device is not a nixxa bunny")
        rx = s.getCharacteristic(UUID.fromString(Proto.RX_UUID))
        tx = s.getCharacteristic(UUID.fromString(Proto.TX_UUID))
        info = s.getCharacteristic(UUID.fromString(Proto.INFO_UUID))
        if (rx == null || tx == null || info == null) throw LinkError("NOT_FOUND", "The bunny's GATT service is incomplete")
    }

    @Suppress("DEPRECATION")
    suspend fun enableNotifications(c: BluetoothGattCharacteristic) {
        val g = gatt ?: throw LinkError("NOT_CONNECTED", "Not connected")
        if (!g.setCharacteristicNotification(c, true)) throw LinkError("NOT_CONNECTED", "setCharacteristicNotification failed")
        val d = c.getDescriptor(UUID.fromString(Proto.CCCD_UUID)) ?: return
        val v = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
        // Writing the CCCD of an encrypted characteristic may trigger pairing; allow time for the dialog.
        op("cccd", 60_000) {
            if (Build.VERSION.SDK_INT >= 33) it.writeDescriptor(d, v) == BluetoothStatusCodes.SUCCESS
            else { d.value = v; it.writeDescriptor(d) }
        }
    }

    @Suppress("DEPRECATION")
    suspend fun read(c: BluetoothGattCharacteristic): ByteArray =
        op("read", 60_000) { it.readCharacteristic(c) } ?: ByteArray(0)

    @Suppress("DEPRECATION")
    suspend fun write(c: BluetoothGattCharacteristic, v: ByteArray) {
        op("write") {
            if (Build.VERSION.SDK_INT >= 33) {
                it.writeCharacteristic(c, v, BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT) == BluetoothStatusCodes.SUCCESS
            } else {
                c.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                c.value = v
                it.writeCharacteristic(c)
            }
        }
    }

    /** Largest write: MTU-3, but an attribute value can't exceed 512 bytes. */
    val chunkSize: Int get() = minOf(mtu - 3, 512).coerceAtLeast(20)

    companion object {
        private const val TAG = "nixxa.gatt"

        fun gattErrorCode(status: Int) = when (status) {
            5, 8, 15, 137 -> "BOND_FAILED" // insufficient authentication / encryption
            else -> "NOT_CONNECTED"
        }
    }
}
