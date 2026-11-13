package com.zenithratech.nixxa

import android.Manifest
import android.content.ComponentName
import android.content.ContentUris
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.BatteryManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.CalendarContract
import android.util.Log
import androidx.core.content.ContextCompat
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.Calendar
import java.util.TimeZone
import java.util.concurrent.Executors

/**
 * MethodChannel `nixxa/phone` + EventChannel `nixxa/phone/nowplaying`: what the
 * phone knows that the widgets show (battery, next calendar event, the song).
 * Lives in both Flutter engines, the app's and the background one the link
 * service hosts, so [activity] is null in the background.
 */
class PhoneChannel(
    private val ctx: Context,
    messenger: BinaryMessenger,
    private val activity: MainActivity?,
) : MethodChannel.MethodCallHandler {
    private val methods = MethodChannel(messenger, "nixxa/phone")
    private val events = EventChannel(messenger, "nixxa/phone/nowplaying")
    private val main = Handler(Looper.getMainLooper())
    private val sessions = ctx.getSystemService(Context.MEDIA_SESSION_SERVICE) as MediaSessionManager
    private val listener = ComponentName(ctx, NixxaNotificationListener::class.java)

    private var sink: EventChannel.EventSink? = null
    private var watching = false
    private val watched = mutableListOf<MediaController>()

    private val controllerCallback = object : MediaController.Callback() {
        override fun onMetadataChanged(metadata: MediaMetadata?) = changed()
        override fun onPlaybackStateChanged(state: PlaybackState?) = changed()
        override fun onSessionDestroyed() = changed()
    }

    private val sessionsChanged = MediaSessionManager.OnActiveSessionsChangedListener { list ->
        watch(list ?: emptyList())
        changed()
    }

    init {
        methods.setMethodCallHandler(this)
        events.setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(arguments: Any?, s: EventChannel.EventSink) {
                sink = s
                ensureWatching()
            }

            override fun onCancel(arguments: Any?) {
                sink = null
                stopWatching()
            }
        })
    }

    fun dispose() {
        stopWatching()
        sink = null
        methods.setMethodCallHandler(null)
        events.setStreamHandler(null)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "battery" -> result.success(battery())
            "calendarAllowed" -> result.success(calendarAllowed())
            "requestCalendar" -> {
                val a = activity
                if (a == null) result.success(calendarAllowed()) else a.requestCalendarPermission { result.success(it) }
            }
            "nextEvent" -> {
                if (!calendarAllowed()) return result.error("NO_PERMISSION", "Calendar access is off.", null)
                io.execute {
                    val e = try { nextEvent() } catch (e: Exception) { Log.w(TAG, "calendar query failed", e); null }
                    main.post { result.success(e) }
                }
            }
            "nowPlaying" -> {
                ensureWatching()
                try {
                    result.success(nowPlaying())
                } catch (e: SecurityException) {
                    result.error("NO_ACCESS", "Allow notification access to see what is playing.", null)
                } catch (e: Exception) {
                    Log.w(TAG, "nowPlaying failed", e)
                    result.error("UNAVAILABLE", e.message ?: "Could not read the player.", null)
                }
            }
            else -> result.notImplemented()
        }
    }

    // ---------------------------------------------------------------- battery

    private fun battery(): Map<String, Any> {
        val i = ctx.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = i?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = i?.getIntExtra(BatteryManager.EXTRA_SCALE, 100) ?: 100
        val status = i?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val plugged = i?.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) ?: 0
        val pct = if (level >= 0 && scale > 0) (level * 100 / scale) else 0
        val charging = status == BatteryManager.BATTERY_STATUS_CHARGING ||
            (plugged != 0 && status != BatteryManager.BATTERY_STATUS_DISCHARGING && status != BatteryManager.BATTERY_STATUS_NOT_CHARGING)
        return mapOf("percent" to pct, "charging" to charging)
    }

    // ---------------------------------------------------------------- calendar

    private fun calendarAllowed() =
        ContextCompat.checkSelfPermission(ctx, Manifest.permission.READ_CALENDAR) == PackageManager.PERMISSION_GRANTED

    /**
     * The next event that has not ended, within a week. A timed event wins over
     * an all-day one (an all-day "today" would otherwise hide the 15:30 meeting).
     */
    private fun nextEvent(): Map<String, Any>? {
        val now = System.currentTimeMillis()
        val b = CalendarContract.Instances.CONTENT_URI.buildUpon()
        ContentUris.appendId(b, now)
        ContentUris.appendId(b, now + 7L * 24 * 3600 * 1000)
        val cols = arrayOf(
            CalendarContract.Instances.TITLE, CalendarContract.Instances.BEGIN,
            CalendarContract.Instances.END, CalendarContract.Instances.ALL_DAY,
        )
        val sort = "${CalendarContract.Instances.BEGIN} ASC"
        val cursor = try {
            ctx.contentResolver.query(b.build(), cols, "${CalendarContract.Instances.VISIBLE} = 1", null, sort)
        } catch (e: Exception) {
            ctx.contentResolver.query(b.build(), cols, null, null, sort)
        } ?: return null
        var timed: Map<String, Any>? = null
        var allDay: Map<String, Any>? = null
        cursor.use {
            while (it.moveToNext()) {
                val title = it.getString(0) ?: ""
                val begin = it.getLong(1)
                val end = it.getLong(2)
                val isAllDay = it.getInt(3) == 1
                if (end <= now) continue
                if (isAllDay) {
                    if (allDay == null) allDay = mapOf("title" to title, "startMs" to localMidnight(begin), "allDay" to true)
                } else if (timed == null) {
                    timed = mapOf("title" to title, "startMs" to begin, "allDay" to false)
                }
                if (timed != null && allDay != null) break
            }
        }
        return timed ?: allDay
    }

    /** All-day events are stored as UTC midnight; show them on that calendar day here. */
    private fun localMidnight(utcMs: Long): Long {
        val utc = Calendar.getInstance(TimeZone.getTimeZone("UTC")).apply { timeInMillis = utcMs }
        return Calendar.getInstance().apply {
            clear()
            set(utc.get(Calendar.YEAR), utc.get(Calendar.MONTH), utc.get(Calendar.DAY_OF_MONTH), 0, 0, 0)
        }.timeInMillis
    }

    // ---------------------------------------------------------------- now playing

    private fun title(c: MediaController): String? {
        val m = c.metadata ?: return null
        val t = m.getString(MediaMetadata.METADATA_KEY_TITLE) ?: m.getString(MediaMetadata.METADATA_KEY_DISPLAY_TITLE)
        return t?.takeIf { it.isNotBlank() }
    }

    /** The session playing now, or else one that is paused; throws SecurityException without notification access. */
    private fun pick(): MediaController? {
        val all = sessions.getActiveSessions(listener)
        fun state(c: MediaController) = c.playbackState?.state
        return all.firstOrNull { (state(it) == PlaybackState.STATE_PLAYING || state(it) == PlaybackState.STATE_BUFFERING) && title(it) != null }
            ?: all.firstOrNull { state(it) == PlaybackState.STATE_PAUSED && title(it) != null }
    }

    private fun nowPlaying(): Map<String, Any>? {
        val c = pick() ?: return null
        val m = c.metadata ?: return null
        val ps = c.playbackState
        val playing = ps?.state == PlaybackState.STATE_PLAYING || ps?.state == PlaybackState.STATE_BUFFERING
        val duration = m.getLong(MediaMetadata.METADATA_KEY_DURATION).coerceAtLeast(0)
        var pos = ps?.position ?: 0L
        if (pos < 0) pos = 0
        if (ps != null && ps.state == PlaybackState.STATE_PLAYING) {
            pos += ((SystemClock.elapsedRealtime() - ps.lastPositionUpdateTime) * ps.playbackSpeed).toLong()
        }
        if (duration > 0) pos = pos.coerceAtMost(duration)
        val artist = m.getString(MediaMetadata.METADATA_KEY_ARTIST)
            ?: m.getString(MediaMetadata.METADATA_KEY_ALBUM_ARTIST)
            ?: m.getString(MediaMetadata.METADATA_KEY_DISPLAY_SUBTITLE) ?: ""
        val app = try {
            ctx.packageManager.getApplicationLabel(ctx.packageManager.getApplicationInfo(c.packageName, 0)).toString()
        } catch (e: Exception) {
            ""
        }
        return mapOf(
            "title" to (title(c) ?: ""), "artist" to artist, "app" to app,
            "durationMs" to duration, "positionMs" to pos, "playing" to playing,
        )
    }

    /** Called from the player's callbacks and the session list: tell Dart to look again. */
    private fun changed() {
        main.post { sink?.success(null) }
    }

    private fun ensureWatching() {
        if (watching || sink == null) return
        try {
            sessions.addOnActiveSessionsChangedListener(sessionsChanged, listener, main)
            watching = true
            watch(sessions.getActiveSessions(listener))
        } catch (e: SecurityException) {
            // No notification access yet: Dart keeps polling, and this runs again on the next read.
        }
    }

    private fun watch(list: List<MediaController>) {
        watched.forEach { it.unregisterCallback(controllerCallback) }
        watched.clear()
        for (c in list) {
            c.registerCallback(controllerCallback, main)
            watched += c
        }
    }

    private fun stopWatching() {
        watched.forEach { it.unregisterCallback(controllerCallback) }
        watched.clear()
        if (watching) {
            try { sessions.removeOnActiveSessionsChangedListener(sessionsChanged) } catch (_: Exception) {}
            watching = false
        }
    }

    companion object {
        private const val TAG = "nixxa.phone"
        private val io = Executors.newSingleThreadExecutor()
    }
}
