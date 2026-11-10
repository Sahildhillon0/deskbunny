// Notification config, filtering, text and icon helpers. Pure Kotlin.
package com.zenithratech.nixxa.proto

import java.text.Normalizer

data class NotifRule(val appId: String, val mood: Int = 0xFF, val show: Boolean = true) {
    fun toMap(): Map<String, Any?> = mapOf("appId" to appId, "mood" to mood, "show" to show)

    companion object {
        fun fromMap(m: Map<*, *>) = NotifRule(
            m["appId"] as String, (m["mood"] as Number).toInt(), m["show"] as Boolean,
        )
    }
}

/** Mirrors NotifConfig in mobile/lib/link/device_link.dart (same map keys). */
data class NotifConfig(
    val enabled: Boolean = true,
    val cardSecs: Int = 6,
    val defaultMood: Int = 5,
    val quietStartMin: Int = 0,
    val quietEndMin: Int = 0,
    val callsInQuiet: Boolean = false,
    val followDnd: Boolean = true,
    val calls: Boolean = true,
    val rules: List<NotifRule> = emptyList(),
) {
    fun rule(appId: String): NotifRule? = rules.firstOrNull { it.appId == appId }

    fun toMap(): Map<String, Any?> = mapOf(
        "enabled" to enabled, "cardSecs" to cardSecs, "defaultMood" to defaultMood,
        "quietStartMin" to quietStartMin, "quietEndMin" to quietEndMin,
        "callsInQuiet" to callsInQuiet, "followDnd" to followDnd, "calls" to calls,
        "rules" to rules.map { it.toMap() },
    )

    companion object {
        fun fromMap(m: Map<*, *>) = NotifConfig(
            enabled = m["enabled"] as Boolean,
            cardSecs = (m["cardSecs"] as Number).toInt(),
            defaultMood = (m["defaultMood"] as Number).toInt(),
            quietStartMin = (m["quietStartMin"] as Number).toInt(),
            quietEndMin = (m["quietEndMin"] as Number).toInt(),
            callsInQuiet = m["callsInQuiet"] as Boolean,
            followDnd = m["followDnd"] as Boolean,
            calls = m["calls"] as Boolean,
            rules = (m["rules"] as List<*>).map { NotifRule.fromMap(it as Map<*, *>) },
        )
    }
}

/** What the listener saw, stripped of Android types. */
data class PostedNotif(
    val pkg: String,
    val key: String,
    val ongoing: Boolean,
    val groupSummary: Boolean,
    val isCallCategory: Boolean,
    /** Notification.EXTRA_CALL_TYPE (API 31+): 1 incoming, 2 ongoing, 3 screening; 0 unknown. */
    val callType: Int = 0,
)

sealed class Decision {
    data class Skip(val reason: String) : Decision()
    data class Message(val mood: Int) : Decision()
    data class Call(val mood: Int) : Decision()
    object CallEnded : Decision()
    object Dismiss : Decision()
}

/**
 * The phone-side filter (ARCHITECTURE.md "Android link", Notifications).
 * Not thread-safe; the listener calls it from one thread.
 */
class NotifFilter(private val ownPkg: String, private val dupMs: Long = 2000) {
    private val lastSent = LinkedHashMap<String, Long>()
    private val activeCalls = HashSet<String>()

    fun onPosted(cfg: NotifConfig, n: PostedNotif, defaultDialer: String?, nowMs: Long): Decision {
        if (n.pkg == ownPkg) return Decision.Skip("own package")
        if (!cfg.enabled) return Decision.Skip("disabled")

        // Ringing notifications are posted by the telephony UI, which is not always
        // the default dialer package (Samsung's in-call UI posts them itself).
        val telephony = n.pkg == defaultDialer || n.pkg in TELEPHONY_UI || n.callType in 1..3
        val isCall = n.isCallCategory && telephony
        if (isCall) {
            if (!cfg.calls) return Decision.Skip("calls off")
            if (n.key in activeCalls) {
                // Answered: the dialer turns the ringing notification into an
                // ongoing-call one with the same key. Stop the ringing.
                if (n.callType == 2) { activeCalls.remove(n.key); return Decision.CallEnded }
                return Decision.Skip("call update")
            }
            if (n.callType == 2) return Decision.Skip("ongoing call")
            activeCalls.add(n.key)
            return Decision.Call(cfg.rule(n.pkg)?.mood ?: 0xFF)
        }

        val rule = cfg.rule(n.pkg) ?: return Decision.Skip("no rule")
        if (!rule.show) return Decision.Skip("hidden")
        if (n.ongoing) return Decision.Skip("ongoing")
        if (n.groupSummary) return Decision.Skip("group summary")
        val last = lastSent[n.key]
        if (last != null && nowMs - last < dupMs) return Decision.Skip("duplicate")
        lastSent.remove(n.key)
        lastSent[n.key] = nowMs
        while (lastSent.size > 256) lastSent.remove(lastSent.keys.first())
        return Decision.Message(rule.mood)
    }

    fun onRemoved(key: String): Decision {
        if (activeCalls.remove(key)) return Decision.CallEnded
        if (lastSent.remove(key) != null) return Decision.Dismiss
        return Decision.Skip("not forwarded")
    }

    companion object {
        /** notifId = the key hash, as u32 on the wire. */
        fun notifId(key: String): Int = key.hashCode()

        /** Packages that post a call's ringing notification from the phone UI. */
        val TELEPHONY_UI = setOf(
            "com.android.incallui",
            "com.samsung.android.incallui",
            "com.samsung.android.dialer",
            "com.google.android.dialer",
            "com.android.dialer",
            "com.android.server.telecom",
        )
    }
}

/** Printable-ASCII transliteration (the device font is 0x20..0x7E). */
object Translit {
    private val MAP = mapOf(
        'ß' to "ss", 'æ' to "ae", 'Æ' to "AE", 'œ' to "oe", 'Œ' to "OE", 'ø' to "o", 'Ø' to "O",
        'ł' to "l", 'Ł' to "L", 'đ' to "d", 'Đ' to "D", 'ð' to "d", 'Ð' to "D", 'þ' to "th",
        'Þ' to "Th", 'ı' to "i", 'ħ' to "h", 'Ħ' to "H",
        '‘' to "'", '’' to "'", '‚' to "'", '‛' to "'", '′' to "'", '´' to "'", '`' to "'",
        '“' to "\"", '”' to "\"", '„' to "\"", '″' to "\"", '«' to "\"", '»' to "\"",
        '‹' to "<", '›' to ">", '–' to "-", '—' to "-", '―' to "-", '‐' to "-", '‑' to "-",
        '−' to "-", '…' to "...", '•' to "*", '·' to ".", '€' to "EUR", '£' to "GBP",
        '¥' to "JPY", '₹' to "Rs", '©' to "(c)", '®' to "(R)", '™' to "TM", '°' to "o",
        '×' to "x", '÷' to "/", '⁄' to "/", '¡' to "!", '¿' to "?", '½' to "1/2", '¼' to "1/4", '¾' to "3/4",
    )

    /**
     * Strip accents (NFD, drop combining marks), map common symbols, turn every
     * other non-ASCII code point into '?'. Zero-width joiners and variation
     * selectors vanish so one emoji becomes one '?'. Tabs and odd spaces become
     * a space, '\r' and other controls vanish, '\n' is kept only when keepNl.
     */
    fun ascii(s: String, keepNl: Boolean): String {
        val d = Normalizer.normalize(s, Normalizer.Form.NFKD)
        val out = StringBuilder(d.length)
        var i = 0
        while (i < d.length) {
            val cp = d.codePointAt(i)
            i += Character.charCount(cp)
            if (cp == 0x200D) { // zero-width joiner: the joined emoji part adds nothing
                if (i < d.length) i += Character.charCount(d.codePointAt(i))
                continue
            }
            when {
                cp == '\n'.code -> out.append(if (keepNl) '\n' else ' ')
                cp in 0x20..0x7E -> out.append(cp.toChar())
                cp == '\t'.code || Character.isSpaceChar(cp) -> out.append(' ')
                cp < 0x20 || cp == 0x7F -> {}
                Character.getType(cp) == Character.NON_SPACING_MARK.toInt() -> {}
                Character.getType(cp) == Character.ENCLOSING_MARK.toInt() -> {}
                cp in 0xFE00..0xFE0F || cp in 0x200B..0x200C || cp == 0x2060 ||
                    cp in 0x1F3FB..0x1F3FF || cp in 0xE0020..0xE007F -> {}
                cp <= 0xFFFF && MAP.containsKey(cp.toChar()) -> out.append(MAP[cp.toChar()])
                else -> out.append('?')
            }
        }
        return out.toString()
    }

    /** ASCII then cut to max chars (title/app name 255, body 512). */
    fun field(s: CharSequence?, max: Int, keepNl: Boolean = false): String {
        if (s == null) return ""
        val a = ascii(s.toString(), keepNl).trim()
        return if (a.length > max) a.substring(0, max) else a
    }
}

object IconPack {
    /** The logo sent to the bunny: 32x32, 1 bpp, 128 bytes (flag 2 on the wire). */
    const val SIZE = 32
    const val BYTES_BIG = SIZE * SIZE / 8
    const val BYTES_SMALL = 16 * 16 / 8

    /** SIZE x SIZE ARGB pixels (row-major) -> 1 bpp, MSB first, lit when alpha >= 50%. */
    fun pack(argb: IntArray): ByteArray {
        require(argb.size == SIZE * SIZE)
        val out = ByteArray(BYTES_BIG)
        for (y in 0 until SIZE) for (x in 0 until SIZE) {
            val a = (argb[y * SIZE + x] ushr 24) and 0xFF
            if (a >= 128) {
                val bit = y * SIZE + x
                out[bit shr 3] = (out[bit shr 3].toInt() or (0x80 ushr (bit and 7))).toByte()
            }
        }
        return out
    }

    /**
     * A full-colour logo -> 1 bpp. Each pixel's luminance is split at an Otsu
     * threshold, and the minority class is the glyph (lit): a white mark on a
     * coloured tile lights the mark, a dark mark on white lights the mark.
     * Transparent pixels stay dark.
     */
    fun fromPixels(argb: IntArray): ByteArray {
        require(argb.size == SIZE * SIZE)
        val lum = IntArray(argb.size)
        val hist = IntArray(256)
        var total = 0
        for (i in argb.indices) {
            val c = argb[i]
            if (((c ushr 24) and 0xFF) < 128) { lum[i] = -1; continue }
            val y = (((c shr 16) and 0xFF) * 299 + ((c shr 8) and 0xFF) * 587 + (c and 0xFF) * 114) / 1000
            lum[i] = y
            hist[y]++
            total++
        }
        val t = otsu(hist, total)
        var bright = 0
        for (v in lum) if (v > t) bright++
        val lightIsGlyph = bright * 2 < total
        val out = ByteArray(BYTES_BIG)
        for (i in lum.indices) {
            val v = lum[i]
            if (v < 0) continue
            val on = if (lightIsGlyph) v > t else v <= t
            if (on) out[i shr 3] = (out[i shr 3].toInt() or (0x80 ushr (i and 7))).toByte()
        }
        return out
    }

    private fun otsu(hist: IntArray, total: Int): Int {
        if (total == 0) return 128
        var sum = 0L
        for (i in 0..255) sum += i.toLong() * hist[i]
        var sumB = 0L
        var wB = 0
        var best = -1.0
        var thr = 128
        for (t in 0..255) {
            wB += hist[t]
            if (wB == 0) continue
            val wF = total - wB
            if (wF == 0) break
            sumB += t.toLong() * hist[t]
            val mB = sumB.toDouble() / wB
            val mF = (sum - sumB).toDouble() / wF
            val between = wB.toDouble() * wF * (mB - mF) * (mB - mF)
            if (between > best) { best = between; thr = t }
        }
        return thr
    }

    /** Blank icons and filled squares (full-colour icons) fall back to the bell. */
    fun usable(packed: ByteArray): Boolean {
        val lit = packed.sumOf { Integer.bitCount(it.toInt() and 0xFF) }
        return lit in 1..(SIZE * SIZE * 9 / 10)
    }
}
