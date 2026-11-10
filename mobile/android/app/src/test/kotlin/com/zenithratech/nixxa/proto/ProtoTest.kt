package com.zenithratech.nixxa.proto

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream

private fun hex(b: ByteArray) = b.joinToString("") { "%02x".format(it) }
private fun bytes(vararg v: Int) = ByteArray(v.size) { v[it].toByte() }

class ProtoTest {
    @Test fun authTagMatchesFirmwareVector() {
        // firmware/nixxa_esp32/tests/test_core.c test_auth
        val k = ByteArray(16) { it.toByte() }
        val nonce = ByteArray(16) { (0x10 + it).toByte() }
        assertEquals("8b0bfcfaf44904dcf18843ba7721e362", hex(Proto.authTag(k, nonce)))
    }

    @Test fun infoParse() {
        // Same bytes as nx_info_build(info, 2, 1, {A1,B2,C3}, nonce 100.., "0.2.0")
        val b = ByteArray(41)
        b[0] = 1; b[1] = 2; b[2] = 1; b[3] = 16; b[4] = 0x40; b[5] = 0x30
        b[6] = 0xA1.toByte(); b[7] = 0xB2.toByte(); b[8] = 0xC3.toByte()
        for (i in 0 until 16) b[9 + i] = (100 + i).toByte()
        "0.2.0".toByteArray().copyInto(b, 25)
        val info = Info.parse(b)!!
        assertEquals(1, info.proto)
        assertEquals("esp32s3", info.chipName)
        assertTrue(info.paired)
        assertEquals(16, info.slots)
        assertEquals(12352, info.maxPayload)
        assertEquals("A1B2C3", info.deviceId)
        assertArrayEquals(ByteArray(16) { (100 + it).toByte() }, info.nonce)
        assertEquals("0.2.0", info.fwVersion)
        assertNull(Info.parse(ByteArray(40)))
        // fwVersion filling all 16 bytes (no NUL)
        "0123456789ABCDEF".toByteArray().copyInto(b, 25)
        assertEquals("0123456789ABCDEF", Info.parse(b)!!.fwVersion)
    }

    @Test fun qrParse() {
        val q = PairingQr.parse("NIXXA1:A1B2C3:F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF")!!
        assertEquals("A1B2C3", q.deviceId)
        assertArrayEquals(ByteArray(16) { (0xF0 + it).toByte() }, q.key)
        assertNotNull(PairingQr.parse("  nixxa1:a1b2c3:f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff\n"))
        assertNull(PairingQr.parse("NIXXA1:A1B2C3:F0F1"))
        assertNull(PairingQr.parse("NIXXA2:A1B2C3:F0F1F2F3F4F5F6F7F8F9FAFBFCFDFEFF"))
        assertNull(PairingQr.parse("https://example.com"))
    }

    @Test fun frameAndChunk() {
        val f = Proto.frame(0x21, 9, ByteArray(296) { it.toByte() })
        assertEquals(300, f.size)
        assertEquals(0x21, f[0].toInt())
        assertEquals(9, f[1].toInt())
        assertEquals(296, (f[2].toInt() and 0xFF) or ((f[3].toInt() and 0xFF) shl 8))
        val chunks = Proto.chunk(f, 182)
        assertEquals(listOf(182, 118), chunks.map { it.size })
        assertArrayEquals(f, chunks.fold(ByteArray(0)) { a, c -> a + c })
        assertEquals(1, Proto.chunk(Proto.frame(2, 1, ByteArray(0)), 20).size)
        // 12352-byte payload in 509-byte writes (MTU 512)
        val big = Proto.frame(0x11, 1, ByteArray(12352))
        assertEquals(25, Proto.chunk(big, 509).size)
    }

    @Test fun reassembly() {
        val r = Reassembler()
        val msg = Proto.frame(0x92, 5, ByteArray(300) { it.toByte() })
        assertTrue(r.feed(msg.copyOfRange(0, 1), 0).isEmpty()) // split header
        assertTrue(r.feed(msg.copyOfRange(1, 3), 1).isEmpty())
        assertTrue(r.feed(msg.copyOfRange(3, 200), 2).isEmpty())
        val out = r.feed(msg.copyOfRange(200, msg.size), 3)
        assertEquals(1, out.size)
        assertEquals(0x92, out[0].op)
        assertEquals(5, out[0].seq)
        assertArrayEquals(ByteArray(300) { it.toByte() }, out[0].payload)

        // two messages in one notification, the second split
        val a = Proto.frame(0x81, 1, bytes(0))
        val b = Proto.frame(0x82, 2, bytes(0, 1, 2, 3, 4, 5))
        val first = r.feed(a + b.copyOfRange(0, 5), 10)
        assertEquals(listOf(0x81), first.map { it.op })
        val second = r.feed(b.copyOfRange(5, b.size), 11)
        assertEquals(listOf(0x82), second.map { it.op })
        assertArrayEquals(bytes(0, 1, 2, 3, 4, 5), second[0].payload)

        // a stale half message is abandoned after the gap
        assertTrue(r.feed(b.copyOfRange(0, 6), 100).isEmpty())
        val fresh = r.feed(a, 2000)
        assertEquals(listOf(0x81), fresh.map { it.op })
    }

    @Test fun setTimeDismissDnd() {
        assertArrayEquals(bytes(0x78, 0x56, 0x34, 0x12, 0x4A, 0x01), Proto.setTime(0x12345678, 330))
        assertArrayEquals(bytes(0x00, 0x00, 0x00, 0x80, 0xC4, 0xFF), Proto.setTime(0x80000000L, -60))
        assertArrayEquals(bytes(0x78, 0x56, 0x34, 0x12), Proto.dismiss(0x12345678))
        assertArrayEquals(bytes(0xFF, 0xFF, 0xFF, 0xFF), Proto.dismiss(-1))
        assertArrayEquals(bytes(1), Proto.setDnd(true))
        assertArrayEquals(bytes(0), Proto.setDnd(false))
    }

    @Test fun notifConfigMatchesFirmwareLayout() {
        // Mirrors test_core.c test_cfg (cardSecs and moods already in range).
        val c = NotifConfig(
            enabled = true, cardSecs = 30, defaultMood = 0, quietStartMin = 22 * 60, quietEndMin = 7 * 60,
            callsInQuiet = true,
            rules = listOf(NotifRule("com.whatsapp", 6, true), NotifRule("com.slack", 2, false)),
        )
        val o = ByteArrayOutputStream()
        o.write(bytes(1, 30, 0, 0x28, 0x05, 0xA4, 0x01, 1, 2))
        o.write(12); o.write("com.whatsapp".toByteArray()); o.write(6); o.write(1)
        o.write(9); o.write("com.slack".toByteArray()); o.write(2); o.write(0)
        o.write(1) // ext: bit0 = calls
        assertArrayEquals(o.toByteArray(), Proto.notifConfig(c))
        assertEquals(0, Proto.notifConfig(c.copy(calls = false)).last().toInt())
    }

    @Test fun notifConfigLimits() {
        val rules = (0 until 40).map { NotifRule("app.$it", if (it == 0) 0xFF else 3, it % 2 == 0) } +
            NotifRule("x".repeat(65)) + NotifRule("")
        val p = Proto.notifConfig(NotifConfig(cardSecs = 99, defaultMood = 7, quietStartMin = 5000, rules = rules))
        assertEquals(30, p[1].toInt())
        assertEquals(1439, (p[3].toInt() and 0xFF) or ((p[4].toInt() and 0xFF) shl 8))
        assertEquals(32, p[8].toInt())
        // shown rules come first; mood 0xFF becomes the default mood
        assertEquals(5, p[9].toInt())
        assertEquals("app.0", String(p, 10, 5))
        assertEquals(7, p[15].toInt())
        assertEquals(1, p[16].toInt())
        // parse back like nx_cfg_parse: 32 rules, then exactly the ext byte
        var pos = 9
        repeat(32) { val n = p[pos].toInt(); assertTrue(n in 1..64); pos += 1 + n + 2 }
        assertEquals(p.size, pos + 1)
        assertEquals(1, p[pos].toInt())
        // 20 shown + 12 hidden
        pos = 9
        var shown = 0
        repeat(32) { val n = p[pos].toInt(); if (p[pos + n + 2].toInt() == 1) shown++; pos += 1 + n + 2 }
        assertEquals(20, shown)
    }

    @Test fun notifyMatchesFirmwareLayout() {
        // Mirrors test_core.c test_notify_put; the phone cuts the body to 512.
        val icon = ByteArray(32) { it.toByte() }
        val p = Proto.notify(0, 0x12345678, 0xFF, "com.x", "X", "Hello", "b".repeat(600), icon)
        val o = ByteArrayOutputStream()
        o.write(bytes(0, 0x78, 0x56, 0x34, 0x12, 0xFF))
        o.write(5); o.write("com.x".toByteArray())
        o.write(1); o.write("X".toByteArray())
        o.write(5); o.write("Hello".toByteArray())
        o.write(bytes(0x00, 0x02)); o.write("b".repeat(512).toByteArray())
        o.write(1); o.write(icon)
        assertArrayEquals(o.toByteArray(), p)

        val call = Proto.notify(1, 7, 3, "com.dialer", "Phone", "Mum", "", null)
        assertArrayEquals(
            bytes(1, 7, 0, 0, 0, 3, 10) + "com.dialer".toByteArray() + bytes(5) + "Phone".toByteArray() +
                bytes(3) + "Mum".toByteArray() + bytes(0, 0, 0),
            call,
        )
    }

    @Test fun utf8CutKeepsSequencesWhole() {
        assertEquals(1, Proto.utf8Cut("aé", 2).size) // would split the 2-byte é
        assertEquals("a", String(Proto.utf8Cut("aé", 2), Charsets.UTF_8))
        assertEquals(3, Proto.utf8Cut("aé", 3).size)
        assertEquals(255, Proto.notify(0, 0, 0, "a", "b", "t".repeat(300), "", null)[6 + 2 + 2].toInt() and 0xFF)
    }

    @Test fun notifConfigWithoutExtIsOneByteShorter() {
        val c = NotifConfig()
        val with = Proto.notifConfig(c, true)
        val without = Proto.notifConfig(c, false)
        org.junit.Assert.assertEquals(with.size - 1, without.size)
        org.junit.Assert.assertArrayEquals(with.copyOf(without.size), without)
    }
}
