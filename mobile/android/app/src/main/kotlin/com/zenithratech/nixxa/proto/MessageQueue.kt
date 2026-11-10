// Stop-and-wait request queue (one message in flight, FIFO, reply timeout).
// Pure Kotlin; every method must be called from the same (link) thread.
package com.zenithratech.nixxa.proto

/** Codes match LinkException in mobile/lib/link/device_link.dart. */
class LinkError(val code: String, message: String, val deviceStatus: Int? = null) : Exception(message)

fun interface Scheduler {
    /** Run [r] after [ms] on the link thread; the returned function cancels it. */
    fun schedule(ms: Long, r: () -> Unit): () -> Unit
}

/** Writes one framed message (chunked by the transport); done(false) on a GATT failure. */
fun interface Transport {
    fun send(frame: ByteArray, done: (Boolean) -> Unit)
}

class MessageQueue(
    private val sched: Scheduler,
    private val transport: Transport,
    private val timeoutMs: Long = 5000,
) {
    private class Pending(val op: Int, val payload: ByteArray, val cb: (Result<ByteArray>) -> Unit)

    private val queue = ArrayDeque<Pending>()
    private var inflight: Pending? = null
    private var inflightSeq = 0
    private var cancelTimer: (() -> Unit)? = null
    private var seq = 0
    private var generation = 0

    /** Closed queues refuse new requests with NOT_CONNECTED. */
    var open = false
        private set

    val busy: Boolean get() = inflight != null || queue.isNotEmpty()

    fun open() { open = true }

    fun enqueue(op: Int, payload: ByteArray, cb: (Result<ByteArray>) -> Unit) {
        if (!open) {
            cb(Result.failure(LinkError("NOT_CONNECTED", "Not connected to the bunny")))
            return
        }
        queue.addLast(Pending(op, payload, cb))
        pump()
    }

    /** Feed every reassembled device -> phone message here. */
    fun onMessage(m: Reassembler.Msg) {
        val p = inflight ?: return
        if (m.op != (p.op or 0x80) || m.seq != inflightSeq) return // stale or unsolicited
        finish(
            if (m.payload.isEmpty()) {
                Result.failure(LinkError("DEVICE_ERROR", "Empty reply", Proto.ST_BAD_MSG))
            } else {
                val st = m.payload[0].toInt() and 0xFF
                if (st == Proto.ST_OK) Result.success(m.payload.copyOfRange(1, m.payload.size))
                else Result.failure(LinkError("DEVICE_ERROR", "Device status ${statusName(st)}", st))
            },
        )
    }

    /** Fail the in-flight and queued requests and refuse new ones. */
    fun close(code: String = "NOT_CONNECTED", message: String = "Disconnected from the bunny") {
        open = false
        generation++
        cancelTimer?.invoke()
        cancelTimer = null
        val all = listOfNotNull(inflight) + queue
        inflight = null
        queue.clear()
        for (p in all) p.cb(Result.failure(LinkError(code, message)))
    }

    private fun pump() {
        if (inflight != null || queue.isEmpty() || !open) return
        val p = queue.removeFirst()
        inflight = p
        seq = (seq + 1) and 0xFF
        inflightSeq = seq
        val gen = ++generation
        cancelTimer = sched.schedule(timeoutMs) {
            if (gen == generation && inflight === p) {
                finish(Result.failure(LinkError("TIMEOUT", "No reply from the bunny within ${timeoutMs / 1000} s")))
            }
        }
        transport.send(Proto.frame(p.op, inflightSeq, p.payload)) { ok ->
            if (!ok && gen == generation && inflight === p) {
                finish(Result.failure(LinkError("NOT_CONNECTED", "Write to the bunny failed")))
            }
        }
    }

    private fun finish(r: Result<ByteArray>) {
        val p = inflight ?: return
        inflight = null
        generation++
        cancelTimer?.invoke()
        cancelTimer = null
        p.cb(r)
        pump()
    }

    companion object {
        fun statusName(st: Int) = when (st) {
            Proto.ST_OK -> "OK"; Proto.ST_NOT_AUTHED -> "NOT_AUTHED"; Proto.ST_BAD_MSG -> "BAD_MSG"
            Proto.ST_BAD_SLOT -> "BAD_SLOT"; Proto.ST_NO_SPACE -> "NO_SPACE"; Proto.ST_BUSY -> "BUSY"
            Proto.ST_AUTH_FAILED -> "AUTH_FAILED"; else -> "0x%02X".format(st)
        }
    }
}
