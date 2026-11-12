package com.pixel.studio.bridge

import com.pixel.studio.json.Json
import com.pixel.studio.json.long
import com.pixel.studio.json.obj
import com.pixel.studio.json.str
import java.util.concurrent.CopyOnWriteArrayList

/** Decoded engine response. */
class EngineResponse(val raw: String, val binary: ByteArray) {
    val json: Map<String, Any?> = try {
        Json.parseObject(raw)
    } catch (e: Json.ParseException) {
        mapOf("ok" to false, "error" to mapOf("code" to "internal", "message" to "bad engine response: ${e.message}"))
    }
    val ok: Boolean get() = json["ok"] == true
    val revision: Long get() = json.long("revision") ?: -1
    val result: Map<String, Any?> get() = json.obj("result") ?: emptyMap()
    val errorCode: String? get() = json.obj("error")?.str("code")
    val errorMessage: String? get() = json.obj("error")?.str("message")

    override fun toString(): String = raw
}

/** Who is changing the canvas; recorded in the history log. */
object Actors {
    const val HUMAN = "human"
}

/**
 * Owns one native engine. Thread-safe: the Rust side serializes all calls (single writer),
 * and listeners are notified after every revision change, whoever caused it.
 */
class PixelEngine(width: Int = 32, height: Int = 32) : AutoCloseable {
    private val handle: Long
    private val listeners = CopyOnWriteArrayList<(Long) -> Unit>()
    @Volatile private var closed = false
    @Volatile private var lastRevision = 0L

    init {
        NativeBridge.ensureLoaded()
        handle = NativeBridge.create(width, height)
        require(handle != 0L) { "invalid canvas size ${width}x$height" }
    }

    val revision: Long get() = if (closed) -1 else NativeBridge.revision(handle)

    fun addChangeListener(listener: (Long) -> Unit) { listeners += listener }
    fun removeChangeListener(listener: (Long) -> Unit) { listeners -= listener }

    /** Executes a command built from typed arguments. */
    fun execute(
        op: String,
        args: Map<String, Any?> = emptyMap(),
        actor: String = Actors.HUMAN,
        operationId: String? = null,
        expectedRevision: Long? = null,
        payload: ByteArray? = null,
    ): EngineResponse {
        val req = LinkedHashMap<String, Any?>()
        req["op"] = op
        req["actor"] = actor
        if (operationId != null) req["operationId"] = operationId
        if (expectedRevision != null) req["expectedRevision"] = expectedRevision
        req["args"] = args
        return executeRaw(Json.write(req).toByteArray(Charsets.UTF_8), payload)
    }

    /** Executes an already-serialized request (used by the REST passthrough). */
    fun executeRaw(request: ByteArray, payload: ByteArray? = null): EngineResponse {
        check(!closed) { "engine is closed" }
        val framed = NativeBridge.call(handle, request, payload)
            ?: return EngineResponse("""{"ok":false,"error":{"code":"internal","message":"native call returned null (out of memory?)"}}""", ByteArray(0))
        val response = decode(framed)
        val rev = response.revision
        if (rev >= 0 && rev != lastRevision) {
            lastRevision = rev
            listeners.forEach { runCatching { it(rev) } }
        }
        return response
    }

    override fun close() {
        if (!closed) {
            closed = true
            NativeBridge.destroy(handle)
        }
    }

    companion object {
        fun decode(framed: ByteArray): EngineResponse {
            if (framed.size < 4) return EngineResponse("""{"ok":false,"error":{"code":"internal","message":"truncated frame"}}""", ByteArray(0))
            val len = (framed[0].toInt() and 0xFF) or ((framed[1].toInt() and 0xFF) shl 8) or
                ((framed[2].toInt() and 0xFF) shl 16) or ((framed[3].toInt() and 0xFF) shl 24)
            if (len < 0 || 4 + len > framed.size) {
                return EngineResponse("""{"ok":false,"error":{"code":"internal","message":"bad frame length"}}""", ByteArray(0))
            }
            val json = String(framed, 4, len, Charsets.UTF_8)
            val binary = framed.copyOfRange(4 + len, framed.size)
            return EngineResponse(json, binary)
        }
    }
}
