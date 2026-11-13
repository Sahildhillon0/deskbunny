package com.zenithratech.nixxa

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.zenithratech.nixxa.link.Link
import io.flutter.FlutterInjector
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.dart.DartExecutor

/**
 * Foreground service (type connectedDevice) that keeps the process, and with it
 * the one BLE connection owned by [Link], alive while the UI is gone.
 * Runs only while a bunny is paired.
 *
 * It also hosts a headless Flutter engine that runs `backgroundMain` (lib/main.dart):
 * the widget scheduler, which keeps the weather, battery, song and other widgets
 * fresh on the bunny and rotates through them when the app is closed.
 */
class NixxaLinkService : Service() {
    private var failed = false
    private var bgEngine: FlutterEngine? = null
    private var bgLink: LinkChannel? = null
    private var bgPhone: PhoneChannel? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        Link.init(this)
        instance = this
        try {
            ServiceCompat.startForeground(
                this, NOTIF_ID, notification(Link.status),
                if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE else 0,
            )
        } catch (e: Exception) {
            // Missing BLUETOOTH_CONNECT, or a background start the OS refused.
            Log.w(TAG, "startForeground failed", e)
            failed = true
            stopSelf()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (failed || !Link.isPaired()) {
            stopSelf()
            return START_NOT_STICKY
        }
        Link.ensureRunning()
        startBackground()
        return START_STICKY
    }

    override fun onDestroy() {
        instance = null
        stopBackground()
        Link.stopLoop()
        super.onDestroy()
    }

    /** Starts the widget scheduler's Dart engine once; a failure only costs auto-update. */
    private fun startBackground() {
        if (bgEngine != null) return
        try {
            val loader = FlutterInjector.instance().flutterLoader()
            loader.startInitialization(applicationContext)
            loader.ensureInitializationComplete(applicationContext, null)
            val engine = FlutterEngine(applicationContext)
            val messenger = engine.dartExecutor.binaryMessenger
            bgLink = LinkChannel(applicationContext, messenger, null, startService = false)
            bgPhone = PhoneChannel(applicationContext, messenger, null)
            engine.dartExecutor.executeDartEntrypoint(
                DartExecutor.DartEntrypoint(loader.findAppBundlePath(), "backgroundMain"),
            )
            bgEngine = engine
            Log.i(TAG, "widget scheduler engine started")
        } catch (e: Exception) {
            Log.w(TAG, "could not start the widget scheduler engine", e)
            stopBackground()
        }
    }

    private fun stopBackground() {
        bgLink?.dispose()
        bgLink = null
        bgPhone?.dispose()
        bgPhone = null
        bgEngine?.destroy()
        bgEngine = null
    }

    private fun notification(st: Link.Status): Notification {
        val nm = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26 && nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, "Bunny connection", NotificationManager.IMPORTANCE_LOW).apply {
                    description = "Shown while nixxa keeps your bunny connected"
                    setShowBadge(false)
                },
            )
        }
        val open = packageManager.getLaunchIntentForPackage(packageName)?.let {
            PendingIntent.getActivity(this, 0, it, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        }
        val text = when (st.state) {
            Link.State.connected -> "nixxa connected"
            Link.State.connecting -> "Connecting to your bunny..."
            Link.State.bluetoothOff -> "Bluetooth is off"
            else -> "Waiting for your bunny"
        }
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_nixxa)
            .setContentTitle(text)
            .setContentText(st.deviceId?.let { "NIXXA-$it" })
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setContentIntent(open)
            .build()
    }

    companion object {
        private const val TAG = "nixxa.service"
        private const val CHANNEL = "link"
        private const val NOTIF_ID = 1
        @Volatile private var instance: NixxaLinkService? = null

        fun start(ctx: Context) {
            try {
                ContextCompat.startForegroundService(ctx, Intent(ctx, NixxaLinkService::class.java))
            } catch (e: Exception) {
                // ForegroundServiceStartNotAllowedException from the background (Android 12+).
                Log.w(TAG, "can't start the service now", e)
            }
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, NixxaLinkService::class.java))
        }

        /** Main thread. */
        fun onStatus(st: Link.Status) {
            val s = instance ?: return
            try {
                s.getSystemService(NotificationManager::class.java).notify(NOTIF_ID, s.notification(st))
            } catch (_: SecurityException) {}
        }
    }
}

/** Restarts the link service after boot, an app update, or Bluetooth turning on. */
class LinkBootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> {}
            android.bluetooth.BluetoothAdapter.ACTION_STATE_CHANGED -> {
                val st = intent.getIntExtra(android.bluetooth.BluetoothAdapter.EXTRA_STATE, -1)
                if (st != android.bluetooth.BluetoothAdapter.STATE_ON) return
            }
            else -> return
        }
        Link.init(ctx)
        if (Link.isPaired()) NixxaLinkService.start(ctx)
    }
}
