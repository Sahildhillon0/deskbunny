package com.zenithratech.nixxa.proto

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TranslitTest {
    @Test fun accentsAndSymbols() {
        assertEquals("Cafe creme", Translit.ascii("Café crème", false))
        assertEquals("Strasse AEble oeuvre Lodz", Translit.ascii("Straße Æble œuvre Łódź", false))
        assertEquals("\"Hi\" - it's 5EUR...", Translit.ascii("“Hi” — it’s 5€…", false))
        assertEquals("!Ole! fi 2 1/2", Translit.ascii("¡Olé! ﬁ ² ½", false))
    }

    @Test fun emojiBecomeOneQuestionMark() {
        assertEquals("ok ? ? ?!", Translit.ascii("ok 👍🏽 ❤️ 👨‍👩‍👧!", false))
        assertEquals("? ?", Translit.ascii("日 本", false))
    }

    @Test fun whitespaceAndControls() {
        assertEquals("a b\nc", Translit.ascii("a\tb\r\nc", true))
        assertEquals("a b c", Translit.ascii("a\tb\r\nc", false))
        assertEquals("x y", Translit.ascii("x y\u0007", false))
        assertTrue(Translit.ascii("Ünïcödé “x” — 😀 ½", true).all { it == '\n' || it.code in 0x20..0x7E })
    }

    @Test fun fieldCutsAndTrims() {
        assertEquals("", Translit.field(null, 10))
        assertEquals("abc", Translit.field("  abcdef ", 3))
        assertEquals(512, Translit.field("é".repeat(600), 512).length)
    }
}

class IconPackTest {
    @Test fun bitOrder() {
        val px = IntArray(1024)
        px[0] = 0xFF000000.toInt()          // (0,0) -> byte 0, MSB
        px[9] = 0x80FFFFFF.toInt()          // (9,0) alpha 128 -> byte 1, bit 6
        px[10] = 0x7FFFFFFF                 // alpha 127 -> off
        px[1023] = 0xFFFFFFFF.toInt()       // (31,31) -> byte 127, LSB
        val b = IconPack.pack(px)
        assertEquals(128, b.size)
        assertEquals(0x80, b[0].toInt() and 0xFF)
        assertEquals(0x40, b[1].toInt() and 0xFF)
        assertEquals(0x01, b[127].toInt() and 0xFF)
        assertEquals(3, b.sumOf { Integer.bitCount(it.toInt() and 0xFF) })
        assertTrue(IconPack.usable(b))
        assertFalse(IconPack.usable(ByteArray(128)))
        assertFalse(IconPack.usable(IconPack.pack(IntArray(1024) { -1 })))
    }

    @Test fun whiteGlyphOnColouredTileIsLit() {
        // WhatsApp-like: the green tile is the majority, the white mark the glyph.
        val green = 0xFF25D366.toInt()
        val px = IntArray(1024) { green }
        for (y in 10 until 14) for (x in 10 until 14) px[y * 32 + x] = 0xFFFFFFFF.toInt()
        val b = IconPack.fromPixels(px)
        assertEquals(16, b.sumOf { Integer.bitCount(it.toInt() and 0xFF) })
        assertTrue(IconPack.usable(b))
    }

    @Test fun darkGlyphOnWhiteIsLit() {
        val px = IntArray(1024) { 0xFFFFFFFF.toInt() }
        for (x in 0 until 32) px[5 * 32 + x] = 0xFF000000.toInt()
        val b = IconPack.fromPixels(px)
        assertEquals(32, b.sumOf { Integer.bitCount(it.toInt() and 0xFF) })
    }
}

class NotifFilterTest {
    private val own = "com.zenithratech.nixxa"
    private val dialer = "com.google.android.dialer"
    private val cfg = NotifConfig(rules = listOf(NotifRule("com.whatsapp", 6, true), NotifRule("com.slack", 2, false)))

    private fun n(pkg: String, key: String = "k", ongoing: Boolean = false, summary: Boolean = false,
                  call: Boolean = false, callType: Int = 0) =
        PostedNotif(pkg, key, ongoing, summary, call, callType)

    @Test fun rules() {
        val f = NotifFilter(own)
        assertEquals(Decision.Message(6), f.onPosted(cfg, n("com.whatsapp", "a"), dialer, 0))
        assertTrue(f.onPosted(cfg, n("com.slack", "b"), dialer, 0) is Decision.Skip) // show = false
        assertTrue(f.onPosted(cfg, n("com.other", "c"), dialer, 0) is Decision.Skip) // no rule
        assertTrue(f.onPosted(cfg, n(own, "d"), dialer, 0) is Decision.Skip)
        assertTrue(f.onPosted(cfg.copy(enabled = false), n("com.whatsapp", "e"), dialer, 0) is Decision.Skip)
        assertTrue(f.onPosted(cfg, n("com.whatsapp", "f", ongoing = true), dialer, 0) is Decision.Skip)
        assertTrue(f.onPosted(cfg, n("com.whatsapp", "g", summary = true), dialer, 0) is Decision.Skip)
        val dflt = cfg.copy(rules = listOf(NotifRule("com.whatsapp")))
        assertEquals(Decision.Message(0xFF), f.onPosted(dflt, n("com.whatsapp", "h"), dialer, 0))
    }

    @Test fun duplicatesWithinTwoSeconds() {
        val f = NotifFilter(own)
        assertTrue(f.onPosted(cfg, n("com.whatsapp", "a"), dialer, 1000) is Decision.Message)
        assertTrue(f.onPosted(cfg, n("com.whatsapp", "a"), dialer, 2999) is Decision.Skip)
        assertTrue(f.onPosted(cfg, n("com.whatsapp", "b"), dialer, 2999) is Decision.Message)
        assertTrue(f.onPosted(cfg, n("com.whatsapp", "a"), dialer, 3000) is Decision.Message)
    }

    @Test fun removalDismissesOnlyForwarded() {
        val f = NotifFilter(own)
        f.onPosted(cfg, n("com.whatsapp", "a"), dialer, 0)
        assertEquals(Decision.Dismiss, f.onRemoved("a"))
        assertTrue(f.onRemoved("a") is Decision.Skip)
        assertTrue(f.onRemoved("never") is Decision.Skip)
    }

    @Test fun calls() {
        val f = NotifFilter(own)
        // ringing call from the default dialer: ongoing, no rule needed
        assertEquals(Decision.Call(0xFF), f.onPosted(cfg, n(dialer, "c", ongoing = true, call = true, callType = 1), dialer, 0))
        assertTrue(f.onPosted(cfg, n(dialer, "c", ongoing = true, call = true, callType = 1), dialer, 10) is Decision.Skip)
        assertEquals(Decision.CallEnded, f.onRemoved("c"))
        // answered: the same key turns into an ongoing call
        f.onPosted(cfg, n(dialer, "d", ongoing = true, call = true, callType = 1), dialer, 0)
        assertEquals(Decision.CallEnded, f.onPosted(cfg, n(dialer, "d", ongoing = true, call = true, callType = 2), dialer, 5))
        assertTrue(f.onRemoved("d") is Decision.Skip)
        // outgoing / already ongoing call: nothing
        assertTrue(f.onPosted(cfg, n(dialer, "e", ongoing = true, call = true, callType = 2), dialer, 0) is Decision.Skip)
        // calls switched off, or CATEGORY_CALL from another app (e.g. VoIP without a rule)
        assertTrue(f.onPosted(cfg.copy(calls = false), n(dialer, "f", call = true), dialer, 0) is Decision.Skip)
        assertTrue(f.onPosted(cfg, n("com.voip", "g", call = true), dialer, 0) is Decision.Skip)
        // rule mood applies to the dialer
        val withRule = cfg.copy(rules = cfg.rules + NotifRule(dialer, 9, true))
        assertEquals(Decision.Call(9), f.onPosted(withRule, n(dialer, "h", call = true), dialer, 0))
    }

    @Test fun notifIdIsKeyHash() {
        assertEquals("0|com.x|1|null|10001".hashCode(), NotifFilter.notifId("0|com.x|1|null|10001"))
    }

    @Test fun configMapRoundTrip() {
        val c = cfg.copy(quietStartMin = 1320, quietEndMin = 420, followDnd = false)
        assertEquals(c, NotifConfig.fromMap(c.toMap()))
    }
}

class MessageQueueTest {
    private class FakeSched : Scheduler {
        val timers = ArrayList<Pair<Long, () -> Unit>>()
        var now = 0L
        override fun schedule(ms: Long, r: () -> Unit): () -> Unit {
            val t = (now + ms) to r
            timers += t
            return { timers.remove(t) }
        }
        fun advance(ms: Long) {
            now += ms
            timers.filter { it.first <= now }.forEach { timers.remove(it); it.second() }
        }
    }

    private val sched = FakeSched()
    private val sent = ArrayList<ByteArray>()
    private val q = MessageQueue(sched, { f, done -> sent += f; done(true) })
    private val results = ArrayList<Result<ByteArray>>()

    private fun reply(op: Int, seq: Int, vararg payload: Int) =
        q.onMessage(Reassembler.Msg(op, seq, ByteArray(payload.size) { payload[it].toByte() }))

    @Test fun closedQueueRefuses() {
        q.enqueue(2, ByteArray(0)) { results += it }
        assertEquals("NOT_CONNECTED", (results[0].exceptionOrNull() as LinkError).code)
        assertTrue(sent.isEmpty())
    }

    @Test fun stopAndWaitFifo() {
        q.open()
        q.enqueue(0x02, ByteArray(0)) { results += it }
        q.enqueue(0x14, byteArrayOf(3)) { results += it }
        assertEquals(1, sent.size) // second waits for the first reply
        assertEquals(0x02, sent[0][0].toInt())
        val seq1 = sent[0][1].toInt() and 0xFF
        reply(0x82, (seq1 + 1) and 0xFF, 0) // wrong seq: ignored
        reply(0x94, seq1, 0)                // wrong op: ignored
        assertEquals(0, results.size)
        reply(0x82, seq1, 0, 5, 1, 0)
        assertEquals(1, results.size)
        assertEquals(listOf<Byte>(5, 1, 0), results[0].getOrThrow().toList())
        assertEquals(2, sent.size)
        assertEquals(0x14, sent[1][0].toInt())
        assertEquals((seq1 + 1) and 0xFF, sent[1][1].toInt() and 0xFF)
        reply(0x94, sent[1][1].toInt() and 0xFF, 3) // BAD_SLOT
        val e = results[1].exceptionOrNull() as LinkError
        assertEquals("DEVICE_ERROR", e.code)
        assertEquals(3, e.deviceStatus)
        assertFalse(q.busy)
    }

    @Test fun timeoutThenNext() {
        q.open()
        q.enqueue(0x10, ByteArray(0)) { results += it }
        q.enqueue(0x02, ByteArray(0)) { results += it }
        sched.advance(4999)
        assertEquals(0, results.size)
        sched.advance(1)
        assertEquals("TIMEOUT", (results[0].exceptionOrNull() as LinkError).code)
        assertEquals(2, sent.size)
        reply(0x90, sent[0][1].toInt() and 0xFF, 0) // late reply to the first: ignored
        assertEquals(1, results.size)
        reply(0x82, sent[1][1].toInt() and 0xFF, 0)
        assertTrue(results[1].isSuccess)
        assertTrue(sched.timers.isEmpty())
    }

    @Test fun closeFailsEverything() {
        q.open()
        q.enqueue(0x10, ByteArray(0)) { results += it }
        q.enqueue(0x02, ByteArray(0)) { results += it }
        q.close()
        assertEquals(listOf("NOT_CONNECTED", "NOT_CONNECTED"), results.map { (it.exceptionOrNull() as LinkError).code })
        assertTrue(sched.timers.isEmpty())
        q.enqueue(0x02, ByteArray(0)) { results += it }
        assertEquals(3, results.size)
    }

    @Test fun writeFailureFailsRequest() {
        val q2 = MessageQueue(sched, { _, done -> done(false) })
        q2.open()
        q2.enqueue(0x02, ByteArray(0)) { results += it }
        assertEquals("NOT_CONNECTED", (results[0].exceptionOrNull() as LinkError).code)
    }

    @Test fun seqWraps() {
        q.open()
        repeat(300) {
            q.enqueue(0x02, ByteArray(0)) { results += it }
            val last = sent.last()
            reply(0x82, last[1].toInt() and 0xFF, 0)
        }
        assertEquals(300, results.count { it.isSuccess })
    }
}
