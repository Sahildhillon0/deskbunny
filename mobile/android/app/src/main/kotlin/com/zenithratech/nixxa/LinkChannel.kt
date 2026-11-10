package com.zenithratech.nixxa

import android.content.ComponentName
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.core.app.NotificationManagerCompat
import com.zenithratech.nixxa.link.Link
import com.zenithratech.nixxa.proto.LinkError
import com.zenithratech.nixxa.proto.NotifConfig
import com.zenithratech.nixxa.proto.Proto
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

/** MethodChannel `nixxa/link` + EventChannel `nixxa/link/status` (ARCHITECTURE.md channel table). */
class LinkChannel(private val activity: MainActivity, messenger: BinaryMessenger) : MethodChannel.MethodCallHandler {
    private val ctx = activity.applicationContext
    private val methods = MethodChannel(messenger, "nixxa/link")
    private val events = EventChannel(messenger, "nixxa/link/status")
    private var statusListener: ((Link.Status) -> Unit)? = null

    init {
        Link.init(ctx)
        methods.setMethodCallHandler(this)
        events.setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(arguments: Any?, sink: EventChannel.EventSink) {
                val l: (Link.Status) -> Unit = { sink.success(it.toMap()) }
                statusListener?.let { Link.removeListener(it) }
                statusListener = l
                Link.addListener(l)
                sink.success(Link.status.toMap())
            }

            override fun onCancel(arguments: Any?) {
                statusListener?.let { Link.removeListener(it) }
                statusListener = null
            }
        })
        if (Link.isPaired()) NixxaLinkService.start(ctx)
    }

    fun dispose() {
        statusListener?.let { Link.removeListener(it) }
        statusListener = null
        methods.setMethodCallHandler(null)
        events.setStreamHandler(null)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "status" -> result.success(Link.status.toMap())
            "pair" -> {
                val qr = call.argument<String>("qr") ?: return result.error("BAD_QR", "Missing QR text", null)
                activity.requestLinkPermissions { granted ->
                    if (!granted) {
                        result.error("PERMISSION", "Nearby devices (Bluetooth) permission is needed to pair", null)
                    } else {
                        Link.pair(qr) { r -> r.fold({ result.success(it.toMap()) }, { fail(result, it) }) }
                    }
                }
            }
            "forget" -> Link.forget { result.success(null) }
            "request" -> {
                val op = call.argument<Int>("op") ?: return result.error("DEVICE_ERROR", "Missing op", Proto.ST_BAD_MSG)
                val payload = call.argument<ByteArray>("payload") ?: ByteArray(0)
                Link.request(op, payload) { r -> r.fold({ result.success(it) }, { fail(result, it) }) }
            }
            "getClock" -> result.success(Link.clockStyle())
            "setClock" -> {
                val style = call.argument<Int>("style") ?: return result.error("DEVICE_ERROR", "Missing style", Proto.ST_BAD_MSG)
                Link.setClock(style) { result.success(null) }
            }
            "getNotifConfig" -> result.success(Link.notifConfig().toMap())
            "setNotifConfig" -> {
                val cfg = try {
                    NotifConfig.fromMap(call.arguments as Map<*, *>)
                } catch (e: Exception) {
                    return result.error("DEVICE_ERROR", "Bad config: ${e.message}", Proto.ST_BAD_MSG)
                }
                Link.setNotifConfig(cfg) { result.success(null) }
            }
            "hasNotificationAccess" ->
                result.success(NotificationManagerCompat.getEnabledListenerPackages(ctx).contains(ctx.packageName))
            "openNotificationAccessSettings" -> {
                val i = if (Build.VERSION.SDK_INT >= 30) {
                    Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS).putExtra(
                        Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME,
                        ComponentName(ctx, NixxaNotificationListener::class.java).flattenToString(),
                    )
                } else {
                    Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
                }
                try {
                    activity.startActivity(i)
                } catch (_: Exception) {
                    activity.startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
                }
                result.success(null)
            }
            "requestBatteryExemption" -> {
                val pm = ctx.getSystemService(PowerManager::class.java)
                if (!pm.isIgnoringBatteryOptimizations(ctx.packageName)) {
                    try {
                        @Suppress("BatteryLife")
                        activity.startActivity(
                            Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${ctx.packageName}")),
                        )
                    } catch (_: Exception) {
                        activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
                    }
                }
                result.success(null)
            }
            "listApps" -> io.execute {
                val apps = try { listApps() } catch (e: Exception) { emptyList() }
                activity.runOnUiThread { result.success(apps) }
            }
            "sendTestNotification" -> {
                val payload = Proto.notify(
                    Proto.KIND_MESSAGE, 0x6E697861, 0xFF, ctx.packageName, "nixxa",
                    "Hello from nixxa", "Notifications reach your bunny. This is what a card looks like.", null,
                )
                Link.request(Proto.OP_NOTIFY, payload) { r -> r.fold({ result.success(null) }, { fail(result, it) }) }
            }
            else -> result.notImplemented()
        }
    }

    private fun fail(result: MethodChannel.Result, e: Throwable) {
        if (e is LinkError) result.error(e.code, e.message, e.deviceStatus)
        else result.error("NOT_CONNECTED", e.message ?: e.toString(), null)
    }

    /** Launcher apps plus apps seen posting, sorted by label, 48x48 PNG icons. */
    private fun listApps(): List<Map<String, Any?>> {
        val pm = ctx.packageManager
        val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        @Suppress("DEPRECATION")
        val pkgs = pm.queryIntentActivities(launcher, 0).map { it.activityInfo.packageName }.toMutableSet()
        pkgs += Link.configs.seen()
        pkgs -= ctx.packageName
        return pkgs.mapNotNull { pkg ->
            try {
                val ai = pm.getApplicationInfo(pkg, 0)
                mapOf("appId" to pkg, "label" to pm.getApplicationLabel(ai).toString(), "iconPng" to iconPng(pm.getApplicationIcon(ai)))
            } catch (_: Exception) {
                null
            }
        }.sortedBy { (it["label"] as String).lowercase() }
    }

    private fun iconPng(d: android.graphics.drawable.Drawable): ByteArray? = try {
        val bmp = Bitmap.createBitmap(48, 48, Bitmap.Config.ARGB_8888)
        d.setBounds(0, 0, 48, 48)
        d.draw(Canvas(bmp))
        ByteArrayOutputStream().use { o -> bmp.compress(Bitmap.CompressFormat.PNG, 100, o); bmp.recycle(); o.toByteArray() }
    } catch (_: Exception) {
        null
    }

    companion object {
        private val io = Executors.newSingleThreadExecutor()
    }
}
