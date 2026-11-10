// nixxa BLE protocol, pure Kotlin (no Android imports) so it runs in JVM unit
// tests. Contract: docs/ARCHITECTURE.md "ESP32 + BLE (nixxa)"; byte-level
// reference: firmware/nixxa_esp32/nx_core.{h,c}.
package com.zenithratech.nixxa.proto

import java.io.ByteArrayOutputStream
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

object Proto {
    const val SERVICE_UUID = "4e580001-6e69-4878-a100-6e6978786100"
    const val INFO_UUID = "4e580002-6e69-4878-a100-6e6978786100"
    const val RX_UUID = "4e580003-6e69-4878-a100-6e6978786100"
    const val TX_UUID = "4e580004-6e69-4878-a100-6e6978786100"
    const val CCCD_UUID = "00002902-0000-1000-8000-00805f9b34fb"

    const val PROTO_VERSION = 1
    const val HDR = 4
    const val INFO_SIZE = 41
    const val MTU_WANTED = 517
    const val BODY_MAX = 512
    const val RULES_MAX = 32
    const val APPID_MAX = 64

    const val OP_AUTH = 0x01
    const val OP_GET_STATE = 0x02
    const val OP_SET_TIME = 0x03
    const val OP_LIST_SLOTS = 0x10
    const val OP_PUT_SLOT = 0x11
    const val OP_GET_SLOT = 0x12
    const val OP_DELETE_SLOT = 0x13
    const val OP_ACTIVATE = 0x14
    const val OP_SET_NOTIF_CONFIG = 0x20
    const val OP_NOTIFY = 0x21
    const val OP_DISMISS = 0x22
    const val OP_SET_DND = 0x23
    const val OP_SET_CLOCK = 0x25
    const val OP_UNPAIR = 0x7F

    const val ST_OK = 0
    const val ST_NOT_AUTHED = 1
    const val ST_BAD_MSG = 2
    const val ST_BAD_SLOT = 3
    const val ST_NO_SPACE = 4
    const val ST_BUSY = 5
    const val ST_AUTH_FAILED = 6

    const val KIND_MESSAGE = 0
    const val KIND_CALL = 1
    const val KIND_CALL_ENDED = 2

    /** op u8, seq u8, len u16 LE, payload. */
    fun frame(op: Int, seq: Int, payload: ByteArray): ByteArray {
        require(payload.size <= 0xFFFF) { "payload too large" }
        val out = ByteArray(HDR + payload.size)
        out[0] = op.toByte()
        out[1] = seq.toByte()
        out[2] = (payload.size and 0xFF).toByte()
        out[3] = (payload.size shr 8).toByte()
        payload.copyInto(out, HDR)
        return out
    }

    /** Split one message into GATT writes of at most [size] bytes. */
    fun chunk(data: ByteArray, size: Int): List<ByteArray> {
        require(size > 0)
        if (data.isEmpty()) return listOf(data)
        return (data.indices step size).map { data.copyOfRange(it, minOf(it + size, data.size)) }
    }

    /** tag = first 16 bytes of HMAC-SHA256(k, nonce || "NIXXA-AUTH"). */
    fun authTag(k: ByteArray, nonce: ByteArray): ByteArray {
        require(k.size == 16 && nonce.size == 16)
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(k, "HmacSHA256"))
        mac.update(nonce)
        mac.update("NIXXA-AUTH".toByteArray(Charsets.US_ASCII))
        return mac.doFinal().copyOf(16)
    }

    fun setTime(unixSecs: Long, tzOffsetMin: Int): ByteArray {
        val w = Le()
        w.u32(unixSecs)
        w.u16(tzOffsetMin and 0xFFFF) // i16
        return w.bytes()
    }

    fun dismiss(notifId: Int): ByteArray = Le().apply { u32(notifId.toLong()) }.bytes()

    fun setDnd(on: Boolean): ByteArray = byteArrayOf(if (on) 1 else 0)

    /** SET_CLOCK: 0 = the face, 1..20 = that clock. */
    fun setClock(style: Int): ByteArray = byteArrayOf(style.coerceIn(0, 20).toByte())

    /**
     * SET_NOTIF_CONFIG. At most 32 rules go to the device (shown rules first);
     * the phone filters with the full list. Rule mood 0xFF ("default") is sent
     * as the default mood, since the device clamps out-of-range moods to 0.
     */
    fun notifConfig(c: NotifConfig, ext: Boolean = true): ByteArray {
        val w = Le()
        w.u8(if (c.enabled) 1 else 0)
        w.u8(c.cardSecs.coerceIn(2, 30))
        w.u8(c.defaultMood and 0xFF)
        w.u16(c.quietStartMin.coerceIn(0, 1439))
        w.u16(c.quietEndMin.coerceIn(0, 1439))
        w.u8(if (c.callsInQuiet) 1 else 0)
        val rules = c.rules
            .filter { it.appId.isNotEmpty() && it.appId.length <= APPID_MAX && it.appId.all { ch -> ch.code in 0x20..0x7E } }
            .sortedByDescending { it.show } // stable: keeps the user's order within each group
            .take(RULES_MAX)
        w.u8(rules.size)
        for (r in rules) {
            val id = r.appId.toByteArray(Charsets.US_ASCII)
            w.u8(id.size)
            w.raw(id)
            w.u8(if (r.mood == 0xFF) c.defaultMood and 0xFF else r.mood and 0xFF)
            w.u8(if (r.show) 1 else 0)
        }
        // ext u8: bit0 = forward calls. Firmware before ANCS rejects it with
        // BAD_MSG; the link then resends without it (ext = false).
        if (ext) w.u8(if (c.calls) 1 else 0)
        return w.bytes()
    }

    /** NOTIFY. Strings are cut to their length fields (u8, body 512). */
    fun notify(
        kind: Int, notifId: Int, mood: Int, appId: String, appName: String,
        title: String, body: String, icon: ByteArray?,
    ): ByteArray {
        val w = Le()
        w.u8(kind)
        w.u32(notifId.toLong())
        w.u8(mood and 0xFF)
        w.str8(appId)
        w.str8(appName)
        w.str8(title)
        val b = utf8Cut(body, BODY_MAX)
        w.u16(b.size)
        w.raw(b)
        if (icon != null) {
            // flag 1: 16x16 (32 bytes, firmware before 0.4), flag 2: 32x32 (128 bytes)
            require(icon.size == IconPack.BYTES_SMALL || icon.size == IconPack.BYTES_BIG)
            w.u8(if (icon.size == IconPack.BYTES_BIG) 2 else 1)
            w.raw(icon)
        } else {
            w.u8(0)
        }
        return w.bytes()
    }

    /** UTF-8 bytes of s, cut at a code-point boundary to at most max bytes. */
    fun utf8Cut(s: String, max: Int): ByteArray {
        val b = s.toByteArray(Charsets.UTF_8)
        if (b.size <= max) return b
        var n = max
        while (n > 0 && (b[n].toInt() and 0xC0) == 0x80) n-- // don't split a sequence
        return b.copyOf(n)
    }

    private class Le {
        private val o = ByteArrayOutputStream()
        fun u8(v: Int) = o.write(v and 0xFF)
        fun u16(v: Int) { o.write(v and 0xFF); o.write((v shr 8) and 0xFF) }
        fun u32(v: Long) { for (i in 0 until 4) o.write(((v shr (8 * i)) and 0xFF).toInt()) }
        fun raw(b: ByteArray) = o.write(b)
        fun str8(s: String) { val b = utf8Cut(s, 255); u8(b.size); raw(b) }
        fun bytes(): ByteArray = o.toByteArray()
    }
}

/** INFO characteristic (41 bytes). */
data class Info(
    val proto: Int, val chip: Int, val paired: Boolean, val slots: Int, val maxPayload: Int,
    val deviceId: String, val nonce: ByteArray, val fwVersion: String,
) {
    val chipName: String get() = when (chip) { 0 -> "esp32"; 1 -> "esp32c3"; 2 -> "esp32s3"; else -> "unknown" }

    companion object {
        fun parse(b: ByteArray): Info? {
            if (b.size < Proto.INFO_SIZE) return null
            val u = { i: Int -> b[i].toInt() and 0xFF }
            val id = (6 until 9).joinToString("") { "%02X".format(u(it)) }
            var end = 25
            while (end < 41 && b[end] != 0.toByte()) end++
            return Info(
                proto = u(0), chip = u(1), paired = u(2) != 0, slots = u(3),
                maxPayload = u(4) or (u(5) shl 8), deviceId = id,
                nonce = b.copyOfRange(9, 25),
                fwVersion = String(b, 25, end - 25, Charsets.US_ASCII),
            )
        }
    }
}

/** QR payload "NIXXA1:<ID>:<K as 32 uppercase hex>". */
data class PairingQr(val deviceId: String, val key: ByteArray) {
    companion object {
        private val RE = Regex("^NIXXA1:([0-9A-F]{6}):([0-9A-F]{32})$")
        fun parse(text: String): PairingQr? {
            val m = RE.matchEntire(text.trim().uppercase()) ?: return null
            val hex = m.groupValues[2]
            val k = ByteArray(16) { hex.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
            return PairingQr(m.groupValues[1], k)
        }
    }
}

/**
 * Reassembles device -> phone messages from TX notifications by the len field.
 * Several messages in one notification are all returned. A partial message
 * older than [gapMs] is abandoned when the next notification arrives.
 */
class Reassembler(private val gapMs: Long = 1000) {
    data class Msg(val op: Int, val seq: Int, val payload: ByteArray)

    private val buf = ByteArrayOutputStream()
    private var lastMs = 0L

    fun reset() { buf.reset() }

    fun feed(data: ByteArray, nowMs: Long): List<Msg> {
        if (buf.size() > 0 && nowMs - lastMs >= gapMs) buf.reset()
        lastMs = nowMs
        buf.write(data)
        val out = ArrayList<Msg>()
        val all = buf.toByteArray()
        var pos = 0
        while (all.size - pos >= Proto.HDR) {
            val len = (all[pos + 2].toInt() and 0xFF) or ((all[pos + 3].toInt() and 0xFF) shl 8)
            if (all.size - pos < Proto.HDR + len) break
            out += Msg(
                all[pos].toInt() and 0xFF, all[pos + 1].toInt() and 0xFF,
                all.copyOfRange(pos + Proto.HDR, pos + Proto.HDR + len),
            )
            pos += Proto.HDR + len
        }
        if (pos > 0) {
            buf.reset()
            buf.write(all, pos, all.size - pos)
        }
        return out
    }
}
