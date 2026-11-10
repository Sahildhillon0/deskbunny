// The phone <-> bunny link: pairing, reconnect, message queue, status.
// Process-wide singleton used by NixxaLinkService (which keeps the process and
// the connection alive), the notification listener and the platform channels.
// Everything runs on one HandlerThread ("link thread").
package com.zenithratech.nixxa.link

import android.annotation.SuppressLint
import android.app.NotificationManager
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.ParcelUuid
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import com.zenithratech.nixxa.NixxaLinkService
import com.zenithratech.nixxa.proto.Info
import com.zenithratech.nixxa.proto.LinkError
import com.zenithratech.nixxa.proto.MessageQueue
import com.zenithratech.nixxa.proto.NotifConfig
import com.zenithratech.nixxa.proto.PairingQr
import com.zenithratech.nixxa.proto.Proto
import com.zenithratech.nixxa.proto.Reassembler
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.android.asCoroutineDispatcher
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import java.util.TimeZone
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.coroutines.resume

@SuppressLint("MissingPermission", "StaticFieldLeak")
object Link {
    private const val TAG = "nixxa.link"

    enum class State { bluetoothOff, unpaired, disconnected, connecting, connected }

    data class Status(val state: State, val deviceId: String?, val fwVersion: String?, val chip: String?) {
        fun toMap(): Map<String, Any?> =
            mapOf("state" to state.name, "deviceId" to deviceId, "fwVersion" to fwVersion, "chip" to chip)
    }

    private lateinit var app: Context
    private lateinit var pairings: PairingStore
    lateinit var configs: ConfigStore
        private set

    private val thread = HandlerThread("nixxa-link").apply { start() }
    private val h = Handler(thread.looper)
    private val main = Handler(Looper.getMainLooper())
    private val scope = CoroutineScope(SupervisorJob() + h.asCoroutineDispatcher())

    private val listeners = CopyOnWriteArrayList<(Status) -> Unit>()
    @Volatile var status = Status(State.unpaired, null, null, null)
        private set
    @Volatile private var config = NotifConfig()
    /** Clock style to show on the bunny: 0 = the face, 1..20 = that clock. */
    @Volatile private var clock = 0

    // link-thread state
    private var session: GattSession? = null
    private var queue: MessageQueue? = null
    private val reasm = Reassembler()
    private var loopJob: Job? = null
    private var pairJob: Job? = null
    private var lastDnd: Boolean? = null
    /** False once this connection's bunny rejected the SET_NOTIF_CONFIG ext byte (old firmware). */
    private var notifExtOk = true
    private var fw: String? = null
    private var chip: String? = null

    /** Ready = connected and authenticated; NOTIFY is dropped otherwise. */
    val isReady: Boolean get() = status.state == State.connected

    private var inited = false

    @Synchronized
    fun init(ctx: Context) {
        if (inited) return
        inited = true
        app = ctx.applicationContext
        pairings = PairingStore(app)
        configs = ConfigStore(app)
        config = configs.load()
        clock = configs.clockStyle()
        val f = IntentFilter().apply {
            addAction(BluetoothAdapter.ACTION_STATE_CHANGED)
            addAction(NotificationManager.ACTION_INTERRUPTION_FILTER_CHANGED)
            addAction(Intent.ACTION_TIMEZONE_CHANGED)
            addAction(Intent.ACTION_TIME_CHANGED)
        }
        // System (protected) broadcasts only.
        ContextCompat.registerReceiver(app, receiver, f, ContextCompat.RECEIVER_EXPORTED)
        h.post { publish() }
    }

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, i: Intent) {
            when (i.action) {
                BluetoothAdapter.ACTION_STATE_CHANGED -> {
                    val st = i.getIntExtra(BluetoothAdapter.EXTRA_STATE, -1)
                    h.post {
                        if (st == BluetoothAdapter.STATE_ON) {
                            publish()
                            if (pairing() != null) NixxaLinkService.start(app)
                            ensureLoop()
                        } else if (st == BluetoothAdapter.STATE_OFF || st == BluetoothAdapter.STATE_TURNING_OFF) {
                            loopJob?.cancel(); loopJob = null
                            teardown()
                            publish()
                        }
                    }
                }
                NotificationManager.ACTION_INTERRUPTION_FILTER_CHANGED -> onDndChanged()
                Intent.ACTION_TIMEZONE_CHANGED, Intent.ACTION_TIME_CHANGED -> h.post {
                    if (isReady) fire(Proto.OP_SET_TIME, timePayload())
                }
            }
        }
    }

    // ---- status ---------------------------------------------------------------

    fun addListener(l: (Status) -> Unit) { listeners += l }
    fun removeListener(l: (Status) -> Unit) { listeners -= l }

    private fun adapter(): BluetoothAdapter? =
        (app.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

    private fun btOn(): Boolean = try { adapter()?.isEnabled == true } catch (_: SecurityException) { false }

    @Volatile private var cached: Pairing? = null
    @Volatile private var cacheLoaded = false

    /** The stored pairing (decrypted once, then cached). */
    @Synchronized
    private fun pairing(): Pairing? {
        if (!cacheLoaded) { cached = pairings.load(); cacheLoaded = true }
        return cached
    }

    @Synchronized
    private fun savePairing(p: Pairing?) {
        if (p == null) pairings.clear() else pairings.save(p)
        cached = p
        cacheLoaded = true
    }

    fun isPaired(): Boolean = pairing() != null

    /** Recompute the status from the link state; link thread. */
    private fun publish(connecting: Boolean = false) {
        val p = pairing()
        val s = session
        val state = when {
            !btOn() -> State.bluetoothOff
            pairJob?.isActive == true -> State.connecting
            p == null -> State.unpaired
            s != null && s.ready && s.isUp -> State.connected
            connecting || (s != null && s.isUp) -> State.connecting
            else -> State.disconnected
        }
        val up = state == State.connected
        val st = Status(state, p?.deviceId, if (up) fw else null, if (up) chip else null)
        if (st == status) return
        status = st
        Log.i(TAG, "status $st")
        main.post {
            for (l in listeners) l(st)
            NixxaLinkService.onStatus(st)
        }
    }

    // ---- public API (any thread; callbacks on the main thread) ----------------

    fun pair(qrText: String, cb: (Result<Status>) -> Unit) {
        val qr = PairingQr.parse(qrText)
        if (qr == null) {
            main.post { cb(Result.failure(LinkError("BAD_QR", "This is not a nixxa QR code"))) }
            return
        }
        h.post {
            pairJob?.cancel()
            pairJob = scope.launch {
                val me = coroutineContext[Job]
                val r: Result<Unit> = try {
                    Result.success(doPair(qr))
                } catch (e: CancellationException) {
                    Result.failure(LinkError("NOT_CONNECTED", "Pairing cancelled"))
                } catch (e: LinkError) {
                    Result.failure(e)
                } catch (e: SecurityException) {
                    Result.failure(LinkError("PERMISSION", "Bluetooth permission missing"))
                } catch (e: Exception) {
                    Log.e(TAG, "pair failed", e)
                    Result.failure(LinkError("NOT_CONNECTED", e.message ?: e.toString()))
                }
                if (r.isFailure && pairJob === me) teardown()
                if (pairJob === me) pairJob = null
                publish()
                ensureLoop()
                val st = status
                main.post { cb(r.map { st }) }
            }
        }
    }

    fun forget(cb: (Result<Unit>) -> Unit) {
        h.post {
            pairJob?.cancel()
            scope.launch {
                if (isReady) {
                    try { withTimeout(3000) { req(Proto.OP_UNPAIR, ByteArray(0)) } } catch (_: Exception) {}
                }
                forgetLocal()
                main.post { cb(Result.success(Unit)) }
            }
        }
    }

    fun request(op: Int, payload: ByteArray, cb: (Result<ByteArray>) -> Unit) {
        h.post {
            val q = queue
            if (!isReady || q == null) {
                main.post { cb(Result.failure(LinkError("NOT_CONNECTED", "Not connected to the bunny"))) }
                return@post
            }
            q.enqueue(op, payload) { r -> main.post { cb(r) } }
        }
    }

    /** From the notification listener: fire and forget, dropped while disconnected. */
    fun fire(op: Int, payload: ByteArray) {
        h.post {
            val q = queue ?: return@post
            if (!isReady) return@post
            q.enqueue(op, payload) { r -> r.exceptionOrNull()?.let { Log.w(TAG, "op 0x%02x: %s".format(op, it.message)) } }
        }
    }

    fun notifConfig(): NotifConfig = config

    fun clockStyle(): Int = clock

    /** Persists the clock style and sends SET_CLOCK now (if connected) and on every connect. */
    fun setClock(style: Int, cb: (Result<Unit>) -> Unit) {
        val s = style.coerceIn(0, 20)
        clock = s
        configs.saveClockStyle(s)
        h.post {
            if (isReady) fire(Proto.OP_SET_CLOCK, Proto.setClock(s))
            main.post { cb(Result.success(Unit)) }
        }
    }

    fun setNotifConfig(c: NotifConfig, cb: (Result<Unit>) -> Unit) {
        config = c
        configs.save(c)
        h.post {
            if (isReady) {
                fire(Proto.OP_SET_NOTIF_CONFIG, Proto.notifConfig(c, notifExtOk))
                lastDnd = null
                onDndChanged()
            }
            main.post { cb(Result.success(Unit)) }
        }
    }

    fun onDndChanged() {
        h.post {
            if (!isReady || !config.followDnd) return@post
            val on = dndOn()
            if (on == lastDnd) return@post
            lastDnd = on
            fire(Proto.OP_SET_DND, Proto.setDnd(on))
        }
    }

    /** Called by the service: connect (and keep reconnecting) when paired. */
    fun ensureRunning() { h.post { publish(); ensureLoop() } }

    /** Called when the service is destroyed. */
    fun stopLoop() {
        h.post {
            if (pairJob?.isActive == true) return@post
            loopJob?.cancel(); loopJob = null
            teardown()
            publish()
        }
    }

    // ---- link thread ------------------------------------------------------------

    private fun dndOn(): Boolean {
        val nm = app.getSystemService(NotificationManager::class.java)
        val f = nm.currentInterruptionFilter
        return f != NotificationManager.INTERRUPTION_FILTER_ALL && f != NotificationManager.INTERRUPTION_FILTER_UNKNOWN
    }

    private fun timePayload(): ByteArray {
        val now = System.currentTimeMillis()
        return Proto.setTime(now / 1000, TimeZone.getDefault().getOffset(now) / 60000)
    }

    private fun teardown() {
        queue?.close()
        queue = null
        session?.close()
        session = null
        reasm.reset()
    }

    private fun forgetLocal() {
        loopJob?.cancel(); loopJob = null
        val dev = session?.device ?: pairing()?.let { p ->
            try { adapter()?.getRemoteDevice(p.address) } catch (_: Exception) { null }
        }
        teardown()
        dev?.let { removeBond(it) }
        savePairing(null)
        fw = null; chip = null
        publish()
        NixxaLinkService.stop(app)
    }

    private fun removeBond(d: BluetoothDevice) {
        // Hidden API (greylisted); the user can still remove it in Bluetooth settings if this fails.
        try {
            if (d.bondState != BluetoothDevice.BOND_NONE) d.javaClass.getMethod("removeBond").invoke(d)
        } catch (e: Exception) {
            Log.w(TAG, "removeBond failed", e)
        }
    }

    private fun newSession(d: BluetoothDevice): GattSession {
        teardown()
        val s = GattSession(app, d, h) { bytes ->
            for (m in reasm.feed(bytes, SystemClock.elapsedRealtime())) queue?.onMessage(m)
        }
        session = s
        val q = MessageQueue(
            sched = { ms, r -> val run = Runnable { r() }; h.postDelayed(run, ms); { h.removeCallbacks(run) } },
            transport = { frame, done ->
                scope.launch {
                    try {
                        val c = s.rx ?: throw LinkError("NOT_CONNECTED", "no RX")
                        for (chunk in Proto.chunk(frame, s.chunkSize)) s.write(c, chunk)
                        done(true)
                    } catch (e: CancellationException) {
                        done(false); throw e
                    } catch (e: Exception) {
                        Log.w(TAG, "write failed: ${e.message}")
                        done(false)
                    }
                }
            },
        )
        queue = q
        scope.launch {
            val st = s.disconnected.await()
            if (session === s) {
                Log.i(TAG, "link dropped ($st)")
                q.close()
                queue = null
                session = null
                reasm.reset()
                publish()
            }
        }
        return s
    }

    private suspend fun req(op: Int, payload: ByteArray): ByteArray = suspendCancellableCoroutine { c ->
        val q = queue
        if (q == null) {
            c.resumeWith(Result.failure(LinkError("NOT_CONNECTED", "Not connected")))
        } else {
            q.enqueue(op, payload) { r -> if (c.isActive) c.resumeWith(r) }
        }
    }

    /** MTU, discovery, notifications, INFO, AUTH, SET_TIME, SET_NOTIF_CONFIG, SET_DND. */
    private suspend fun setup(s: GattSession, key: ByteArray, expectedId: String) {
        publish(connecting = true)
        s.requestMtu(Proto.MTU_WANTED)
        s.discover()
        s.enableNotifications(s.tx!!)
        val info = Info.parse(s.read(s.info!!))
            ?: throw LinkError("DEVICE_ERROR", "Bad INFO from the bunny", Proto.ST_BAD_MSG)
        if (info.proto != Proto.PROTO_VERSION) {
            throw LinkError("DEVICE_ERROR", "Unsupported protocol ${info.proto}; update the app or firmware", Proto.ST_BAD_MSG)
        }
        if (info.deviceId != expectedId) throw LinkError("NOT_FOUND", "Found ${info.deviceId}, expected $expectedId")
        queue!!.open()
        reasm.reset()
        try {
            req(Proto.OP_AUTH, Proto.authTag(key, info.nonce))
        } catch (e: LinkError) {
            if (e.deviceStatus == Proto.ST_AUTH_FAILED) {
                throw LinkError("AUTH_FAILED", "The bunny rejected the key; scan its QR code again")
            }
            throw e
        }
        req(Proto.OP_SET_TIME, timePayload())
        try {
            req(Proto.OP_SET_CLOCK, Proto.setClock(clock))
        } catch (e: LinkError) {
            // Firmware before clocks answers BAD_MSG: the bunny simply has no clocks.
            if (e.deviceStatus != Proto.ST_BAD_MSG) throw e
            Log.i(TAG, "bunny firmware predates clocks; SET_CLOCK ignored")
        }
        notifExtOk = true
        try {
            req(Proto.OP_SET_NOTIF_CONFIG, Proto.notifConfig(config, true))
        } catch (e: LinkError) {
            if (e.deviceStatus != Proto.ST_BAD_MSG) throw e
            Log.i(TAG, "bunny firmware predates the notif-config ext byte; resending without it")
            notifExtOk = false
            req(Proto.OP_SET_NOTIF_CONFIG, Proto.notifConfig(config, false))
        }
        lastDnd = null
        if (config.followDnd) {
            val on = dndOn()
            req(Proto.OP_SET_DND, Proto.setDnd(on))
            lastDnd = on
        }
        fw = info.fwVersion
        chip = info.chipName
        s.ready = true
        publish()
    }

    private suspend fun doPair(qr: PairingQr) {
        loopJob?.cancel(); loopJob = null
        teardown()
        publish(connecting = true)
        if (!btOn()) throw LinkError("BT_OFF", "Bluetooth is off")
        val dev = scan("NIXXA-${qr.deviceId}", 15_000)
        if (dev.bondState == BluetoothDevice.BOND_BONDED) {
            // A stale bond from before a reflash would make encryption fail.
            removeBond(dev)
            waitBond(dev, 5_000) { it == BluetoothDevice.BOND_NONE }
        }
        val s = newSession(dev)
        s.connect(autoConnect = false)
        publish(connecting = true)
        bond(dev)
        setup(s, qr.key, qr.deviceId)
        val old = pairing()
        if (old != null && old.address != dev.address) {
            try { adapter()?.getRemoteDevice(old.address)?.let { removeBond(it) } } catch (_: Exception) {}
        }
        savePairing(Pairing(dev.address, qr.deviceId, qr.key))
        NixxaLinkService.start(app)
    }

    private suspend fun bond(d: BluetoothDevice) {
        if (d.bondState == BluetoothDevice.BOND_BONDED) return
        if (d.bondState != BluetoothDevice.BOND_BONDING && !d.createBond()) {
            // The device may have started pairing itself a moment ago.
            if (d.bondState == BluetoothDevice.BOND_NONE) throw LinkError("BOND_FAILED", "Bluetooth pairing could not start")
        }
        var sawBonding = false
        val ok = waitBond(d, 60_000) {
            if (it == BluetoothDevice.BOND_BONDING) sawBonding = true
            it == BluetoothDevice.BOND_BONDED || (sawBonding && it == BluetoothDevice.BOND_NONE)
        }
        if (!ok || d.bondState != BluetoothDevice.BOND_BONDED) {
            throw LinkError("BOND_FAILED", "Bluetooth pairing failed or was declined")
        }
    }

    /** Poll the bond state (no receiver needed); true when [done] matched in time. */
    private suspend fun waitBond(d: BluetoothDevice, ms: Long, done: (Int) -> Boolean): Boolean {
        val end = SystemClock.elapsedRealtime() + ms
        while (SystemClock.elapsedRealtime() < end) {
            if (done(d.bondState)) return true
            session?.let { if (it.disconnected.isCompleted) throw LinkError("BOND_FAILED", "The bunny disconnected during pairing") }
            delay(250)
        }
        return false
    }

    private suspend fun scan(name: String, ms: Long): BluetoothDevice {
        val scanner = adapter()?.bluetoothLeScanner ?: throw LinkError("BT_OFF", "Bluetooth is off")
        var cb: ScanCallback? = null
        try {
            return withTimeout(ms) {
                suspendCancellableCoroutine { c ->
                    val callback = object : ScanCallback() {
                        override fun onScanResult(type: Int, r: ScanResult) {
                            val n = r.scanRecord?.deviceName ?: try { r.device.name } catch (_: SecurityException) { null }
                            if (n == name) h.post { if (c.isActive) c.resume(r.device) }
                        }

                        override fun onScanFailed(errorCode: Int) {
                            h.post {
                                if (c.isActive) c.resumeWith(Result.failure(LinkError("NOT_FOUND", "Bluetooth scan failed ($errorCode)")))
                            }
                        }
                    }
                    cb = callback
                    val filter = ScanFilter.Builder().setServiceUuid(ParcelUuid.fromString(Proto.SERVICE_UUID)).build()
                    val settings = ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build()
                    scanner.startScan(listOf(filter), settings, callback)
                }
            }
        } catch (e: TimeoutCancellationException) {
            throw LinkError("NOT_FOUND", "No bunny named $name nearby. Is it on and showing its QR code?")
        } finally {
            cb?.let { try { scanner.stopScan(it) } catch (_: Exception) {} }
        }
    }

    private fun ensureLoop() {
        if (loopJob?.isActive == true || pairJob?.isActive == true) return
        if (pairing() == null || !btOn()) return
        loopJob = scope.launch { linkLoop() }
    }

    /**
     * Encryption can't start because this phone still holds a bond the bunny
     * dropped (for example after a power cut while bonded). Remove the stale
     * bond, bond again on a fresh link and set up on the new bond. Returns the
     * new session.
     */
    private suspend fun repairBond(dev: BluetoothDevice, p: Pairing): GattSession {
        Log.w(TAG, "encryption failed: the bond is stale, bonding again")
        teardown()
        removeBond(dev)
        waitBond(dev, 5_000) { it == BluetoothDevice.BOND_NONE }
        val s = newSession(dev)
        s.connect(autoConnect = true)
        bond(dev)
        setup(s, p.key, p.deviceId)
        return s
    }

    /** Keep the paired bunny connected: autoConnect, set up, wait for the drop, repeat. */
    private suspend fun linkLoop() {
        var backoff = 1_000L
        while (true) {
            val p = pairing() ?: return
            if (!btOn()) { publish(); return }
            var s = session
            if (s == null || !s.isUp || !s.ready) {
                publish()
                try {
                    val dev = adapter()?.getRemoteDevice(p.address) ?: return
                    s = newSession(dev)
                    s.connect(autoConnect = true)
                    try {
                        setup(s, p.key, p.deviceId)
                    } catch (e: LinkError) {
                        if (e.code != "BOND_FAILED") throw e
                        s = repairBond(dev, p) // the bunny no longer knows our keys
                    }
                    backoff = 1_000L
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    Log.w(TAG, "reconnect failed: ${e.message}")
                    teardown()
                    if (e is LinkError && e.code == "AUTH_FAILED") {
                        // The bunny was reflashed or unpaired elsewhere: K is gone for good.
                        forgetLocal()
                        return
                    }
                    publish()
                    delay(backoff)
                    backoff = minOf(backoff * 2, 30_000L)
                    continue
                }
            }
            s.disconnected.await()
            publish()
            delay(1_000)
        }
    }
}
