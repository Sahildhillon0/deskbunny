package com.zenithratech.nixxa

import android.app.Notification
import android.content.Context
import android.content.pm.ApplicationInfo
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.drawable.Drawable
import android.os.Build
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.telecom.TelecomManager
import android.util.Log
import com.zenithratech.nixxa.link.Link
import com.zenithratech.nixxa.proto.Decision
import com.zenithratech.nixxa.proto.IconPack
import com.zenithratech.nixxa.proto.NotifFilter
import com.zenithratech.nixxa.proto.PostedNotif
import com.zenithratech.nixxa.proto.Proto
import com.zenithratech.nixxa.proto.Translit

/**
 * Forwards the phone's notifications to the bunny (ARCHITECTURE.md "Android
 * link", Notifications). Filtering is in [NotifFilter]; everything here runs on
 * the main thread.
 */
class NixxaNotificationListener : NotificationListenerService() {
    private val filter by lazy { NotifFilter(packageName) }

    override fun onListenerConnected() {
        Link.init(this)
        connected = true
        if (Link.isPaired()) NixxaLinkService.start(this)
        Link.onDndChanged()
    }

    override fun onListenerDisconnected() {
        connected = false
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        try {
            posted(sbn)
        } catch (e: Exception) {
            Log.w(TAG, "notification from ${sbn.packageName} not forwarded", e)
        }
    }

    private fun posted(sbn: StatusBarNotification) {
        Link.init(this)
        if (sbn.packageName != packageName) Link.configs.addSeen(sbn.packageName)
        if (!Link.isReady) return // NOTIFY is dropped while disconnected
        val n = sbn.notification
        val callType = if (Build.VERSION.SDK_INT >= 31) n.extras.getInt(Notification.EXTRA_CALL_TYPE, 0) else 0
        val p = PostedNotif(
            pkg = sbn.packageName,
            key = sbn.key,
            ongoing = sbn.isOngoing,
            groupSummary = (n.flags and Notification.FLAG_GROUP_SUMMARY) != 0,
            isCallCategory = n.category == Notification.CATEGORY_CALL,
            callType = callType,
        )
        val id = NotifFilter.notifId(sbn.key)
        val decision = filter.onPosted(Link.notifConfig(), p, defaultDialer(), System.currentTimeMillis())
        if (n.category == Notification.CATEGORY_CALL) Log.i(TAG, "call from ${sbn.packageName}: $decision")
        when (val d = decision) {
            is Decision.Message -> send(sbn, Proto.KIND_MESSAGE, id, d.mood)
            is Decision.Call -> send(sbn, Proto.KIND_CALL, id, d.mood)
            is Decision.CallEnded -> callEnded(sbn.packageName, id)
            else -> {}
        }
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification) {
        val id = NotifFilter.notifId(sbn.key)
        when (filter.onRemoved(sbn.key)) {
            is Decision.CallEnded -> callEnded(sbn.packageName, id)
            is Decision.Dismiss -> Link.fire(Proto.OP_DISMISS, Proto.dismiss(id))
            else -> {}
        }
    }

    override fun onInterruptionFilterChanged(interruptionFilter: Int) {
        Link.onDndChanged()
    }

    private fun callEnded(pkg: String, id: Int) {
        Link.fire(Proto.OP_NOTIFY, Proto.notify(Proto.KIND_CALL_ENDED, id, 0xFF, pkg, "", "", "", null))
    }

    private fun send(sbn: StatusBarNotification, kind: Int, id: Int, mood: Int) {
        val ex = sbn.notification.extras
        val title = ex.getCharSequence(Notification.EXTRA_TITLE)
            ?: ex.getCharSequence(Notification.EXTRA_TITLE_BIG)
        val body = ex.getCharSequence(Notification.EXTRA_BIG_TEXT)
            ?: ex.getCharSequence(Notification.EXTRA_TEXT)
        val payload = Proto.notify(
            kind = kind, notifId = id, mood = mood, appId = sbn.packageName,
            appName = Translit.field(appLabel(this, sbn.packageName, sbn.notification), 255),
            title = Translit.field(title, 255),
            body = Translit.field(body, Proto.BODY_MAX, keepNl = true),
            icon = logo(sbn),
        )
        Link.fire(Proto.OP_NOTIFY, payload)
    }

    /**
     * The app's logo as a 32x32 1 bpp bitmap: the launcher icon when the app
     * has one (full colour, so it is thresholded by luminance), else the
     * notification's small icon.
     */
    private fun logo(sbn: StatusBarNotification): ByteArray? {
        val d: Drawable = try {
            packageManager.getApplicationIcon(sbn.packageName)
        } catch (_: Exception) {
            sbn.notification.smallIcon?.loadDrawable(this)
        } ?: return null
        return try {
            val bmp = Bitmap.createBitmap(IconPack.SIZE, IconPack.SIZE, Bitmap.Config.ARGB_8888)
            d.setBounds(0, 0, IconPack.SIZE, IconPack.SIZE)
            d.draw(Canvas(bmp))
            val px = IntArray(IconPack.SIZE * IconPack.SIZE)
            bmp.getPixels(px, 0, IconPack.SIZE, 0, 0, IconPack.SIZE, IconPack.SIZE)
            bmp.recycle()
            IconPack.fromPixels(px).takeIf { IconPack.usable(it) }
        } catch (e: Exception) {
            null
        }
    }

    private fun defaultDialer(): String? = try {
        getSystemService(TelecomManager::class.java)?.defaultDialerPackage
    } catch (_: Exception) {
        null
    }

    companion object {
        private const val TAG = "nixxa.listener"
        @Volatile var connected = false
            private set

        fun appLabel(ctx: Context, pkg: String, n: Notification? = null): String = try {
            val pm = ctx.packageManager
            // The notification carries its app's ApplicationInfo, which works even
            // when package visibility hides the app from us.
            @Suppress("DEPRECATION")
            val ai = n?.extras?.getParcelable<ApplicationInfo>("android.appInfo")
                ?: pm.getApplicationInfo(pkg, 0)
            pm.getApplicationLabel(ai).toString()
        } catch (_: Exception) {
            pkg
        }
    }
}
