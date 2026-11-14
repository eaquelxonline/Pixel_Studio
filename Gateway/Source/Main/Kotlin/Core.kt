// Pixel Studio — shared JVM core: JSON, JNI bridge to the Rust engine, and the agent gateway
// (MCP + REST + function calling, permissions, tool catalogue). Used by Android and Windows.

package com.pixel.studio.gateway

import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.net.URI
import java.net.URLDecoder
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64
import java.util.Collections
import java.util.Locale
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import javax.net.ServerSocketFactory

// ═══ Json ════════════════════════════════════════════════════════════════════════════════════════

/**
 * Minimal, dependency-free JSON for Pixel Studio.
 *
 * Parsed values map to plain Kotlin types:
 * object → [LinkedHashMap] (insertion order kept), array → [List], string → [String],
 * integer → [Long], other numbers → [Double], `true`/`false` → [Boolean], `null` → `null`.
 */
object Json {
    private const val MAX_DEPTH = 128

    class ParseException(message: String) : IllegalArgumentException(message)

    /** Already-serialized JSON that should be embedded verbatim when writing. */
    class Raw(val json: String)

    fun parse(text: String): Any? {
        val p = Parser(text)
        p.skipWs()
        val v = p.value(0)
        p.skipWs()
        if (p.pos != text.length) throw ParseException("unexpected trailing data at ${p.pos}")
        return v
    }

    @Suppress("UNCHECKED_CAST")
    fun parseObject(text: String): Map<String, Any?> =
        parse(text) as? Map<String, Any?> ?: throw ParseException("expected a JSON object")

    fun write(value: Any?): String = StringBuilder(64).also { writeTo(value, it) }.toString()

    fun writeTo(value: Any?, out: StringBuilder) {
        when (value) {
            null -> out.append("null")
            is Raw -> out.append(value.json)
            is String -> writeString(value, out)
            is Boolean -> out.append(if (value) "true" else "false")
            is Int, is Long, is Short, is Byte -> out.append(value.toString())
            is Double -> writeDouble(value, out)
            is Float -> writeDouble(value.toDouble(), out)
            is Number -> writeDouble(value.toDouble(), out)
            is Map<*, *> -> {
                out.append('{')
                var first = true
                for ((k, v) in value) {
                    if (!first) out.append(',')
                    first = false
                    writeString(k.toString(), out)
                    out.append(':')
                    writeTo(v, out)
                }
                out.append('}')
            }
            is Iterable<*> -> {
                out.append('[')
                var first = true
                for (v in value) {
                    if (!first) out.append(',')
                    first = false
                    writeTo(v, out)
                }
                out.append(']')
            }
            is Array<*> -> writeTo(value.asList(), out)
            is IntArray -> writeTo(value.asList(), out)
            is LongArray -> writeTo(value.asList(), out)
            is Enum<*> -> writeString(value.name, out)
            else -> writeString(value.toString(), out)
        }
    }

    private fun writeDouble(d: Double, out: StringBuilder) {
        when {
            d.isNaN() || d.isInfinite() -> out.append("null")
            d == Math.rint(d) && Math.abs(d) < 9.007199254740992E15 -> out.append(d.toLong())
            else -> out.append(d.toString())
        }
    }

    private fun writeString(s: String, out: StringBuilder) {
        out.append('"')
        for (c in s) {
            when (c) {
                '"' -> out.append("\\\"")
                '\\' -> out.append("\\\\")
                '\n' -> out.append("\\n")
                '\r' -> out.append("\\r")
                '\t' -> out.append("\\t")
                ' ' -> out.append("\\u2028")
                ' ' -> out.append("\\u2029")
                else -> if (c < ' ') out.append(String.format("\\u%04x", c.code)) else out.append(c)
            }
        }
        out.append('"')
    }

    private class Parser(val s: String) {
        var pos = 0

        fun skipWs() {
            while (pos < s.length && (s[pos] == ' ' || s[pos] == '\t' || s[pos] == '\n' || s[pos] == '\r')) pos++
        }

        fun fail(msg: String): Nothing = throw ParseException("$msg at $pos")

        fun value(depth: Int): Any? {
            if (depth > MAX_DEPTH) fail("nesting too deep")
            if (pos >= s.length) fail("unexpected end of input")
            return when (s[pos]) {
                'n' -> literal("null", null)
                't' -> literal("true", true)
                'f' -> literal("false", false)
                '"' -> string()
                '[' -> array(depth)
                '{' -> obj(depth)
                '-', in '0'..'9' -> number()
                else -> fail("unexpected character '${s[pos]}'")
            }
        }

        fun literal(lit: String, v: Any?): Any? {
            if (!s.startsWith(lit, pos)) fail("invalid literal")
            pos += lit.length
            return v
        }

        fun array(depth: Int): List<Any?> {
            pos++
            val out = ArrayList<Any?>()
            skipWs()
            if (pos < s.length && s[pos] == ']') { pos++; return out }
            while (true) {
                skipWs()
                out.add(value(depth + 1))
                skipWs()
                if (pos >= s.length) fail("unterminated array")
                when (s[pos++]) {
                    ',' -> continue
                    ']' -> return out
                    else -> fail("expected ',' or ']'")
                }
            }
        }

        fun obj(depth: Int): Map<String, Any?> {
            pos++
            val out = LinkedHashMap<String, Any?>()
            skipWs()
            if (pos < s.length && s[pos] == '}') { pos++; return out }
            while (true) {
                skipWs()
                if (pos >= s.length || s[pos] != '"') fail("expected object key")
                val key = string()
                skipWs()
                if (pos >= s.length || s[pos] != ':') fail("expected ':'")
                pos++
                skipWs()
                out[key] = value(depth + 1)
                skipWs()
                if (pos >= s.length) fail("unterminated object")
                when (s[pos++]) {
                    ',' -> continue
                    '}' -> return out
                    else -> fail("expected ',' or '}'")
                }
            }
        }

        fun number(): Any {
            val start = pos
            if (s[pos] == '-') pos++
            val digits = pos
            while (pos < s.length && s[pos].isAsciiDigit()) pos++
            if (pos == digits) fail("invalid number")
            var integral = true
            if (pos < s.length && s[pos] == '.') {
                integral = false
                pos++
                val f = pos
                while (pos < s.length && s[pos].isAsciiDigit()) pos++
                if (pos == f) fail("invalid fraction")
            }
            if (pos < s.length && (s[pos] == 'e' || s[pos] == 'E')) {
                integral = false
                pos++
                if (pos < s.length && (s[pos] == '+' || s[pos] == '-')) pos++
                val e = pos
                while (pos < s.length && s[pos].isAsciiDigit()) pos++
                if (pos == e) fail("invalid exponent")
            }
            val text = s.substring(start, pos)
            if (integral) text.toLongOrNull()?.let { return it }
            return text.toDouble()
        }

        private fun Char.isAsciiDigit() = this in '0'..'9'

        fun string(): String {
            pos++
            val sb = StringBuilder()
            while (true) {
                if (pos >= s.length) fail("unterminated string")
                val c = s[pos++]
                when {
                    c == '"' -> return sb.toString()
                    c == '\\' -> {
                        if (pos >= s.length) fail("truncated escape")
                        when (val e = s[pos++]) {
                            '"' -> sb.append('"')
                            '\\' -> sb.append('\\')
                            '/' -> sb.append('/')
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000C')
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'u' -> {
                                if (pos + 4 > s.length) fail("truncated \\u escape")
                                val code = s.substring(pos, pos + 4).toIntOrNull(16) ?: fail("invalid \\u escape")
                                pos += 4
                                sb.append(code.toChar())
                            }
                            else -> fail("invalid escape '\\$e'")
                        }
                    }
                    c < ' ' -> fail("control character in string")
                    else -> sb.append(c)
                }
            }
        }
    }
}

// ---- Typed accessors for parsed objects ----------------------------------------------------

@Suppress("UNCHECKED_CAST")
fun Map<String, Any?>.obj(key: String): Map<String, Any?>? = this[key] as? Map<String, Any?>

@Suppress("UNCHECKED_CAST")
fun Map<String, Any?>.list(key: String): List<Any?>? = this[key] as? List<Any?>

fun Map<String, Any?>.str(key: String): String? = this[key] as? String

fun Map<String, Any?>.long(key: String): Long? = when (val v = this[key]) {
    is Long -> v
    is Int -> v.toLong()
    is Double -> if (v == Math.rint(v)) v.toLong() else null
    else -> null
}

fun Map<String, Any?>.bool(key: String): Boolean? = this[key] as? Boolean

/** Small builder: `jsonObject("a" to 1, "b" to listOf(2))`. */
fun jsonObject(vararg pairs: Pair<String, Any?>): LinkedHashMap<String, Any?> = linkedMapOf(*pairs)

// ═══ NativeBridge ════════════════════════════════════════════════════════════════════════════════

/**
 * Raw JNI surface of the Rust core (`libpixcore.so` / `pixcore.dll`).
 *
 * The symbols are implemented by hand in `Native/Core.rs` (mod ffi). Prefer [PixelEngine], which
 * owns the handle lifecycle and decodes responses.
 */
object NativeBridge {
    /** Must equal `pixcore::ffi::ABI_VERSION`. */
    const val EXPECTED_ABI = 1

    @Volatile
    private var loaded = false

    /**
     * Loads the native library once. Resolution order:
     * 1. system property `pixcore.path` or env `PIXCORE_PATH` (absolute file path),
     * 2. `System.loadLibrary("pixcore")` (APK `lib/<abi>/`, or `java.library.path` on desktop).
     */
    @Synchronized
    fun ensureLoaded() {
        if (loaded) return
        val explicit = System.getProperty("pixcore.path") ?: System.getenv("PIXCORE_PATH")
        if (explicit != null && File(explicit).isFile) {
            System.load(File(explicit).absolutePath)
        } else {
            System.loadLibrary("pixcore")
        }
        val abi = abiVersion()
        check(abi == EXPECTED_ABI) { "pixcore ABI mismatch: native=$abi expected=$EXPECTED_ABI" }
        loaded = true
    }

    @JvmStatic external fun abiVersion(): Int

    /** Returns an opaque handle, or 0 if the size is invalid. */
    @JvmStatic external fun create(width: Int, height: Int): Long

    @JvmStatic external fun destroy(handle: Long)

    /** Current revision, or -1 for an invalid handle. Cheap; safe to poll from UI timers. */
    @JvmStatic external fun revision(handle: Long): Long

    /** Framed response: `u32 LE json length | UTF-8 JSON | binary`. */
    @JvmStatic external fun call(handle: Long, request: ByteArray, payload: ByteArray?): ByteArray?
}

// ═══ PixelEngine ═════════════════════════════════════════════════════════════════════════════════

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

// ═══ AgentGateway ════════════════════════════════════════════════════════════════════════════════

data class GatewayConfig(
    /** 127.0.0.1 by default. LAN mode binds 0.0.0.0 and should use TLS ([socketFactory]). */
    val host: String = "127.0.0.1",
    val port: Int = DEFAULT_PORT,
    val token: String = AccessControl.generateToken(),
    val allowedOrigins: Set<String> = emptySet(),
    val socketFactory: ServerSocketFactory = ServerSocketFactory.getDefault(),
) {
    companion object {
        const val DEFAULT_PORT = 38471
    }

    val loopbackOnly: Boolean get() = host == "127.0.0.1" || host == "localhost" || host == "::1"
}

/**
 * The Universal AI Gateway: one HTTP listener serving MCP (`/mcp`) and the REST API
 * (`/api/v1`), both backed by the same [PixelEngine] the human is drawing on.
 */
class AgentGateway(
    private val engine: PixelEngine,
    val config: GatewayConfig = GatewayConfig(),
    workspace: Workspace? = null,
    confirmer: Confirmer = Confirmer.DENY_ALL,
) : AutoCloseable {
    val access = AccessControl(config.token, config.allowedOrigins, config.loopbackOnly)
    val policy = PermissionPolicy(confirmer = confirmer)
    private val executor = ToolExecutor(engine, policy, workspace)
    private val mcp = McpHttpEndpoint(McpServer(executor, ResourceCatalog(engine)))
    private val rest = RestApi(engine, executor) { linkedMapOf("mcpEndpoint" to mcpUrl, "protocolVersions" to McpServer.SUPPORTED) }
    private var server: HttpServer? = null

    val port: Int get() = server?.boundPort ?: config.port
    val mcpUrl: String get() = "${scheme()}://${displayHost()}:$port/mcp"
    val running: Boolean get() = server != null

    private fun scheme() = if (config.socketFactory is javax.net.ssl.SSLServerSocketFactory) "https" else "http"
    private fun displayHost() = if (config.loopbackOnly) "127.0.0.1" else config.host

    @Synchronized
    fun start(): AgentGateway {
        if (server == null) server = HttpServer(config.host, config.port, ::route, config.socketFactory).start()
        return this
    }

    @Synchronized
    fun stop() {
        server?.close()
        server = null
    }

    override fun close() = stop()

    private fun route(req: HttpRequest): HttpResponse {
        if (req.path == "/health") return HttpResponse.json(200, """{"status":"ok","name":"pixel-studio"}""")
        when (val verdict = access.check(req)) {
            is AccessControl.Verdict.Denied -> {
                val headers = if (verdict.status == 401) mapOf("WWW-Authenticate" to "Bearer realm=\"pixel-studio\"") else emptyMap()
                return HttpResponse.json(verdict.status, Json.write(mapOf("ok" to false, "error" to mapOf("code" to "unauthorized", "message" to verdict.reason))), headers)
            }
            AccessControl.Verdict.Allowed -> {}
        }
        return when {
            req.path == "/mcp" -> mcp.handle(req)
            req.path.startsWith("/api/v1/") || req.path == "/api/v1" -> rest.handle(req)
            else -> HttpResponse.json(404, """{"ok":false,"error":{"code":"not_found","message":"unknown path"}}""")
        }
    }

    /** Ready-to-paste connection settings shown in the app's "Connect an agent" dialog. */
    fun connectionSnippets(): Map<String, String> {
        val token = access.token
        return linkedMapOf(
            "Claude Code" to "claude mcp add --transport http pixel-studio $mcpUrl --header \"Authorization: Bearer $token\"",
            "Antigravity / JSON config" to Json.write(
                mapOf("mcpServers" to mapOf("pixel-studio" to mapOf("serverUrl" to mcpUrl, "headers" to mapOf("Authorization" to "Bearer $token")))),
            ),
            "STDIO adapter" to "pixstudio-mcp --url $mcpUrl --token $token",
            "REST" to "curl -H \"Authorization: Bearer $token\" ${mcpUrl.removeSuffix("/mcp")}/api/v1/status",
        )
    }
}

// ═══ HeadlessServer ══════════════════════════════════════════════════════════════════════════════

/**
 * Runs the engine and the agent gateway without a UI:
 * `--size 64x64 --port 38471 --workspace ./workspace [--token TOKEN] [--allow-destructive]`,
 * or `--export-schemas <dir>` to regenerate the tool schema files.
 */
fun main(args: Array<String>) {
    if (args.firstOrNull() == "--export-schemas") return exportSchemas(args.getOrNull(1))
    fun opt(name: String) = args.indexOf(name).takeIf { it >= 0 && it + 1 < args.size }?.let { args[it + 1] }
    val (w, h) = (opt("--size") ?: "32x32").lowercase().split('x').map { it.toInt() }
    val port = opt("--port")?.toInt() ?: GatewayConfig.DEFAULT_PORT
    val workspace = File(opt("--workspace") ?: "workspace").absoluteDir()
    val confirmer = if ("--allow-destructive" in args) Confirmer.APPROVE_ALL else Confirmer.DENY_ALL
    val engine = PixelEngine(w, h)
    val config = opt("--token")?.let { GatewayConfig(port = port, token = it) } ?: GatewayConfig(port = port)
    val gateway = AgentGateway(engine, config, DirectoryWorkspace(workspace), confirmer).start()
    println("Pixel Studio headless gateway - canvas ${w}x$h, workspace ${workspace.path}")
    println("MCP endpoint: ${gateway.mcpUrl}")
    gateway.connectionSnippets().forEach { (k, v) -> println("\n[$k]\n$v") }
    Runtime.getRuntime().addShutdownHook(Thread { gateway.close(); engine.close() })
    Thread.currentThread().join()
}

private fun File.absoluteDir(): File = absoluteFile.also { it.mkdirs() }

// ═══ AccessControl ═══════════════════════════════════════════════════════════════════════════════

/**
 * Bearer-token authentication plus Origin/Host validation (DNS-rebinding protection).
 *
 * Tokens are 256-bit random values generated on the device; they never leave it unless the
 * user copies the connection snippet into an agent's configuration.
 */
class AccessControl(
    token: String = generateToken(),
    /** Extra allowed browser origins, e.g. "https://my-tool.example". Localhost is always allowed. */
    private val allowedOrigins: Set<String> = emptySet(),
    /** When serving only on loopback, also require a loopback Host header. */
    private val loopbackOnly: Boolean = true,
) {
    @Volatile var token: String = token
        private set

    fun rotateToken(): String = generateToken().also { token = it }

    sealed class Verdict {
        object Allowed : Verdict()
        class Denied(val status: Int, val reason: String) : Verdict()
    }

    fun check(request: HttpRequest): Verdict {
        val origin = request.header("origin")
        if (origin != null && origin != "null" && !isAllowedOrigin(origin)) {
            return Verdict.Denied(403, "origin not allowed")
        }
        if (origin == "null") return Verdict.Denied(403, "opaque origin not allowed")
        if (loopbackOnly) {
            val host = request.header("host")?.let { hostOnly(it) }
            if (host != null && host !in LOOPBACK_NAMES) return Verdict.Denied(403, "host not allowed")
        }
        val auth = request.header("authorization") ?: return Verdict.Denied(401, "missing bearer token")
        if (!auth.startsWith("Bearer ", ignoreCase = true)) return Verdict.Denied(401, "expected a bearer token")
        val presented = auth.substring(7).trim()
        val ok = MessageDigest.isEqual(presented.toByteArray(Charsets.UTF_8), token.toByteArray(Charsets.UTF_8))
        return if (ok) Verdict.Allowed else Verdict.Denied(401, "invalid token")
    }

    private fun isAllowedOrigin(origin: String): Boolean {
        if (origin in allowedOrigins) return true
        val host = runCatching { URI(origin).host }.getOrNull() ?: return false
        return host.removePrefix("[").removeSuffix("]") in LOOPBACK_NAMES
    }

    private fun hostOnly(hostHeader: String): String {
        val h = hostHeader.trim()
        if (h.startsWith("[")) return h.substring(1, h.indexOf(']').takeIf { it > 0 } ?: h.length)
        return h.substringBefore(':')
    }

    companion object {
        val LOOPBACK_NAMES = setOf("localhost", "127.0.0.1", "::1")

        fun generateToken(): String {
            val bytes = ByteArray(32)
            SecureRandom().nextBytes(bytes)
            return "pxs_" + Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
        }
    }
}

// ═══ HttpServer ══════════════════════════════════════════════════════════════════════════════════

class HttpRequest(
    val method: String,
    val path: String,
    val query: Map<String, String>,
    /** Header names are lower-cased. */
    val headers: Map<String, String>,
    val body: ByteArray,
    val remoteAddress: InetAddress?,
) {
    fun header(name: String): String? = headers[name.lowercase(Locale.ROOT)]
    fun bodyText(): String = String(body, Charsets.UTF_8)
}

class HttpResponse(
    val status: Int,
    val body: ByteArray = ByteArray(0),
    val contentType: String? = null,
    val headers: Map<String, String> = emptyMap(),
) {
    companion object {
        fun json(status: Int, json: String, headers: Map<String, String> = emptyMap()) =
            HttpResponse(status, json.toByteArray(Charsets.UTF_8), "application/json; charset=utf-8", headers)

        fun text(status: Int, text: String) = HttpResponse(status, text.toByteArray(Charsets.UTF_8), "text/plain; charset=utf-8")

        fun empty(status: Int, headers: Map<String, String> = emptyMap()) = HttpResponse(status, headers = headers)
    }
}

fun interface HttpHandler {
    fun handle(request: HttpRequest): HttpResponse
}

/**
 * Small HTTP/1.1 server on plain `java.net` sockets, usable on both Android and desktop JVMs
 * (no `com.sun.net.httpserver`, no third-party server). Supports keep-alive, Content-Length
 * and chunked request bodies, and strict size limits.
 *
 * Pass an `SSLServerSocketFactory` as [socketFactory] to serve HTTPS (LAN mode).
 */
class HttpServer(
    private val host: String,
    private val port: Int,
    private val handler: HttpHandler,
    private val socketFactory: ServerSocketFactory = ServerSocketFactory.getDefault(),
    private val maxBodyBytes: Int = 32 * 1024 * 1024,
    private val workers: Int = 8,
) : AutoCloseable {
    private var server: ServerSocket? = null
    private var pool: ExecutorService? = null
    private var acceptThread: Thread? = null
    @Volatile private var running = false

    /** The port actually bound (useful with port 0). */
    val boundPort: Int get() = server?.localPort ?: -1

    @Synchronized
    fun start(): HttpServer {
        check(!running) { "already running" }
        val ss = socketFactory.createServerSocket()
        ss.reuseAddress = true
        ss.bind(InetSocketAddress(InetAddress.getByName(host), port), 64)
        server = ss
        pool = Executors.newFixedThreadPool(workers) { r -> Thread(r, "pixel-http-worker").apply { isDaemon = true } }
        running = true
        acceptThread = Thread({ acceptLoop(ss) }, "pixel-http-accept").apply { isDaemon = true; start() }
        return this
    }

    private fun acceptLoop(ss: ServerSocket) {
        while (running) {
            val socket = try { ss.accept() } catch (e: IOException) { if (running) continue else break }
            try {
                pool?.execute { serve(socket) } ?: socket.close()
            } catch (e: Exception) {
                runCatching { socket.close() }
            }
        }
    }

    private fun serve(socket: Socket) {
        socket.use { s ->
            try {
                s.soTimeout = 30_000
                s.tcpNoDelay = true
                val input = BufferedInputStream(s.getInputStream(), 16 * 1024)
                val output = s.getOutputStream()
                while (running) {
                    val request = try {
                        readRequest(input, s.inetAddress) ?: return
                    } catch (e: HttpError) {
                        write(output, HttpResponse.text(e.status, e.message ?: "bad request"), keepAlive = false)
                        return
                    }
                    val response = try {
                        handler.handle(request)
                    } catch (e: Exception) {
                        HttpResponse.text(500, "internal error")
                    }
                    val keepAlive = !request.header("connection").equals("close", ignoreCase = true)
                    write(output, response, keepAlive)
                    if (!keepAlive) return
                }
            } catch (_: SocketTimeoutException) {
            } catch (_: SocketException) {
            } catch (_: IOException) {
            }
        }
    }

    private class HttpError(val status: Int, message: String) : Exception(message)

    private fun readLine(input: InputStream, limit: Int = 8192): String? {
        val buf = ByteArrayOutputStream(128)
        while (true) {
            val b = input.read()
            if (b == -1) return if (buf.size() == 0) null else buf.toString(Charsets.ISO_8859_1.name())
            if (b == '\n'.code) break
            if (b != '\r'.code) buf.write(b)
            if (buf.size() > limit) throw HttpError(431, "header line too long")
        }
        return buf.toString(Charsets.ISO_8859_1.name())
    }

    private fun readRequest(input: InputStream, remote: InetAddress?): HttpRequest? {
        var line = readLine(input) ?: return null
        while (line.isEmpty()) line = readLine(input) ?: return null
        val parts = line.split(' ')
        if (parts.size != 3 || !parts[2].startsWith("HTTP/1.")) throw HttpError(400, "malformed request line")
        val method = parts[0].uppercase(Locale.ROOT)
        val target = parts[1]
        val headers = LinkedHashMap<String, String>()
        while (true) {
            val h = readLine(input) ?: throw HttpError(400, "unexpected end of headers")
            if (h.isEmpty()) break
            if (headers.size >= 100) throw HttpError(431, "too many headers")
            val colon = h.indexOf(':')
            if (colon <= 0) throw HttpError(400, "malformed header")
            val name = h.substring(0, colon).trim().lowercase(Locale.ROOT)
            val value = h.substring(colon + 1).trim()
            headers[name] = headers[name]?.let { "$it, $value" } ?: value
        }
        val body = when {
            headers["transfer-encoding"]?.lowercase(Locale.ROOT)?.contains("chunked") == true -> readChunked(input)
            headers["content-length"] != null -> {
                val len = headers["content-length"]!!.toLongOrNull() ?: throw HttpError(400, "bad content-length")
                if (len < 0 || len > maxBodyBytes) throw HttpError(413, "body too large")
                readExactly(input, len.toInt())
            }
            else -> ByteArray(0)
        }
        val qIndex = target.indexOf('?')
        val rawPath = if (qIndex >= 0) target.substring(0, qIndex) else target
        val query = if (qIndex >= 0) parseQuery(target.substring(qIndex + 1)) else emptyMap()
        val path = URLDecoder.decode(rawPath, "UTF-8")
        return HttpRequest(method, path, query, headers, body, remote)
    }

    private fun readExactly(input: InputStream, n: Int): ByteArray {
        val out = ByteArray(n)
        var off = 0
        while (off < n) {
            val r = input.read(out, off, n - off)
            if (r < 0) throw HttpError(400, "truncated body")
            off += r
        }
        return out
    }

    private fun readChunked(input: InputStream): ByteArray {
        val out = ByteArrayOutputStream()
        while (true) {
            val sizeLine = readLine(input) ?: throw HttpError(400, "truncated chunk")
            val size = sizeLine.substringBefore(';').trim().toIntOrNull(16) ?: throw HttpError(400, "bad chunk size")
            if (size == 0) {
                while (true) { if ((readLine(input) ?: "").isEmpty()) break }
                return out.toByteArray()
            }
            if (size < 0 || out.size() + size > maxBodyBytes) throw HttpError(413, "body too large")
            out.write(readExactly(input, size))
            readLine(input)
        }
    }

    private fun parseQuery(q: String): Map<String, String> = q.split('&').filter { it.isNotEmpty() }.associate {
        val eq = it.indexOf('=')
        if (eq < 0) URLDecoder.decode(it, "UTF-8") to ""
        else URLDecoder.decode(it.substring(0, eq), "UTF-8") to URLDecoder.decode(it.substring(eq + 1), "UTF-8")
    }

    private fun write(out: OutputStream, r: HttpResponse, keepAlive: Boolean) {
        val sb = StringBuilder()
        sb.append("HTTP/1.1 ").append(r.status).append(' ').append(reason(r.status)).append("\r\n")
        if (r.contentType != null) sb.append("Content-Type: ").append(r.contentType).append("\r\n")
        sb.append("Content-Length: ").append(r.body.size).append("\r\n")
        sb.append("Cache-Control: no-store\r\n")
        sb.append("X-Content-Type-Options: nosniff\r\n")
        sb.append("Connection: ").append(if (keepAlive) "keep-alive" else "close").append("\r\n")
        for ((k, v) in r.headers) {
            require('\r' !in k && '\n' !in k && '\r' !in v && '\n' !in v) { "header injection" }
            sb.append(k).append(": ").append(v).append("\r\n")
        }
        sb.append("\r\n")
        out.write(sb.toString().toByteArray(Charsets.ISO_8859_1))
        out.write(r.body)
        out.flush()
    }

    private fun reason(status: Int) = when (status) {
        200 -> "OK"; 202 -> "Accepted"; 204 -> "No Content"; 400 -> "Bad Request"; 401 -> "Unauthorized"
        403 -> "Forbidden"; 404 -> "Not Found"; 405 -> "Method Not Allowed"; 409 -> "Conflict"; 411 -> "Length Required"
        413 -> "Payload Too Large"; 415 -> "Unsupported Media Type"; 431 -> "Request Header Fields Too Large"
        500 -> "Internal Server Error"; else -> "Status"
    }

    override fun close() {
        running = false
        runCatching { server?.close() }
        pool?.shutdownNow()
        pool?.awaitTermination(2, TimeUnit.SECONDS)
    }
}

// ═══ JsonRpc ═════════════════════════════════════════════════════════════════════════════════════

/** JSON-RPC 2.0 helpers and the error codes used by MCP. */
object JsonRpc {
    const val PARSE_ERROR = -32700
    const val INVALID_REQUEST = -32600
    const val METHOD_NOT_FOUND = -32601
    const val INVALID_PARAMS = -32602
    const val INTERNAL_ERROR = -32603

    // MCP 2026-07-28 reserved codes.
    const val HEADER_MISMATCH = -32020
    const val MISSING_CLIENT_CAPABILITY = -32021
    const val UNSUPPORTED_PROTOCOL_VERSION = -32022

    fun result(id: Any?, result: Any?): Map<String, Any?> = linkedMapOf("jsonrpc" to "2.0", "id" to id, "result" to result)

    fun error(id: Any?, code: Int, message: String, data: Any? = null): Map<String, Any?> {
        val err = linkedMapOf<String, Any?>("code" to code, "message" to message)
        if (data != null) err["data"] = data
        return linkedMapOf("jsonrpc" to "2.0", "id" to id, "error" to err)
    }

    /** Valid ids are strings or integers (null only appears in error responses). */
    fun isValidId(id: Any?): Boolean = id is String || id is Long || id is Int
}

/** Thrown by handlers to produce a JSON-RPC error response. */
class RpcException(val code: Int, message: String, val data: Any? = null, val httpStatus: Int = 200) : Exception(message)

// ═══ McpServer ═══════════════════════════════════════════════════════════════════════════════════

/**
 * Transport-independent MCP server ("dual-era"):
 * - Modern revision 2026-07-28: stateless; every request carries its protocol version and
 *   client capabilities in `params._meta`; `server/discover` replaces the handshake.
 * - Legacy revisions (2025-11-25 … 2024-11-05): `initialize` handshake.
 */
class McpServer(
    private val executor: ToolExecutor,
    private val resources: ResourceCatalog,
    private val version: String = "0.1.0",
) {
    companion object {
        const val MODERN = "2026-07-28"
        val LEGACY = listOf("2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05")
        val SUPPORTED = listOf(MODERN) + LEGACY

        const val META_PROTOCOL_VERSION = "io.modelcontextprotocol/protocolVersion"
        const val META_CLIENT_CAPABILITIES = "io.modelcontextprotocol/clientCapabilities"
        const val META_CLIENT_INFO = "io.modelcontextprotocol/clientInfo"
        const val META_SERVER_INFO = "io.modelcontextprotocol/serverInfo"

        /** Static catalogues may be cached for an hour; live canvas data never. */
        const val STATIC_TTL_MS = 3_600_000L
    }

    class Context(val modern: Boolean, val protocolVersion: String, val actor: String)

    val serverInfo: Map<String, Any?> = linkedMapOf("name" to "pixel-studio", "title" to "Pixel Studio", "version" to version)

    private val capabilities: Map<String, Any?> = linkedMapOf(
        "tools" to linkedMapOf("listChanged" to false),
        "resources" to linkedMapOf("subscribe" to false, "listChanged" to false),
        "prompts" to linkedMapOf("listChanged" to false),
    )

    /**
     * Handles one JSON-RPC message. Returns the response, or null for notifications.
     * Protocol errors are returned as JSON-RPC errors; [RpcException.httpStatus] travels in
     * the returned map under the private key `__http` for the HTTP transport.
     */
    fun handle(message: Map<String, Any?>, ctx: Context): Map<String, Any?>? {
        val id = message["id"]
        val method = message["method"] as? String
        if (message["jsonrpc"] != "2.0" || method == null) {
            return JsonRpc.error(if (JsonRpc.isValidId(id)) id else null, JsonRpc.INVALID_REQUEST, "invalid JSON-RPC request")
        }
        val isNotification = !message.containsKey("id")
        if (isNotification) return null // notifications/initialized, notifications/cancelled, ...
        if (!JsonRpc.isValidId(id)) return JsonRpc.error(null, JsonRpc.INVALID_REQUEST, "id must be a string or integer")

        @Suppress("UNCHECKED_CAST")
        val params = (message["params"] as? Map<String, Any?>) ?: emptyMap()
        return try {
            val result = dispatch(method, params, ctx)
            JsonRpc.result(id, if (ctx.modern) decorate(result) else result)
        } catch (e: RpcException) {
            val err = LinkedHashMap(JsonRpc.error(id, e.code, e.message ?: "error", e.data))
            if (e.httpStatus != 200) err["__http"] = e.httpStatus
            err
        } catch (e: UnknownToolException) {
            JsonRpc.error(id, JsonRpc.INVALID_PARAMS, e.message ?: "unknown tool")
        } catch (e: Exception) {
            JsonRpc.error(id, JsonRpc.INTERNAL_ERROR, "internal error: ${e.javaClass.simpleName}")
        }
    }

    private fun decorate(result: Map<String, Any?>): Map<String, Any?> {
        val out = LinkedHashMap<String, Any?>()
        out["resultType"] = "complete"
        out.putAll(result)
        @Suppress("UNCHECKED_CAST")
        val meta = LinkedHashMap((result["_meta"] as? Map<String, Any?>) ?: emptyMap())
        meta[META_SERVER_INFO] = serverInfo
        out["_meta"] = meta
        return out
    }

    private fun cache(result: Map<String, Any?>, ctx: Context, ttlMs: Long, scope: String): Map<String, Any?> =
        if (!ctx.modern) result else LinkedHashMap(result).apply { put("ttlMs", ttlMs); put("cacheScope", scope) }

    private fun dispatch(method: String, params: Map<String, Any?>, ctx: Context): Map<String, Any?> = when (method) {
        "initialize" -> {
            val requested = params.str("protocolVersion")
            val negotiated = if (requested in LEGACY) requested!! else LEGACY.first()
            linkedMapOf(
                "protocolVersion" to negotiated,
                "capabilities" to capabilities,
                "serverInfo" to serverInfo,
                "instructions" to Guidance.SERVER_INSTRUCTIONS,
            )
        }
        "server/discover" -> cache(
            linkedMapOf(
                "supportedVersions" to SUPPORTED,
                "capabilities" to capabilities,
                "instructions" to Guidance.SERVER_INSTRUCTIONS,
            ),
            ctx, STATIC_TTL_MS, "public",
        )
        "ping" -> emptyMap()
        "logging/setLevel" -> emptyMap()
        "tools/list" -> cache(linkedMapOf("tools" to ToolCatalog.tools.map(FunctionSchemas::mcpTool)), ctx, STATIC_TTL_MS, "public")
        "tools/call" -> {
            val name = params.str("name") ?: throw RpcException(JsonRpc.INVALID_PARAMS, "missing tool name")
            val args = params["arguments"]
            if (args != null && args !is Map<*, *>) throw RpcException(JsonRpc.INVALID_PARAMS, "arguments must be an object")
            @Suppress("UNCHECKED_CAST")
            executor.call(name, args as Map<String, Any?>?, ctx.actor).toMcp()
        }
        "resources/list" -> cache(linkedMapOf("resources" to resources.list()), ctx, STATIC_TTL_MS, "public")
        "resources/templates/list" -> cache(linkedMapOf("resourceTemplates" to emptyList<Any>()), ctx, STATIC_TTL_MS, "public")
        "resources/read" -> {
            val uri = params.str("uri") ?: throw RpcException(JsonRpc.INVALID_PARAMS, "missing uri")
            cache(resources.read(uri), ctx, 0, "private")
        }
        "prompts/list" -> cache(linkedMapOf("prompts" to Guidance.listPrompts()), ctx, STATIC_TTL_MS, "public")
        "prompts/get" -> {
            val name = params.str("name") ?: throw RpcException(JsonRpc.INVALID_PARAMS, "missing prompt name")
            Guidance.getPrompt(name, params.obj("arguments") ?: emptyMap())
        }
        else -> throw RpcException(JsonRpc.METHOD_NOT_FOUND, "method not found: $method", httpStatus = if (ctx.modern) 404 else 200)
    }
}

// ═══ McpHttpEndpoint ═════════════════════════════════════════════════════════════════════════════

/**
 * Streamable HTTP transport for [McpServer] (POST-only, JSON responses).
 *
 * Modern (2026-07-28) requests are validated against the `MCP-Protocol-Version`, `Mcp-Method`
 * and `Mcp-Name` headers. Legacy clients get a session id from `initialize` that is used
 * only to remember their name for the history log.
 */
class McpHttpEndpoint(private val server: McpServer) {
    private val legacySessions: MutableMap<String, String> = Collections.synchronizedMap(
        object : LinkedHashMap<String, String>(16, 0.75f, true) {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?) = size > 256
        },
    )
    private val random = SecureRandom()

    fun handle(req: HttpRequest): HttpResponse {
        if (req.method != "POST") {
            return HttpResponse.json(405, Json.write(JsonRpc.error(null, JsonRpc.INVALID_REQUEST, "use POST")), mapOf("Allow" to "POST"))
        }
        val parsed = try {
            Json.parse(req.bodyText())
        } catch (e: Json.ParseException) {
            return HttpResponse.json(400, Json.write(JsonRpc.error(null, JsonRpc.PARSE_ERROR, "parse error: ${e.message}")))
        }
        if (parsed is List<*>) return batch(req, parsed)
        @Suppress("UNCHECKED_CAST")
        val msg = parsed as? Map<String, Any?> ?: return HttpResponse.json(400, Json.write(JsonRpc.error(null, JsonRpc.INVALID_REQUEST, "expected a JSON-RPC object")))
        return single(req, msg)
    }

    /** JSON-RPC batches existed only in 2025-03-26; served with legacy semantics. */
    private fun batch(req: HttpRequest, items: List<*>): HttpResponse {
        if (items.isEmpty()) return HttpResponse.json(400, Json.write(JsonRpc.error(null, JsonRpc.INVALID_REQUEST, "empty batch")))
        val ctx = legacyContext(req)
        val responses = items.mapNotNull { item ->
            @Suppress("UNCHECKED_CAST")
            val m = item as? Map<String, Any?> ?: return@mapNotNull JsonRpc.error(null, JsonRpc.INVALID_REQUEST, "invalid batch item")
            server.handle(m, ctx)?.let { strip(it) }
        }
        return if (responses.isEmpty()) HttpResponse.empty(202) else HttpResponse.json(200, Json.write(responses))
    }

    private fun single(req: HttpRequest, msg: Map<String, Any?>): HttpResponse {
        val method = msg["method"] as? String
        val id = msg["id"]
        val meta = msg.obj("params")?.obj("_meta")
        val modernVersion = meta?.str(McpServer.META_PROTOCOL_VERSION)

        val ctx: McpServer.Context
        val extraHeaders = LinkedHashMap<String, String>()
        if (method != "initialize" && modernVersion != null) {
            if (modernVersion != McpServer.MODERN) {
                return error(400, id, JsonRpc.UNSUPPORTED_PROTOCOL_VERSION, "Unsupported protocol version",
                    mapOf("supported" to McpServer.SUPPORTED, "requested" to modernVersion))
            }
            headerMismatch(req, msg, method, modernVersion)?.let { return error(400, id, JsonRpc.HEADER_MISMATCH, it) }
            if (meta[McpServer.META_CLIENT_CAPABILITIES] !is Map<*, *>) {
                return error(400, id, JsonRpc.INVALID_PARAMS, "missing _meta['${McpServer.META_CLIENT_CAPABILITIES}']")
            }
            val client = meta.obj(McpServer.META_CLIENT_INFO)?.str("name")
            ctx = McpServer.Context(modern = true, protocolVersion = modernVersion, actor = actorName(client))
        } else {
            val headerVersion = req.header("mcp-protocol-version")
            if (method != "initialize" && headerVersion != null && headerVersion !in McpServer.LEGACY) {
                return error(400, id, JsonRpc.UNSUPPORTED_PROTOCOL_VERSION, "Unsupported protocol version",
                    mapOf("supported" to McpServer.SUPPORTED, "requested" to headerVersion))
            }
            ctx = legacyContext(req)
            if (method == "initialize") {
                val session = newSessionId()
                legacySessions[session] = actorName(msg.obj("params")?.obj("clientInfo")?.str("name"))
                extraHeaders["Mcp-Session-Id"] = session
            }
        }

        val response = server.handle(msg, ctx) ?: return HttpResponse.empty(202)
        val status = (response["__http"] as? Int) ?: if (response.containsKey("error") && response["id"] == null) 400 else 200
        return HttpResponse.json(status, Json.write(strip(response)), extraHeaders)
    }

    private fun legacyContext(req: HttpRequest): McpServer.Context {
        val actor = req.header("mcp-session-id")?.let { legacySessions[it] } ?: "mcp-agent"
        val version = req.header("mcp-protocol-version")?.takeIf { it in McpServer.LEGACY } ?: McpServer.LEGACY.first()
        return McpServer.Context(modern = false, protocolVersion = version, actor = actor)
    }

    private fun headerMismatch(req: HttpRequest, msg: Map<String, Any?>, method: String?, version: String): String? {
        val headerVersion = req.header("mcp-protocol-version") ?: return "missing MCP-Protocol-Version header"
        if (headerVersion != version) return "MCP-Protocol-Version header ($headerVersion) does not match _meta ($version)"
        val headerMethod = req.header("mcp-method") ?: return "missing Mcp-Method header"
        if (headerMethod != method) return "Mcp-Method header ($headerMethod) does not match method ($method)"
        val params = msg.obj("params")
        val expectedName = when (method) {
            "tools/call", "prompts/get" -> params?.str("name")
            "resources/read" -> params?.str("uri")
            else -> return null
        }
        val headerName = req.header("mcp-name") ?: return "missing Mcp-Name header"
        if (headerName != expectedName) return "Mcp-Name header does not match the request"
        return null
    }

    private fun error(status: Int, id: Any?, code: Int, message: String, data: Any? = null) =
        HttpResponse.json(status, Json.write(JsonRpc.error(if (JsonRpc.isValidId(id)) id else null, code, message, data)))

    private fun strip(response: Map<String, Any?>): Map<String, Any?> =
        if (response.containsKey("__http")) LinkedHashMap(response).apply { remove("__http") } else response

    private fun newSessionId(): String {
        val b = ByteArray(16)
        random.nextBytes(b)
        return b.joinToString("") { "%02x".format(it) }
    }

    private fun actorName(client: String?): String {
        val clean = client?.filter { it.isLetterOrDigit() || it in "-_. " }?.trim()?.take(48)
        return if (clean.isNullOrEmpty()) "mcp-agent" else "mcp:$clean"
    }
}

// ═══ Guidance ════════════════════════════════════════════════════════════════════════════════════

/** Text that teaches any connected AI how to work as a pixel artist in Pixel Studio. */
object Guidance {
    val SERVER_INSTRUCTIONS = """
        Pixel Studio is a shared pixel-art and animation workspace. A human may be drawing on the same canvas
        while you work. You draw with real pixel tools; nothing is generated for you.

        Workflow:
        1. canvas_inspect → learn size, layers, frames and the current revision.
        2. Plan material ramps (ramp_set / color_ramp name) and split the subject into layers (layer_create: Shadow, Body, Head, ...).
        3. Block in shapes (draw_rectangle/draw_ellipse/fill_region), then refine with set_pixels.
           Batch related edits into one set_pixels or transaction call — each call is one undo step.
        4. Check your work with canvas_snapshot (image) or read_region (ascii text) after each major step.
        5. Animation: frame_duplicate or copy_region the previous pose, then change only what moves.
           Set timing with frame_set_duration, group ranges with animation_tag_create, review with animation_preview.

        3D pixel art (voxels): voxel_model_create → texture_generate/material_set (PBR) → rig_bone_add → voxel_shape /
        voxel_set_many → rig_bind → anim3d_generate (walk/run/idle) → voxel_render to look at it (shadows, bloom, lights) →
        voxel_render_sheet or voxel_render_to_frames with directions=8 for game sprites (also pass="normal" for normal maps).
        Chunky models (40-80 voxels tall) read best as sprites. VFX: build a base once, voxel_model_clone it per frame,
        add the moving voxels (flames, sparks, smoke with alpha) and render the list with voxel_render_sheet models=[...].

        Game production: levels = tileset_generate_blob (47-tile autotile set) → tilemap_create → tilemap_terrain with ASCII
        rows ('#' = ground) → tilemap_bake onto a layer → tilemap_render to look. Mood = layers with layer_set blend (multiply shadows,
        add/screen glow) and render_lit with point lights (torch/campfire), bands=3-5 for crisp pixel steps, fire on unlit_layers.
        Combat data = slice_set hitbox/hurtbox/pivot per frame. Variants = palette_swap. Smooth weapon swings = rotate (any angle uses RotSprite).
        Ship it with project_export format=atlas (Aseprite JSON), godot (SpriteFrames .tres) or tiled (.tmj).

        Collaboration rules:
        - Pass expectedRevision when you edit after reading; on a 'conflict' error re-inspect and retry — never overwrite blindly.
        - Use operationId for retries so nothing is applied twice.
        - Destructive actions (deleting frames/layers, new project, resize) ask the human for approval.
        Coordinates: (0,0) is the top-left pixel. Colors: #RRGGBB or #RRGGBBAA.
    """.trimIndent()

    val PIXEL_ART_GUIDE = """
        # Pixel art guide for agents
        - Readability first: strong silhouette, 1px dark outline (not pure black — a darkened hue of the fill).
        - Limited palette: 3-5 shades per material; shift hue toward warm in highlights and cool in shadows.
        - Avoid "jaggies": lines should step in consistent runs (1-1-1, 2-2-2, 1-2-1...), not random lengths.
        - Avoid pillow shading (light from all edges); pick one light direction (usually top-left).
        - Anti-aliasing only on long curves, never on the outer silhouette for game sprites.
        - Animation: keep volume constant between frames; use squash/stretch sparingly at 16-64px.
          Typical timings: idle 4-6 frames @ 150-200ms, walk 6-8 frames @ 80-120ms, attack: short anticipation, fast strike, longer recovery.
        - Verify consistency by comparing frames with onion skin (canvas_snapshot with onion {"before":1}).

        ## Character workflow (64-256 px) — the order matters
        1. Value plan first: one ramp per material with ramp_set, dark → light, spanning ≥ 45 L (ramp_set reports lightness and warns).
           Light materials sit high on their ramp; keep the figure clearly lighter (or darker) than the background.
        2. Body: draw_figure (pose contrapposto/walk, heads 6-7.5 realistic, 2-3 chibi, layers 5) — use the returned joints for everything else.
        3. Clothes, hair, props: draw_path closed+filled shapes and tapered strokes in each ramp's base tone, on layers stacked between the limbs.
        4. Light: shade_form per material layer from ONE light (inflate for cloth/hair, capsule for limbs/boots, sphere for heads/orbs); depth -1 for far parts.
        5. Design lines: draw_path folds and hair strands in ramp tones, shade for cast shadows (hair on neck, hem on thighs).
        6. One dark silhouette outline, then fix_banding and antialias on the detailed layers.
        7. art_review after each stage; fix every high/medium issue it lists (score ≥ 85, value span ≥ 55) before animating.
    """.trimIndent()

    class PromptDef(val name: String, val title: String, val description: String, val arguments: List<Triple<String, String, Boolean>>, val render: (Map<String, String>) -> String)

    val prompts = listOf(
        PromptDef(
            "draw_sprite", "Draw a sprite", "Step-by-step plan for drawing a character or object pixel by pixel.",
            listOf(Triple("subject", "What to draw, e.g. 'a samurai with a red scarf'", true), Triple("size", "Canvas size, e.g. 64x64", false)),
        ) { a ->
            "Using the Pixel Studio tools, draw ${a["subject"]} on a ${a["size"] ?: "the current"} canvas. " +
                "Start with canvas_inspect, plan a palette of at most 16 colors and the layers you need, block in the silhouette, " +
                "then refine shading and outline with set_pixels. Check the result with canvas_snapshot after each stage and fix problems you see. " +
                "For characters of 64 px and more, follow the guide's character workflow (ramp_set → draw_figure → draw_path → shade_form → art_review) and keep going until art_review scores ≥ 85."
        },
        PromptDef(
            "animate_sprite", "Animate a sprite", "Builds a frame-by-frame animation from the existing drawing.",
            listOf(Triple("animation", "Animation name, e.g. 'run'", true), Triple("frames", "Number of frames", false)),
        ) { a ->
            "Create a '${a["animation"]}' animation with ${a["frames"] ?: "an appropriate number of"} frames from the character on frame 0. " +
                "Plan the key poses first, duplicate the previous frame for each new pose, redraw only the moving parts, keep the character's " +
                "proportions and palette identical, set per-frame durations, tag the range with animation_tag_create and review it with animation_preview."
        },
        PromptDef(
            "build_level", "Build a game level", "Generates an autotiled tilemap level with lighting and exports it for a game engine.",
            listOf(Triple("theme", "Level theme, e.g. 'mossy forest at night'", true), Triple("engine", "godot, tiled or atlas", false)),
        ) { a ->
            "Build a '${a["theme"]}' level: pick a library palette, generate a ground tileset with tileset_generate_blob (tile_size 16, lip 3 for depth), " +
                "create a tilemap sized to the canvas, paint terrain with ASCII rows via tilemap_terrain, bake it to a 'Ground' layer, add props and a " +
                "'Shadow' layer set to multiply, then preview the mood with render_lit (2-3 warm/cool lights, bands 4). Finish with project_export " +
                "format=${a["engine"] ?: "tiled"}."
        },
        PromptDef(
            "concept_prompt", "Prompt for an image model", "Writes a prompt for an AI image model whose output concept_import can turn into clean pixel art.",
            listOf(Triple("character", "Who: age, hair, outfit, props, weapon", true), Triple("view", "e.g. '3/4 view facing left' (default)", false)),
        ) { a ->
            "Write this prompt for the user's image model, then import the result with concept_import:\n" +
                "Pixel art game character sprite, full body, standing idle pose, ${a["view"] ?: "3/4 view facing left"}, ${a["character"]}, crisp hard-edged pixels on a clear " +
                "consistent pixel grid, 128x128 sprite scale, limited palette of about 32 colors, hue-shifted shading, single light source from the top left, clean dark " +
                "selective outline, readable silhouette, no anti-aliasing blur, no gradients, centered, feet fully visible, plain flat solid background in one color " +
                "(#FF00FF magenta), no text, no frame.\nNegative: blurry, painterly, 3d render, smooth gradients, jpeg artifacts, text, watermark, cropped feet, " +
                "multiple characters, busy background, inconsistent pixel size."
        },
        PromptDef(
            "turnaround_prompt", "Prompt for the other directions", "Writes a turnaround prompt (all 8 directions on one sheet) to use with the imported concept as reference.",
            listOf(Triple("character", "Short description of the same character", true)),
        ) { a ->
            "Write this prompt and tell the user to attach the concept image as the reference:\n" +
                "Character turnaround sheet of the SAME pixel art character as the reference image (${a["character"]}): exactly 8 full-body views in one row, evenly " +
                "spaced, same size, same scale, same pixel grid, feet on one baseline: front, front-right 3/4, right side, back-right 3/4, back, back-left 3/4, left side, " +
                "front-left 3/4. Identical outfit, colors, proportions and props in every view; idle standing pose; single light from the top left; plain flat solid " +
                "#FF00FF magenta background; no text, no labels, no frames."
        },
        PromptDef("review_canvas", "Critique the canvas", "Reviews the current artwork and suggests concrete pixel-level fixes.", emptyList()) {
            "Run art_review first and read its value map and issues, then look with canvas_snapshot and read_region. Critique value structure, silhouette, palette, " +
                "shading direction, outline quality and (if animated) frame-to-frame consistency. List concrete fixes with coordinates and the tool for each, " +
                "then apply them only after summarizing your plan, and re-run art_review to confirm."
        },
    )

    fun listPrompts(): List<Map<String, Any?>> = prompts.map { p ->
        linkedMapOf(
            "name" to p.name, "title" to p.title, "description" to p.description,
            "arguments" to p.arguments.map { (n, d, r) -> linkedMapOf("name" to n, "description" to d, "required" to r) },
        )
    }

    fun getPrompt(name: String, args: Map<String, Any?>): Map<String, Any?> {
        val p = prompts.firstOrNull { it.name == name } ?: throw RpcException(JsonRpc.INVALID_PARAMS, "unknown prompt '$name'")
        val strArgs = args.mapValues { it.value?.toString() ?: "" }
        p.arguments.filter { it.third }.forEach { (n, _, _) ->
            if (strArgs[n].isNullOrBlank()) throw RpcException(JsonRpc.INVALID_PARAMS, "missing prompt argument '$n'")
        }
        return linkedMapOf(
            "description" to p.description,
            "messages" to listOf(linkedMapOf("role" to "user", "content" to linkedMapOf("type" to "text", "text" to p.render(strArgs)))),
        )
    }
}

// ═══ ResourceCatalog ═════════════════════════════════════════════════════════════════════════════

/** Read-only context an agent can pull in: project state, palette, current frame, timing, guide. */
class ResourceCatalog(private val engine: PixelEngine) {
    private class Res(val uri: String, val name: String, val title: String, val description: String, val mimeType: String)

    private val resources = listOf(
        Res("pixelstudio://project/current", "project", "Current project", "Canvas size, layers, frames, tags and revision.", "application/json"),
        Res("pixelstudio://project/palette", "palette", "Palette", "Palette colors with indices.", "application/json"),
        Res("pixelstudio://frame/current", "frame", "Current frame image", "PNG of the active frame (all visible layers).", "image/png"),
        Res("pixelstudio://animation/current", "animation", "Animation timing", "Playback order, durations and FPS of all frames.", "application/json"),
        Res("pixelstudio://guide/pixel-art", "guide", "Pixel art guide", "Craft rules for clean pixel art and animation.", "text/markdown"),
    )

    fun list(): List<Map<String, Any?>> = resources.map {
        linkedMapOf("uri" to it.uri, "name" to it.name, "title" to it.title, "description" to it.description, "mimeType" to it.mimeType)
    }

    fun read(uri: String): Map<String, Any?> {
        val res = resources.firstOrNull { it.uri == uri } ?: throw RpcException(JsonRpc.INVALID_PARAMS, "resource not found: $uri", mapOf("uri" to uri))
        val content: Map<String, Any?> = when (res.name) {
            "project" -> textContent(res, Json.write(engine.execute("canvas_inspect").result))
            "palette" -> textContent(res, Json.write(engine.execute("canvas_inspect", mapOf("detail" to "palette")).result))
            "animation" -> textContent(res, Json.write(engine.execute("canvas_inspect", mapOf("detail" to "animation")).result))
            "guide" -> textContent(res, Guidance.PIXEL_ART_GUIDE)
            else -> {
                val png = engine.execute("canvas_snapshot").binary
                linkedMapOf("uri" to res.uri, "mimeType" to res.mimeType, "blob" to Base64.getEncoder().encodeToString(png))
            }
        }
        return linkedMapOf("contents" to listOf(content))
    }

    private fun textContent(res: Res, text: String) = linkedMapOf<String, Any?>("uri" to res.uri, "mimeType" to res.mimeType, "text" to text)
}

// ═══ RestApi ═════════════════════════════════════════════════════════════════════════════════════

/**
 * Universal Pixel API (`/api/v1`) for agents that speak plain HTTP or function calling.
 * Everything ends up in the same engine as MCP and the human UI.
 */
class RestApi(
    private val engine: PixelEngine,
    private val executor: ToolExecutor,
    private val gatewayInfo: () -> Map<String, Any?>,
) {
    fun handle(req: HttpRequest): HttpResponse {
        val path = req.path.removePrefix("/api/v1").trimEnd('/').ifEmpty { "/" }
        val segments = path.split('/').filter { it.isNotEmpty() }
        val actor = actor(req)
        return try {
            when {
                req.method == "GET" && path == "/status" -> ok(LinkedHashMap(engine.execute("status").result).apply { put("gateway", gatewayInfo()) })
                req.method == "GET" && path == "/project" -> engineJson(engine.execute("canvas_inspect", mapOf("detail" to "full")))
                req.method == "GET" && path == "/canvas" -> engineJson(engine.execute("canvas_inspect"))
                req.method == "GET" && segments.size == 2 && segments[0] == "frame" ->
                    engineJson(engine.execute("canvas_inspect", mapOf("detail" to "frame", "frame" to segments[1].toLongOrNull())))
                req.method == "GET" && segments.size == 3 && segments[0] == "frame" && segments[2] == "pixels" ->
                    engineJson(engine.execute("read_region", queryArgs(req) + ("frame" to segments[1].toLongOrNull())))
                req.method == "GET" && path == "/snapshot" -> binary(engine.execute("export", queryArgs(req) + ("format" to "png")), "image/png")
                req.method == "GET" && path == "/tools" -> {
                    val format = FunctionSchemas.parseFormat(req.query["format"]) ?: return badRequest("format must be mcp, openai, anthropic or gemini")
                    HttpResponse.json(200, Json.write(FunctionSchemas.export(format)))
                }
                req.method == "POST" && segments.size == 2 && segments[0] == "tools" -> callTool(segments[1], req, actor)
                req.method == "POST" && path == "/commands" -> commands(req, actor)
                req.method == "POST" && path == "/frames" -> callTool("frame_add", req, actor)
                req.method == "POST" && path == "/animations" -> callTool("animation_tag_create", req, actor)
                req.method == "POST" && path == "/export" -> export(req, actor)
                else -> HttpResponse.json(404, Json.write(mapOf("ok" to false, "error" to mapOf("code" to "not_found", "message" to "no route ${req.method} ${req.path}"))))
            }
        } catch (e: Json.ParseException) {
            badRequest("malformed JSON body: ${e.message}")
        }
    }

    private fun actor(req: HttpRequest): String {
        val raw = req.header("x-pixel-actor")?.filter { it.isLetterOrDigit() || it in "-_.: " }?.trim()?.take(48)
        return if (raw.isNullOrEmpty()) "rest-client" else "api:$raw"
    }

    private fun queryArgs(req: HttpRequest): Map<String, Any?> = req.query.mapValues { (_, v) ->
        v.toLongOrNull() ?: when (v) { "true" -> true; "false" -> false; else -> v }
    }

    private fun body(req: HttpRequest): Map<String, Any?> = if (req.body.isEmpty()) emptyMap() else Json.parseObject(req.bodyText())

    private fun ok(result: Any?) = HttpResponse.json(200, Json.write(mapOf("ok" to true, "result" to result)))

    private fun badRequest(message: String) =
        HttpResponse.json(400, Json.write(mapOf("ok" to false, "error" to mapOf("code" to "invalid_request", "message" to message))))

    private fun statusFor(r: EngineResponse) = when {
        r.ok -> 200
        r.errorCode == "conflict" -> 409
        r.errorCode == "not_found" -> 404
        r.errorCode == "permission_denied" -> 403
        r.errorCode == "internal" -> 500
        else -> 400
    }

    private fun engineJson(r: EngineResponse) = HttpResponse.json(statusFor(r), r.raw)

    private fun binary(r: EngineResponse, mime: String) =
        if (r.ok) HttpResponse(200, r.binary, mime, mapOf("X-Pixel-Revision" to r.revision.toString())) else engineJson(r)

    /** Function-calling bridge: the model's tool call arguments are the request body. */
    private fun callTool(name: String, req: HttpRequest, actor: String): HttpResponse = try {
        val result = executor.call(name, body(req), actor)
        HttpResponse.json(if (result.isError) 400 else 200, Json.write(result.toJson()))
    } catch (e: UnknownToolException) {
        HttpResponse.json(404, Json.write(mapOf("ok" to false, "error" to mapOf("code" to "unknown_tool", "message" to e.message))))
    }

    /**
     * Raw engine commands: a single request object, an array of them, or {"commands": [...]}.
     * The same permission policy as MCP applies to every operation.
     */
    @Suppress("UNCHECKED_CAST")
    private fun commands(req: HttpRequest, actor: String): HttpResponse {
        val parsed = Json.parse(req.bodyText())
        val list: List<Map<String, Any?>> = when {
            parsed is List<*> -> parsed.map { it as? Map<String, Any?> ?: return badRequest("each command must be an object") }
            parsed is Map<*, *> && parsed["commands"] is List<*> -> (parsed["commands"] as List<*>).map { it as? Map<String, Any?> ?: return badRequest("each command must be an object") }
            parsed is Map<*, *> -> listOf(parsed as Map<String, Any?>)
            else -> return badRequest("expected a command object or array")
        }
        if (list.size > 1000) return badRequest("at most 1000 commands per request")
        val responses = list.map { cmd ->
            val op = cmd.str("op") ?: return badRequest("command without 'op'")
            if (op == "project_load") return badRequest("project_load is not available over the API")
            val access = if (op == "transaction") {
                val inner = ((cmd["args"] as? Map<*, *>)?.get("commands") ?: cmd["commands"]) as? List<*>
                if (inner?.any { ToolCatalog.accessForOp(((it as? Map<*, *>)?.get("op") as? String) ?: "") == Access.DESTRUCTIVE } == true) Access.DESTRUCTIVE else Access.WRITE
            } else ToolCatalog.accessForOp(op)
            val refusal = executor.policy.authorize(actor, op, access, "REST command $op")
            if (refusal != null) {
                mapOf("ok" to false, "op" to op, "error" to mapOf("code" to "permission_denied", "message" to refusal))
            } else {
                val args = (cmd["args"] as? Map<String, Any?>) ?: cmd.filterKeys { it !in setOf("op", "actor", "operationId", "expectedRevision") }
                val r = engine.execute(op, args, actor, cmd.str("operationId"), cmd.long("expectedRevision"))
                val out = LinkedHashMap(r.json)
                if (r.binary.isNotEmpty()) out["binaryBase64"] = Base64.getEncoder().encodeToString(r.binary)
                out
            }
        }
        val body = if (parsed is Map<*, *> && parsed["commands"] !is List<*>) responses.first() else mapOf("responses" to responses)
        val allOk = responses.all { it["ok"] == true }
        val firstError = responses.firstOrNull { it["ok"] != true }
        val status = when {
            allOk -> 200
            (firstError?.get("error") as? Map<*, *>)?.get("code") == "conflict" -> 409
            (firstError?.get("error") as? Map<*, *>)?.get("code") == "permission_denied" -> 403
            else -> 400
        }
        return HttpResponse.json(status, Json.write(body))
    }

    private fun export(req: HttpRequest, actor: String): HttpResponse {
        val args = LinkedHashMap(body(req))
        val format = args["format"] ?: "png"
        val (op, mime) = when (format) {
            "png", "spritesheet" -> "export" to "image/png"
            "gif" -> "export" to "image/gif"
            "pxs" -> "project_save" to "application/octet-stream"
            else -> return badRequest("format must be png, gif, spritesheet or pxs")
        }
        if (op == "project_save") args.remove("format") else args["format"] = format
        val r = engine.execute(op, args, actor)
        if (!r.ok) return engineJson(r)
        val headers = LinkedHashMap<String, String>()
        headers["X-Pixel-Revision"] = r.revision.toString()
        if (format == "spritesheet") headers["X-Pixel-Metadata"] = Base64.getEncoder().encodeToString(Json.write(r.result).toByteArray())
        return HttpResponse(200, r.binary, mime, headers)
    }
}

// ═══ ToolCatalog ═════════════════════════════════════════════════════════════════════════════════

/** How dangerous a tool is; drives the default permission decision. */
enum class Access { READ, WRITE, DESTRUCTIVE }

/** What the tool returns besides JSON. */
enum class Output { JSON, PNG_IMAGE, GIF_IMAGE, PROJECT_SAVE, PROJECT_EXPORT }

class ToolDef(
    val name: String,
    val title: String,
    val description: String,
    /** Engine operation executed by this tool. */
    val op: String,
    val properties: Map<String, Map<String, Any?>>,
    val required: List<String>,
    val access: Access,
    val output: Output = Output.JSON,
    val category: String,
    /** Arguments always sent to the engine (e.g. a fixed export format). */
    val fixed: Map<String, Any?> = emptyMap(),
) {
    /** JSON Schema (2020-12 compatible) for the arguments. */
    fun inputSchema(): Map<String, Any?> {
        val props = LinkedHashMap(properties)
        if (access != Access.READ) {
            props["expectedRevision"] = integer("Optimistic concurrency: fail with 'conflict' if the canvas revision differs (get it from canvas_inspect or any previous result).", min = 0)
            props["operationId"] = string("Idempotency key: retrying with the same id never applies the change twice.")
        }
        val schema = linkedMapOf<String, Any?>("type" to "object", "properties" to props)
        if (required.isNotEmpty()) schema["required"] = required
        schema["additionalProperties"] = false
        return schema
    }

    fun annotations(): Map<String, Any?> = linkedMapOf(
        "title" to title,
        "readOnlyHint" to (access == Access.READ),
        "destructiveHint" to (access == Access.DESTRUCTIVE),
        "idempotentHint" to (access == Access.READ),
        "openWorldHint" to false,
    )
}

// ---- schema helpers ---------------------------------------------------------------------

fun integer(description: String, min: Long? = null, max: Long? = null): Map<String, Any?> =
    linkedMapOf<String, Any?>("type" to "integer", "description" to description).apply {
        if (min != null) put("minimum", min)
        if (max != null) put("maximum", max)
    }

fun string(description: String, enum: List<String>? = null): Map<String, Any?> =
    linkedMapOf<String, Any?>("type" to "string", "description" to description).apply { if (enum != null) put("enum", enum) }

fun boolean(description: String): Map<String, Any?> = linkedMapOf("type" to "boolean", "description" to description)

fun color(description: String = "True color as #RRGGBB or #RRGGBBAA (any of 16.7M colors + 8-bit alpha), or 'transparent'."): Map<String, Any?> =
    linkedMapOf("type" to "string", "description" to description, "pattern" to "^(#?([0-9A-Fa-f]{3,4}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})|transparent)$")

internal val LAYER = linkedMapOf<String, Any?>(
    "type" to listOf("integer", "string"),
    "description" to "Layer index (0 = bottom) or exact layer name. Defaults to the active layer.",
)
internal val FRAME = integer("0-based frame index. Defaults to the active frame.", min = 0)
internal val X = integer("X coordinate (0 = left).")
internal val Y = integer("Y coordinate (0 = top).")

internal fun target() = linkedMapOf("layer" to LAYER, "frame" to FRAME)

internal fun region(): LinkedHashMap<String, Map<String, Any?>> = linkedMapOf(
    "x" to integer("Region left (default 0)."),
    "y" to integer("Region top (default 0)."),
    "width" to integer("Region width (default: to the canvas edge).", min = 1),
    "height" to integer("Region height (default: to the canvas edge).", min = 1),
)

internal fun props(vararg groups: Map<String, Map<String, Any?>>): LinkedHashMap<String, Map<String, Any?>> =
    LinkedHashMap<String, Map<String, Any?>>().apply { groups.forEach { putAll(it) } }

/**
 * The tool catalogue shared by MCP (`tools/list`), REST (`/api/v1/tools`) and the
 * function-calling adapters. It is exported to `Gateway/Schemas/pixel-studio-tools.*.json`.
 */
object ToolCatalog {
    val tools: List<ToolDef> = buildList<ToolDef> {
        fun tool(
            name: String, title: String, category: String, access: Access, description: String,
            properties: Map<String, Map<String, Any?>> = emptyMap(), required: List<String> = emptyList(),
            op: String = name, output: Output = Output.JSON, fixed: Map<String, Any?> = emptyMap(),
        ) = add(ToolDef(name, title, description, op, properties, required, access, output, category, fixed))

        // -- Canvas ---------------------------------------------------------------------
        tool("canvas_inspect", "Inspect canvas", "canvas", Access.READ,
            "Canvas size, current revision, layers (with pixel density and blend), frames, tags and active layer/frame. Call this first and whenever you need the latest revision. " +
                "detail: summary (default) | full (pixel counts, storage) | frame (one frame's cels and tags) | animation (playback order, durations, FPS of a tag/range) | palette (swatches, material ramps, palette library) | gamedev (slices, tilesets, tilemaps).",
            linkedMapOf("detail" to string("What to inspect.", listOf("summary", "full", "frame", "animation", "palette", "gamedev")), "frame" to FRAME,
                "tag" to string("detail=animation: tag name."), "from" to integer("detail=animation: first frame.", 0), "to" to integer("detail=animation: last frame.", 0),
                "direction" to string("detail=animation: direction.", listOf("forward", "reverse", "pingpong"))))
        tool("canvas_snapshot", "Look at the canvas", "canvas", Access.READ,
            "Your eyes: returns a PNG image of a frame (all visible layers, upscaled for viewing) plus a JSON summary of content bounds and used colors. Use after drawing to check your work.",
            props(linkedMapOf("frame" to FRAME, "scale" to integer("Integer upscale factor 1-64 (default: auto to ~512px).", 1, 64),
                "onion" to linkedMapOf("type" to "object", "description" to "Onion skin: {before, after, opacity 0-255, tint}. Shows neighbouring frames faintly underneath.",
                    "properties" to linkedMapOf("before" to integer("Previous frames to show.", 0, 8), "after" to integer("Next frames to show.", 0, 8),
                        "opacity" to integer("Opacity of the nearest onion frame.", 0, 255), "tint" to boolean("Tint previous red and next blue."))),
                "background" to color("Optional background color for the image."))), output = Output.PNG_IMAGE)
        tool("get_pixel", "Read pixel", "canvas", Access.READ, "Reads one pixel's color from a layer (or the composited frame with composite=true).",
            props(linkedMapOf("x" to X, "y" to Y), target(), linkedMapOf("composite" to boolean("Read the final image of all visible layers instead of one layer."))), listOf("x", "y"))
        tool("read_region", "Read pixels", "canvas", Access.READ,
            "Reads a rectangle of pixels (max 128x128) as text. format 'ascii' (default) gives one string per row with a color legend ('.' = transparent) — ideal for reasoning about shapes; 'hex' gives exact colors.",
            props(region(), target(), linkedMapOf("format" to string("Output format.", listOf("ascii", "hex")), "composite" to boolean("Read the composited frame."))))
        tool("set_pixel", "Paint pixel", "canvas", Access.WRITE, "Paints (or with color 'transparent' erases) a single pixel. On a density 2/4 layer, x/y address its finer grid.",
            props(linkedMapOf("x" to X, "y" to Y, "color" to color()), target()), listOf("x", "y", "color"))
        tool("set_pixels", "Paint pixels", "canvas", Access.WRITE,
            "Paints many explicit pixels in one undoable step. Each entry is [x, y, color] or {x, y, color}; entries without a color use 'color'. Use null/'transparent' to erase. Up to 262144 pixels.",
            props(linkedMapOf("pixels" to linkedMapOf("type" to "array", "description" to "Pixels: [[x, y, \"#RRGGBB\"], ...] or [{\"x\":..,\"y\":..,\"color\":..}, ...].",
                "items" to linkedMapOf("type" to listOf("array", "object"))), "color" to color("Default color for entries without one.")), target()), listOf("pixels"))
        tool("draw_line", "Draw line", "canvas", Access.WRITE, "Draws a 1-pixel Bresenham line including both end points.",
            props(linkedMapOf("x0" to X, "y0" to Y, "x1" to X, "y1" to Y, "color" to color()), target()), listOf("x0", "y0", "x1", "y1", "color"))
        tool("draw_rectangle", "Draw rectangle", "canvas", Access.WRITE, "Draws a rectangle outline, or a filled rectangle with filled=true.",
            props(linkedMapOf("x" to X, "y" to Y, "width" to integer("Width.", 1), "height" to integer("Height.", 1), "color" to color(), "filled" to boolean("Fill the inside.")), target()),
            listOf("x", "y", "width", "height", "color"))
        tool("draw_ellipse", "Draw ellipse", "canvas", Access.WRITE, "Draws a pixel-perfect ellipse inscribed in the given box (outline or filled).",
            props(linkedMapOf("x" to X, "y" to Y, "width" to integer("Width.", 1), "height" to integer("Height.", 1), "color" to color(), "filled" to boolean("Fill the inside.")), target()),
            listOf("x", "y", "width", "height", "color"))
        tool("fill_region", "Fill area", "canvas", Access.WRITE, "Bucket fill: replaces the connected area of the seed pixel's color (4-connected). contiguous=false replaces that color everywhere on the layer.",
            props(linkedMapOf("x" to X, "y" to Y, "color" to color(), "contiguous" to boolean("Default true.")), target()), listOf("x", "y", "color"))
        tool("flip", "Flip", "canvas", Access.WRITE, "Mirrors a region (default whole layer) left↔right (axis horizontal) or top↕bottom (axis vertical).",
            props(linkedMapOf("axis" to string("Default horizontal.", listOf("horizontal", "vertical"))), region(), target()))
        tool("rotate", "Rotate", "canvas", Access.WRITE,
            "Rotates clockwise. 90/180/270 rotate a region exactly (90/270 need a square region); any other angle (or method rotsprite) rotates the cel with RotSprite — clean lines, no blur, no new colors (swords, swing frames).",
            props(linkedMapOf("degrees" to linkedMapOf<String, Any?>("type" to "number", "description" to "Clockwise degrees."), "method" to string("auto (default) or rotsprite.", listOf("auto", "rotsprite")),
                "center" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf("type" to "number"), "minItems" to 2, "maxItems" to 2, "description" to "RotSprite pivot [x, y] (default: content center).")),
                region(), target()), listOf("degrees"))
        tool("move_region", "Move pixels", "canvas", Access.WRITE, "Moves the pixels of a region by (dx, dy) on the same layer. mode 'over' keeps destination pixels under transparent source pixels.",
            props(region(), linkedMapOf("dx" to integer("Horizontal offset."), "dy" to integer("Vertical offset."), "mode" to string("Paste mode (default 'over').", listOf("over", "replace"))), target()),
            listOf("dx", "dy"))
        tool("copy_region", "Copy pixels", "canvas", Access.WRITE,
            "Copies a region to another position, layer and/or frame. Ideal for building a new animation frame from the previous pose, then editing only what moves.",
            props(region(), target(), linkedMapOf("toX" to integer("Destination left (default: same x)."), "toY" to integer("Destination top (default: same y)."),
                "toLayer" to LAYER, "toFrame" to integer("Destination frame index.", 0), "mode" to string("Paste mode (default 'over').", listOf("over", "replace")))))
        tool("clear", "Clear", "canvas", Access.WRITE, "Erases a region (default: the whole cel).", props(region(), target()))
        tool("canvas_resize", "Resize canvas", "canvas", Access.DESTRUCTIVE, "Changes the canvas size; pixels outside the new bounds are cropped. offsetX/offsetY shift existing art.",
            linkedMapOf("width" to integer("New width.", 1, 4096), "height" to integer("New height.", 1, 4096), "offsetX" to integer("Shift existing pixels right."), "offsetY" to integer("Shift existing pixels down.")),
            listOf("width", "height"))

        // -- Animation ------------------------------------------------------------------
        tool("frame_add", "Add frames", "animation", Access.WRITE, "Adds one or more empty frames (no fixed limit). Defaults: appended at the end, duration of the active frame.",
            linkedMapOf("index" to integer("Insert position.", 0), "count" to integer("How many frames (1-10000).", 1, 10000), "duration" to integer("Duration in ms (1-60000).", 1, 60000)))
        tool("frame_duplicate", "Duplicate frame", "animation", Access.WRITE, "Copies a frame with all its layers (stored efficiently until edited). Default target: right after the source.",
            linkedMapOf("frame" to FRAME, "index" to integer("Position of the copy.", 0)))
        tool("frame_delete", "Delete frame", "animation", Access.DESTRUCTIVE, "Deletes a frame and its pixels. Requires the user's approval.",
            linkedMapOf("frame" to integer("Frame index to delete.", 0)), listOf("frame"))
        tool("frame_move", "Move frame", "animation", Access.WRITE, "Reorders a frame.", linkedMapOf("frame" to FRAME, "index" to integer("New position.", 0)), listOf("index"))
        tool("frame_set_duration", "Set frame duration", "animation", Access.WRITE, "Sets how long a frame is shown (ms). frame='all' sets every frame.",
            linkedMapOf("frame" to linkedMapOf("type" to listOf("integer", "string"), "description" to "Frame index, or \"all\"."), "duration" to integer("Milliseconds (1-60000).", 1, 60000)),
            listOf("duration"))
        tool("animation_tag_create", "Create animation tag", "animation", Access.WRITE, "Names a frame range as an animation (e.g. Idle, Walk, Attack) with a playback direction.",
            linkedMapOf("name" to string("Tag name."), "from" to integer("First frame index.", 0), "to" to integer("Last frame index.", 0),
                "direction" to string("Playback direction.", listOf("forward", "reverse", "pingpong")), "repeat" to integer("0 = loop forever.", 0, 65535)),
            listOf("name", "from", "to"))
        tool("animation_tag_delete", "Delete animation tag", "animation", Access.WRITE, "Removes a tag (frames are kept).", linkedMapOf("name" to string("Tag name.")), listOf("name"))
        tool("animation_set_loop", "Set loop mode", "animation", Access.WRITE, "Sets a tag's direction (forward/reverse/pingpong) and repeat count.",
            linkedMapOf("tag" to string("Tag name."), "direction" to string("Direction.", listOf("forward", "reverse", "pingpong")), "repeat" to integer("0 = loop forever.", 0, 65535)),
            listOf("tag"))
        tool("animation_preview", "Preview animation", "animation", Access.READ, "Renders the animation (or a tag) as an animated GIF so you can review motion.",
            linkedMapOf("tag" to string("Tag name."), "from" to integer("First frame.", 0), "to" to integer("Last frame.", 0), "scale" to integer("Upscale 1-64.", 1, 64)),
            op = "export", output = Output.GIF_IMAGE, fixed = mapOf("format" to "gif"))

        // -- Layers ---------------------------------------------------------------------
        tool("layer_create", "Create layer", "layers", Access.WRITE, "Adds a layer (default: above the active one) and selects it. Split characters into parts (Body, Head, Weapon, ...) so they can be edited independently.",
            linkedMapOf("name" to string("Layer name."), "index" to integer("Position (0 = bottom).", 0)))
        tool("layer_delete", "Delete layer", "layers", Access.DESTRUCTIVE, "Deletes a layer and all its pixels in every frame. Requires the user's approval.",
            linkedMapOf("layer" to LAYER), listOf("layer"))
        tool("layer_set", "Layer properties", "layers", Access.WRITE,
            "Changes any layer property in one call: name, visible, locked, opacity, blend (normal, multiply for shadows, screen/add for glow, overlay, darken, lighten, soft_light) " +
                "and density — 1, 2 or 4 pixels per canvas pixel. A density 2/4 layer holds half/quarter-size pixels (eye shines, strands, fine trims) on top of normal ones; changing density resamples the art.",
            linkedMapOf("layer" to LAYER, "name" to string("New name."), "visible" to boolean("Show or hide."), "locked" to boolean("Lock against edits."),
                "opacity" to integer("0-255.", 0, 255), "blend" to string("Blend mode.", listOf("normal", "multiply", "screen", "add", "overlay", "darken", "lighten", "soft_light")),
                "density" to linkedMapOf<String, Any?>("type" to "integer", "enum" to listOf(1, 2, 4), "description" to "Pixels per canvas pixel.")))
        tool("layer_move", "Move layer", "layers", Access.WRITE, "Changes a layer's stacking position.", linkedMapOf("layer" to LAYER, "index" to integer("New position (0 = bottom).", 0)), listOf("index"))

        // -- Palette --------------------------------------------------------------------
        tool("palette_set", "Set palette", "palette", Access.WRITE,
            "With index+color: changes a swatch and recolors every pixel that used it (color variants). With preset (endesga-32/64, resurrect-64, sweetie-16, apollo, aap-64, pico-8, lospec500, vinik24 …) or colors: loads swatches, mode replace (default) or add.",
            linkedMapOf("index" to integer("Swatch index (1+).", 1), "color" to color(), "preset" to string("Library palette name (see canvas_inspect detail=palette)."),
                "colors" to linkedMapOf("type" to "array", "items" to color(), "description" to "Explicit colors."), "mode" to string("replace (default) or add.", listOf("replace", "add"))))

        // -- Project --------------------------------------------------------------------
        tool("project_save", "Save project", "project", Access.WRITE, "Saves the project (.pxs) to its file in the Pixel Studio workspace.",
            linkedMapOf("name" to string("Optional file name (letters, digits, - and _ only).")), op = "project_save", output = Output.PROJECT_SAVE)
        tool("project_export", "Export", "project", Access.WRITE,
            "Exports into the workspace's Exports folder: png (one frame), gif (animation), spritesheet (PNG + JSON), atlas (trimmed, de-duplicated MaxRects PNG + Aseprite-compatible JSON with tags and slices for Phaser/PixiJS/Unity/Godot), godot (atlas PNG + Godot 4 SpriteFrames .tres), tiled (tileset PNG + .tmj map with Wang terrain), tileset (tileset atlas PNG), normal_map (PNG for 2D lighting).",
            linkedMapOf("format" to string("Export format.", listOf("png", "gif", "spritesheet", "atlas", "godot", "tiled", "tileset", "normal_map")), "name" to string("File name without extension."),
                "tilemap" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Tilemap (format tiled)."), "layer" to LAYER,
                "trim" to boolean("Atlas: trim transparent borders (default true)."), "extrude" to integer("Atlas: edge extrusion px.", 0, 16),
                "frame" to FRAME, "tag" to string("Tag to export (gif/spritesheet)."), "scale" to integer("Upscale 1-64.", 1, 64),
                "columns" to integer("Sprite sheet columns (0 = auto).", 0), "padding" to integer("Sprite sheet padding in px.", 0, 64),
                "tileset" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Tileset (format tileset)."),
                "strength" to linkedMapOf<String, Any?>("type" to "number", "description" to "Normal map bump strength (default 3)."),
                "bevel" to linkedMapOf<String, Any?>("type" to "number", "description" to "Normal map edge roundness px (default 3)."),
                "density" to linkedMapOf<String, Any?>("type" to "integer", "enum" to listOf(1, 2, 4), "description" to "Output pixels per canvas pixel (default: finest layer).")),
            listOf("format"), op = "export", output = Output.PROJECT_EXPORT)
        tool("project_new", "New project", "project", Access.DESTRUCTIVE, "Replaces the current project with a new blank canvas (undoable). Requires the user's approval.",
            linkedMapOf("width" to integer("Width 1-4096.", 1, 4096), "height" to integer("Height 1-4096.", 1, 4096), "name" to string("Project name.")), listOf("width", "height"))
        tool("project_rename", "Rename project", "project", Access.WRITE, "Renames the project.", linkedMapOf("name" to string("New name.")), listOf("name"))
        tool("history_undo", "Undo", "project", Access.WRITE, "Undoes the last change(s), whoever made them.", linkedMapOf("steps" to integer("1-256.", 1, 256)))
        tool("history_redo", "Redo", "project", Access.WRITE, "Redoes undone change(s).", linkedMapOf("steps" to integer("1-256.", 1, 256)))
        tool("history_list", "History", "project", Access.READ, "Recent operations with actor, revision and changed pixel counts.", linkedMapOf("limit" to integer("1-1000.", 1, 1000)))
        tool("transaction", "Atomic batch", "project", Access.WRITE,
            "Runs several drawing/structure commands as ONE atomic, undoable step: if any fails, nothing is applied. Each command is {\"op\": <tool name>, ...its arguments}.",
            linkedMapOf("commands" to linkedMapOf("type" to "array", "description" to "Commands, e.g. [{\"op\":\"draw_line\",\"x0\":0,...}, {\"op\":\"set_pixel\",...}].",
                "items" to linkedMapOf("type" to "object", "required" to listOf("op"), "properties" to linkedMapOf("op" to string("Operation name."))))),
            listOf("commands"))
        // -- Pixel-art craft ------------------------------------------------------------------
        tool("draw_ascii", "Draw ASCII sprite", "pixelart", Access.WRITE,
            "Draws pixel art from text: one string per row and a legend mapping single characters to colors ('.' = transparent). The best way to author sprites: write the whole sprite in one call, read it back with read_region format=ascii, edit, redraw.",
            props(linkedMapOf("rows" to linkedMapOf("type" to "array", "items" to linkedMapOf("type" to "string"), "description" to "Rows top to bottom, one character per pixel."),
                "legend" to linkedMapOf("type" to "object", "description" to "Character → #RRGGBB[AA], e.g. {\"o\":\"#1B1B2F\",\"s\":\"#F6CDB0\"}."),
                "x" to X, "y" to Y, "transparent" to string("Character meaning 'skip' (default '.')."), "erase_transparent" to boolean("Clear pixels under transparent characters."),
                "flip_x" to boolean("Mirror horizontally (reuse a side view for the other side).")), target()),
            listOf("rows", "legend"))
        tool("outline", "Outline", "pixelart", Access.WRITE,
            "Adds a 1-pixel outline around a layer's pixels: a fixed color, 'selective' (each edge pixel is a darker, cooler shade of what it borders) or mode 'selout' " +
                "(selective + lighter on the lit side — the soft pastel anime look). inside=true draws on the shape's edge pixels instead of growing it.",
            props(linkedMapOf("color" to linkedMapOf("type" to "string", "description" to "Hex color, 'selective' (default) or 'selout'."), "corners" to boolean("Include diagonal corners."),
                "mode" to string("Outline style.", listOf("selective", "selout", "solid")), "darkness" to linkedMapOf<String, Any?>("type" to "number", "description" to "0.05-0.95: how dark generated outlines are (default 0.58; pastel ≈ 0.4)."),
                "inside" to boolean("Draw on the edge pixels (keeps the size)."), "light" to linkedMapOf<String, Any?>("type" to listOf("string", "array"), "description" to "Light direction for selout (default top-left).")), target()))
        tool("color_ramp", "Hue-shifted ramp", "pixelart", Access.WRITE,
            "Builds a shading ramp from a base color: shadows shift toward cool purple, highlights toward warm yellow (perceptual OKLCH). Adds the ramp to the swatches. Use 4-6 steps per material.",
            linkedMapOf("base" to color("Base (mid) color."), "steps" to integer("2-16 (default 5).", 2, 16),
                "hue_shift" to linkedMapOf<String, Any?>("type" to "number", "description" to "Degrees of hue shift at the ends (default 18)."),
                "contrast" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-1 lightness range (default 0.75)."), "add_to_palette" to boolean("Default true."),
                "name" to string("Also store it as a named material ramp (skin, hair, cloth…) for shade / auto_shade.")),
            listOf("base"))
        tool("quantize", "Reduce colors", "pixelart", Access.WRITE,
            "Reduces colors of a cel/frame/layer/everything to a limited palette (median cut N colors, the swatches, or a library palette) with optional ordered dithering — turns renders and imports into crisp pixel art. One shared palette across the scope keeps animations consistent.",
            props(linkedMapOf("scope" to string("cel | frame | layer | all.", listOf("cel", "frame", "layer", "all")), "colors" to integer("Target color count (2-256).", 2, 256),
                "palette" to linkedMapOf<String, Any?>("type" to listOf("string", "array"), "description" to "Library name, 'swatches', or a color list."),
                "dither" to string("Ordered dither.", listOf("none", "bayer2", "bayer4", "bayer8")), "dither_strength" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-255 (default 28)."),
                "add_to_palette" to boolean("Add the resulting colors to the swatches.")), target()))
        tool("dither_fill", "Dither fill", "pixelart", Access.WRITE, "Fills a region with an ordered-dither blend of two colors; level 0 = color, 16 = color2 (gradients, glass, shading transitions).",
            props(region(), linkedMapOf("color" to color(), "color2" to color("Second color."), "level" to integer("0-16 density.", 0, 16), "only_opaque" to boolean("Only over existing pixels.")), target()),
            listOf("color", "color2"))
        addGameTools()
        addCharTools()

        tool("image_import", "Import image", "project", Access.WRITE,
            "Imports a PNG as pixel art at x,y. grid auto recovers the true pixel grid of upscaled art (also non-integer scales like 3.35×); transparent auto drops a solid background; " +
                "max_colors limits the palette; ramps true turns its colors into material ramps (hue families, dark → light) for shade and auto_shade. " +
                "reference true puts it on a locked, faded 'Reference' layer to study or trace. layer: index/name; an unknown name creates that layer.",
            props(linkedMapOf("data_base64" to string("PNG file as base64."), "x" to X, "y" to Y,
                "grid" to linkedMapOf<String, Any?>("type" to listOf("string", "number"), "description" to "auto (default), none, or the upscale factor."),
                "transparent" to string("auto (default) or none.", listOf("auto", "none")), "max_colors" to integer("Reduce to N colors.", 2, 256),
                "ramps" to boolean("Build material ramps from its colors (default false)."), "reference" to boolean("Import as a locked, faded reference layer.")), target()),
            listOf("data_base64"))

        // -- 3D pixel art (voxels) ---------------------------------------------------------
        val model = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Model index or name (default: the first model).")
        val vec3 = { d: String -> linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf("type" to "number"), "minItems" to 3, "maxItems" to 3, "description" to d) }
        val boneRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Bone name or index the voxels follow when animated.")
        val materialRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "PBR material name or index (default material 0).")
        val look = linkedMapOf(
            "yaw" to linkedMapOf<String, Any?>("type" to "number", "description" to "Turntable angle in degrees; 0 = front faces the camera, 90 = character faces screen-right."),
            "pitch" to linkedMapOf<String, Any?>("type" to "number", "description" to "Camera elevation in degrees (default 30; 0 = side view)."),
            "width" to integer("Output width in pixels (8-2048, default 256).", 8, 2048),
            "height" to integer("Output height (default = width).", 8, 2048),
            "scale" to linkedMapOf<String, Any?>("type" to "number", "description" to "Pixels per voxel. Omit to auto-fit the model."),
            "focus" to vec3("Model point to center in the image (close-ups, with scale)."),
            "animation" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "3D animation name/index."),
            "frame" to linkedMapOf<String, Any?>("type" to "number", "description" to "Animation time in frames (fractions allowed)."),
            "pass" to string("Render pass / G-buffer map.", listOf("lit", "albedo", "normal", "depth", "occlusion", "roughness", "metallic", "emission")),
            "smoothness" to linkedMapOf<String, Any?>("type" to "number", "description" to "0 = crisp voxel faces (3D pixel look), 1 = smooth sprite shading (default 0.55)."),
            "toon_steps" to integer("0 = physically based; 2-16 = toon light bands.", 0, 16),
            "outline" to linkedMapOf<String, Any?>("type" to listOf("boolean", "string"), "description" to "true/'selective' = pixel-art selective outline, or a hex outline color."),
            "inner_lines" to boolean("Darken depth edges inside the silhouette (separates limbs)."),
            "supersample" to integer("1-4. 2 gives clean edges at small sprite sizes.", 1, 4),
            "exposure" to linkedMapOf<String, Any?>("type" to "number", "description" to "Lighting exposure (default 1)."),
            "background" to color("Optional background color."),
            "background_gradient" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "minItems" to 2, "maxItems" to 2, "description" to "[top, bottom] colors with a soft vignette (diorama look)."),
            "shadows" to linkedMapOf<String, Any?>("type" to "number", "description" to "Cast shadows from the key light, 0-1."),
            "ground_shadow" to linkedMapOf<String, Any?>("type" to "number", "description" to "Shadow on the ground under the model for transparent sprites (alpha 0-1)."),
            "bloom" to linkedMapOf<String, Any?>("type" to "number", "description" to "Glow around emissive voxels (0-4; 0.5-0.8 typical)."),
            "bloom_radius" to linkedMapOf<String, Any?>("type" to "number", "description" to "Glow radius in output pixels (0 = auto)."),
            "lights" to linkedMapOf<String, Any?>("type" to "array", "description" to "Point lights: [{position:[x,y,z], color, intensity, radius}] in model space (fires, windows, magic).",
                "items" to linkedMapOf("type" to "object")),
            "key_light" to vec3("Key light direction (view space, toward the light)."),
            "key_intensity" to linkedMapOf<String, Any?>("type" to "number", "description" to "Key light intensity (default 3.2)."),
            "ambient" to linkedMapOf<String, Any?>("type" to "number", "description" to "Ambient multiplier (default 1; lower for night scenes)."),
            "pixel_palette" to linkedMapOf<String, Any?>("type" to listOf("string", "integer"), "description" to "Pixel-art post-process: quantize the render to a library palette (e.g. 'endesga-32'), 'swatches', or N colors. Shared across all sprites of a sheet."),
            "dither" to string("Ordered dither for pixel_palette.", listOf("none", "bayer2", "bayer4", "bayer8")),
        )
        val sequence = linkedMapOf<String, Map<String, Any?>>(
            "models" to linkedMapOf("type" to "array", "items" to linkedMapOf("type" to "string"), "description" to "Frame-by-frame 3D animation: each model is one frame (VFX, morphs). One shared camera keeps it stable."),
            "name" to string("Sequence name (tags become <name>_<direction>)."),
            "frame_ms" to integer("Milliseconds per frame for sequences.", 1, 60000),
        )
        tool("voxel_model_create", "Create 3D model", "3d", Access.WRITE,
            "Creates a sparse voxel model (3D pixels), up to 512x512x512. y is up, the model's front faces +z, origin = point between the feet (default bottom center).",
            linkedMapOf("name" to string("Model name."), "width" to integer("X size.", 1, 512), "height" to integer("Y size (up).", 1, 512), "depth" to integer("Z size.", 1, 512), "origin" to vec3("Ground pivot [x, y, z].")),
            listOf("name"))
        tool("voxel_model_clone", "Clone 3D model", "3d", Access.WRITE,
            "Duplicates a model (voxels, rig, animations). Build a static base once, clone it per frame and add the moving voxels: frame-by-frame VFX (fire, smoke, magic). empty=true clones only the rig/size.",
            linkedMapOf("model" to model, "name" to string("New model name."), "empty" to boolean("Copy without voxels.")), listOf("name"))
        tool("voxel_model_delete", "Delete 3D model", "3d", Access.DESTRUCTIVE, "Deletes a voxel model. Requires the user's approval.", linkedMapOf("model" to model), listOf("model"))
        tool("voxel_model_set", "Rename / re-pivot model", "3d", Access.WRITE, "Renames a model or moves its origin pivot.", linkedMapOf("model" to model, "name" to string("New name."), "origin" to vec3("New origin.")))
        tool("voxel_inspect", "Inspect 3D project", "3d", Access.READ, "Models (size, voxel count, bounds, skeleton, animations), PBR materials and textures.", linkedMapOf("model" to model))
        tool("voxel_set", "Place voxel", "3d", Access.WRITE, "Places (or with 'transparent' removes) one voxel.",
            linkedMapOf("model" to model, "x" to X, "y" to Y, "z" to integer("Z coordinate."), "color" to color(), "material" to materialRef, "bone" to boneRef), listOf("x", "y", "z", "color"))
        tool("voxel_set_many", "Place voxels", "3d", Access.WRITE, "Places up to 1,000,000 explicit voxels in one undoable step: [[x, y, z, color], ...] or objects with material/bone.",
            linkedMapOf("model" to model, "voxels" to linkedMapOf("type" to "array", "items" to linkedMapOf("type" to listOf("array", "object")), "description" to "Voxels."), "color" to color("Default color."), "material" to materialRef, "bone" to boneRef),
            listOf("voxels"))
        tool("voxel_shape", "Sculpt shape", "3d", Access.WRITE,
            "Sculpts or paints with a primitive. shape: box(min,max) | ellipsoid(center, radii or radius) | capsule(a, b, radius, radius_b — tapered limbs) | cone | cylinder(a, b, radius). mode: add | paint (recolor existing surface only) | erase. Extras: color_to + gradient_axis (gradients), jitter 0-1 (tonal noise for texture), mirror_x (symmetric, swaps _l/_r bones), material, bone.",
            linkedMapOf("model" to model, "shape" to string("Primitive.", listOf("box", "ellipsoid", "sphere", "capsule", "cone", "cylinder")), "mode" to string("Operation.", listOf("add", "paint", "erase")),
                "min" to vec3("Box min corner."), "max" to vec3("Box max corner (inclusive)."), "center" to vec3("Ellipsoid center."), "radii" to vec3("Ellipsoid radii."),
                "radius" to linkedMapOf<String, Any?>("type" to "number", "description" to "Radius (capsule start / sphere / cylinder)."), "radius_b" to linkedMapOf<String, Any?>("type" to "number", "description" to "Capsule end radius (taper)."),
                "a" to vec3("Capsule/cylinder start."), "b" to vec3("Capsule/cylinder end."), "color" to color(), "color_to" to color("Gradient end color."),
                "gradient_axis" to string("Gradient axis.", listOf("x", "y", "z")), "jitter" to linkedMapOf<String, Any?>("type" to "number", "description" to "Tonal noise 0-1."), "seed" to integer("Noise seed."),
                "mirror_x" to boolean("Also apply mirrored across the origin's X."), "material" to materialRef, "bone" to boneRef),
            listOf("shape"))
        tool("voxel_mirror", "Mirror model half", "3d", Access.WRITE, "Copies one half onto the other across the origin's X plane (bones _l/_r swapped).",
            linkedMapOf("model" to model, "from" to string("Source half.", listOf("+", "-"))))
        tool("material_set", "Create/edit PBR material", "3d", Access.WRITE,
            "Creates or updates a PBR material: roughness, metallic, emission, uv_scale (texels per voxel), normal_strength, maps {albedo, normal, roughness, metallic, occlusion, height, emission: texture name}.",
            linkedMapOf("name" to string("Material name."), "roughness" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-1."), "metallic" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-1."),
                "emission" to linkedMapOf<String, Any?>("type" to "number", "description" to "Glow strength (0 = none)."), "uv_scale" to linkedMapOf<String, Any?>("type" to "number", "description" to "Texels per voxel."),
                "normal_strength" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-4."), "maps" to linkedMapOf<String, Any?>("type" to "object", "description" to "Slot → texture name.")),
            listOf("name"))
        tool("texture_import", "Import texture", "3d", Access.WRITE, "Imports a PNG (base64) as a texture map usable in any PBR slot.",
            linkedMapOf("name" to string("Texture name."), "data_base64" to string("PNG as base64.")), listOf("name", "data_base64"))
        tool("texture_generate", "Generate PBR textures", "3d", Access.WRITE,
            "Generates a tileable PBR set (albedo, normal, roughness, occlusion, height) and a material of the same name. kinds: noise, brushed_metal, hammered_metal, leather, fabric, scales, stone, wood, chainmail.",
            linkedMapOf("name" to string("Set/material name."), "kind" to string("Surface type.", listOf("noise", "brushed_metal", "hammered_metal", "leather", "fabric", "scales", "stone", "wood", "chainmail")),
                "size" to integer("Texture size (8-1024).", 8, 1024), "cells" to integer("Feature count across the tile.", 1, 64), "seed" to integer("Seed."),
                "roughness" to linkedMapOf<String, Any?>("type" to "number", "description" to "Base roughness."), "metallic" to linkedMapOf<String, Any?>("type" to "number", "description" to "Metallic."),
                "contrast" to linkedMapOf<String, Any?>("type" to "number", "description" to "Albedo variation 0-1."), "uv_scale" to linkedMapOf<String, Any?>("type" to "number", "description" to "Texels per voxel.")),
            listOf("name", "kind"))
        tool("rig_bone_add", "Add bone", "3d", Access.WRITE,
            "Adds a bone (pivot head → tail). Humanoid names enable the motion generators: hips, spine, chest, neck, head, upper_arm_l/r, lower_arm_l/r, hand_l/r, upper_leg_l/r, lower_leg_l/r, foot_l/r, cape. Character's left = +x.",
            linkedMapOf("model" to model, "name" to string("Bone name."), "parent" to boneRef, "head" to vec3("Pivot (joint) position."), "tail" to vec3("End position.")), listOf("name", "head"))
        tool("rig_clear", "Clear rig", "3d", Access.DESTRUCTIVE, "Removes all bones, animations and bindings of a model. Requires approval.", linkedMapOf("model" to model))
        tool("rig_bind", "Bind voxels to bones", "3d", Access.WRITE, "Binds voxels to their nearest bone (only_unbound keeps explicit bindings from voxel_shape's bone).",
            linkedMapOf("model" to model, "only_unbound" to boolean("Keep explicit bindings.")))
        tool("anim3d_create", "Create 3D animation", "3d", Access.WRITE, "Creates an empty keyframe animation.",
            linkedMapOf("model" to model, "name" to string("Animation name."), "frames" to integer("Length in frames.", 1, 10000), "frame_ms" to integer("Milliseconds per frame.", 1, 60000), "loop" to boolean("Loops.")), listOf("name"))
        tool("anim3d_key", "Set bone key", "3d", Access.WRITE, "Sets a bone's rotation (Euler degrees around its head) and position offset at a frame; frames between keys are interpolated.",
            linkedMapOf("model" to model, "animation" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Animation."), "bone" to boneRef,
                "frame" to linkedMapOf<String, Any?>("type" to "number", "description" to "Frame."), "rotation" to vec3("Euler degrees [x, y, z]."), "position" to vec3("Offset [x, y, z].")),
            listOf("animation", "bone", "frame"))
        tool("anim3d_generate", "Generate motion cycle", "3d", Access.WRITE, "Generates a humanoid walk, run or idle cycle (foot planting, counter-swing, cape lag) from standard bone names.",
            linkedMapOf("model" to model, "kind" to string("Cycle.", listOf("walk", "run", "idle")), "name" to string("Animation name (default = kind)."), "frames" to integer("Frames (2-240).", 2, 240),
                "frame_ms" to integer("ms per frame.", 1, 60000), "stride" to linkedMapOf<String, Any?>("type" to "number", "description" to "Leg swing degrees."), "knee" to linkedMapOf<String, Any?>("type" to "number", "description" to "Knee bend degrees."),
                "arm_swing" to linkedMapOf<String, Any?>("type" to "number", "description" to "Arm swing degrees."), "bob" to linkedMapOf<String, Any?>("type" to "number", "description" to "Extra vertical bob (voxels)."),
                "lean" to linkedMapOf<String, Any?>("type" to "number", "description" to "Forward lean degrees.")),
            listOf("kind"))
        tool("voxel_render", "Render 3D model", "3d", Access.READ, "Renders a model to PNG from any angle, animation frame and pass (lit PBR or a G-buffer map). Use it to look at your 3D work.",
            props(linkedMapOf("model" to model), look), op = "voxel_render", output = Output.PNG_IMAGE)
        tool("voxel_render_sheet", "Render sprite sheet", "3d", Access.READ,
            "Renders directions × animation frames into one sprite sheet (rows = directions S, SE, E, NE, N, NW, W, SW) for any pass — e.g. normal-map sheets for 2D dynamic lighting.",
            props(linkedMapOf("model" to model, "directions" to integer("1, 2, 4, 8 or 16.", 1, 16)), sequence, look), op = "voxel_render_sheet", output = Output.PNG_IMAGE)
        tool("voxel_render_to_frames", "Bake 3D to 2D frames", "3d", Access.WRITE,
            "Bakes directions × animation frames into the 2D project as editable pixel frames on one layer, tagged per direction (e.g. walk_S, walk_SE). All sprites share one scale and ground line.",
            props(linkedMapOf("model" to model, "directions" to integer("1, 2, 4, 8 or 16.", 1, 16), "layer" to string("Target layer name (default '3D Render')."), "resize_canvas" to boolean("Resize the 2D canvas to the render size (default true).")), sequence, look))
        tool("voxel_pick", "Pick voxel at pixel", "3d", Access.READ, "Which voxel and face is under image pixel (px, py) for the given camera — used by 3D viewports for click editing.",
            props(linkedMapOf("model" to model, "px" to integer("Pixel x."), "py" to integer("Pixel y.")), look), listOf("px", "py"))
    }

    private val byName = tools.associateBy { it.name }

    operator fun get(name: String): ToolDef? = byName[name]

    /** Access level of a raw engine operation (REST passthrough), by matching tool. */
    fun accessForOp(op: String): Access = tools.firstOrNull { it.op == op }?.access ?: when (op) {
        "project_load" -> Access.DESTRUCTIVE
        "status", "render_frame", "export" -> Access.READ
        else -> Access.WRITE
    }
}

// ═══ GameTools ═══════════════════════════════════════════════════════════════════════════════════

// Game-production tools, kept apart from the main catalogue (and fully typed) so the Kotlin
// compiler does not have to infer one enormous builder lambda.

internal fun MutableList<ToolDef>.addGameTools() {
    fun tool(
        name: String, title: String, category: String, access: Access, description: String,
        properties: Map<String, Map<String, Any?>> = emptyMap(), required: List<String> = emptyList(),
        op: String = name, output: Output = Output.JSON,
    ) {
        add(ToolDef(name, title, description, op, properties, required, access, output, category))
    }

    val num: (String) -> Map<String, Any?> = { d -> linkedMapOf<String, Any?>("type" to "number", "description" to d) }
    val tilesetRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Tileset name or index (optional when there is only one).")
    val tilemapRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Tilemap name or index (optional when there is only one).")
    val pack: LinkedHashMap<String, Map<String, Any?>> = linkedMapOf(
        "tag" to string("Only this tag's frames (default: all frames)."), "layer" to LAYER, "scale" to integer("Upscale 1-64.", 1, 64),
        "trim" to boolean("Trim transparent borders (default true)."), "padding" to integer("Gap between sprites (default 1).", 0, 64),
        "extrude" to integer("Edge pixels repeated around each sprite against bleeding (default 0).", 0, 16), "power_of_two" to boolean("Power-of-two atlas size."),
    )
    tool("slice_set", "Set slice / hitbox", "gamedev", Access.WRITE,
        "Creates or updates a named per-frame rectangle (Aseprite-compatible slice): hitbox, hurtbox, pivot, weapon attach point. A key without 'frame' covers the whole animation; later keys override from their frame on. Exported in the atlas meta.slices.",
        linkedMapOf<String, Map<String, Any?>>("name" to string("Slice name, e.g. hitbox, hurtbox, pivot."), "frame" to FRAME, "x" to X, "y" to Y, "width" to integer("Width.", 1), "height" to integer("Height.", 1),
        "pivot" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "integer"), "minItems" to 2, "maxItems" to 2, "description" to "Pivot [x, y] relative to the slice."),
        "color" to color("Editor color of the slice.")),
        listOf("name", "x", "y", "width", "height"))
    tool("slice_delete", "Delete slice", "gamedev", Access.WRITE, "Deletes a slice, or only its key at 'frame'.", linkedMapOf<String, Map<String, Any?>>("name" to string("Slice name."), "frame" to FRAME), listOf("name"))
    tool("palette_swap", "Palette swap", "gamedev", Access.WRITE,
        "Replaces or remaps colors (RGB match, alpha kept) across a cel/layer/frame/everything: one from/to pair, many, or a map — team colors, enemy variants, day/night palettes, damage flashes.",
        props(linkedMapOf<String, Map<String, Any?>>("map" to linkedMapOf<String, Any?>("type" to "object", "description" to "{\"#from\": \"#to\", …}"),
        "from" to linkedMapOf<String, Any?>("type" to listOf("string", "array"), "items" to color(), "description" to "Source color, or colors (with 'to')."),
        "to" to linkedMapOf<String, Any?>("type" to listOf("string", "array"), "items" to color(), "description" to "Target color, or colors (same length as 'from')."),
        "scope" to string("cel | layer | frame | all (default cel).", listOf("cel", "layer", "frame", "all"))), target()))
    tool("tileset_create", "Create tileset", "gamedev", Access.WRITE, "Creates an empty tileset (fill it with tileset_add_tiles).",
        linkedMapOf<String, Map<String, Any?>>("name" to string("Tileset name."), "tile_size" to integer("Square tile size (default 16).", 1, 256), "tile_width" to integer("Tile width.", 1, 256), "tile_height" to integer("Tile height.", 1, 256)), listOf("name"))
    tool("tileset_add_tiles", "Cut tiles from canvas", "gamedev", Access.WRITE,
        "Cuts a grid of tiles from a rendered frame (or one layer) into a tileset; identical and empty tiles are skipped. Returns the tile index of each cut cell.",
        props(linkedMapOf<String, Map<String, Any?>>("tileset" to tilesetRef, "x" to X, "y" to Y, "columns" to integer("Tiles across.", 1, 256), "rows" to integer("Tiles down.", 1, 256),
        "dedupe" to boolean("Reuse identical tiles (default true)."), "skip_empty" to boolean("Skip empty cells (default true).")), target()))
    tool("tileset_generate_blob", "Generate 47-tile autotile set", "gamedev", Access.WRITE,
        "Synthesizes a complete 47-tile blob autotile terrain set (Godot terrains / Tiled Wang / LDtk / GameMaker layout) from one seamless fill: a canvas region (source_x/source_y) or a procedural fill color with accent clusters. Lit top edges, shaded sides, selective outline, rounded corners, optional cliff lip.",
        props(linkedMapOf<String, Map<String, Any?>>("name" to string("Tileset name."), "tile_size" to integer("Tile size (default 16).", 4, 256),
        "source_x" to integer("Take the fill tile from the canvas at this x."), "source_y" to integer("…and y."),
        "fill" to color("Procedural base color."), "accents" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to "Accent colors (default: lighter/darker ramp steps)."),
        "density" to num("Accent density 0-1 (default 0.12)."), "seed" to integer("Pattern seed."),
        "light" to color("Lit top edge."), "dark" to color("Shaded edge."), "outline" to color("Outline (default: selective darker hue)."),
        "inset" to integer("Transparent margin on open sides (default 1).", 0, 64), "edge" to integer("Edge band width (default 1).", 0, 64),
        "lip" to integer("Cliff lip height under open south edges (default 0).", 0, 128), "lip_color" to color("Cliff lip color."), "rounded" to boolean("Round outer corners (default true).")),
        linkedMapOf<String, Map<String, Any?>>("frame" to FRAME, "layer" to LAYER)),
        listOf("name"))
    tool("tileset_delete", "Delete tileset", "gamedev", Access.DESTRUCTIVE, "Deletes an unused tileset.", linkedMapOf<String, Map<String, Any?>>("tileset" to tilesetRef))
    tool("tilemap_create", "Create tilemap", "gamedev", Access.WRITE, "Creates an empty tilemap (grid of tile cells) bound to a tileset.",
        linkedMapOf<String, Map<String, Any?>>("name" to string("Tilemap name."), "tileset" to tilesetRef, "columns" to integer("Width in tiles.", 1, 4096), "rows" to integer("Height in tiles.", 1, 4096)), listOf("name", "columns", "rows"))
    tool("tilemap_delete", "Delete tilemap", "gamedev", Access.DESTRUCTIVE, "Deletes a tilemap.", linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef))
    tool("tilemap_set", "Place tiles", "gamedev", Access.WRITE,
        "Places explicit tiles: cells [[x, y, tile], …] (tile null = empty), rows of space-separated tile indices ('.' = empty) starting at x/y, or a single x/y/tile.",
        linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "cells" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "[[x, y, tile], …]."),
        "rows" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "string"), "description" to "e.g. [\"0 1 1 2\", \". 3 3 .\"]."),
        "x" to X, "y" to Y, "tile" to integer("Tile index.", 0)))
    tool("tilemap_terrain", "Paint terrain (autotile)", "gamedev", Access.WRITE,
        "Paints terrain on a blob-tileset map and re-autotiles every cell (edges, inner/outer corners chosen automatically). Give ASCII rows ('#' = terrain, '.' = empty), a rectangle x/y/width/height, or cells [[x, y], …]; mode remove erases.",
        linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "rows" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "string"), "description" to "ASCII terrain rows."),
        "replace" to boolean("With rows: '.' clears terrain (default true)."), "x" to X, "y" to Y, "width" to integer("Rect width.", 1), "height" to integer("Rect height.", 1),
        "cells" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "[[x, y], …]."), "mode" to string("add (default) or remove.", listOf("add", "remove"))))
    tool("tilemap_bake", "Bake tilemap to layer", "gamedev", Access.WRITE, "Draws a tilemap into a layer cel at x/y (so it composites, lights and exports with the scene).",
        props(linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "x" to X, "y" to Y, "replace" to boolean("Also clear pixels under empty cells.")), target()))
    tool("tilemap_render", "Look at tilemap", "gamedev", Access.READ, "Renders a tilemap to PNG.", linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "scale" to integer("Upscale 1-64.", 1, 64)), output = Output.PNG_IMAGE)
    tool("render_lit", "Preview 2D lighting", "gamedev", Access.READ,
        "Renders a frame with normal-mapped 2D point lights (torches, campfires, spells) — what Godot/Unity 2D lights do with the exported normal map. Normals are generated from the art or taken from a hand-painted normal layer; unlit_layers (fire, glowing eyes, UI) stay full-bright. bands ≥ 2 quantizes light into crisp pixel-art steps.",
        linkedMapOf<String, Map<String, Any?>>("frame" to FRAME,
        "lights" to linkedMapOf<String, Any?>("type" to "array", "description" to "Point lights.", "items" to linkedMapOf<String, Any?>("type" to "object", "properties" to linkedMapOf<String, Map<String, Any?>>(
            "x" to num("X."), "y" to num("Y."), "z" to num("Height above the sprites (default 24)."), "color" to color("Light color."), "intensity" to num("Default 1.5."), "radius" to num("Reach in px (default 64).")))),
        "ambient" to color("Ambient light color (default dim blue night)."), "bands" to integer("Light steps (0 = smooth).", 0, 32),
        "normal_strength" to num("Normal map influence 0-8 (default 1)."), "normal_layer" to LAYER, "unlit_layers" to linkedMapOf<String, Any?>("type" to "array", "items" to LAYER, "description" to "Layers drawn full-bright on top."),
        "strength" to num("Generated normal bump strength."), "bevel" to num("Generated normal bevel px."), "background" to color("Opaque background."), "scale" to integer("Upscale 1-64.", 1, 64)),
        output = Output.PNG_IMAGE)

}

// ═══ CharTools ═══════════════════════════════════════════════════════════════════════════════════

// Character-art finishing tools (fully typed, separate from the main catalogue builder).

private typealias Schema = Map<String, Any?>

internal fun MutableList<ToolDef>.addCharTools() {
    fun tool(name: String, title: String, access: Access, description: String, properties: Map<String, Schema>, required: List<String> = emptyList()) {
        add(ToolDef(name, title, description, name, properties, required, access, Output.JSON, "pixelart"))
    }
    val num: (String) -> Schema = { d -> linkedMapOf<String, Any?>("type" to "number", "description" to d) }
    val colors: (String) -> Schema = { d -> linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to d) }
    val light: Schema = linkedMapOf<String, Any?>(
        "type" to listOf("string", "array"),
        "description" to "Light direction: top-left (default), top-right, top, left, right, bottom, front, or [x, y] (image space, y down).",
    )
    val scope: Schema = string("cel (default) | layer (all frames) | frame (all layers) | all.", listOf("cel", "layer", "frame", "all"))

    val pts: Schema = linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf("type" to "number"), "minItems" to 2, "maxItems" to 2), "description" to "[[x, y], …]")
    val layerArg: Schema = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer (default active).")
    val frameArg: Schema = integer("Frame (default active).", min = 0)
    val lightArg: Schema = linkedMapOf<String, Any?>("type" to listOf("string", "array"),
        "description" to "Direction toward the light: top-left (default), top-right, top, left, right, front, bottom or [x, y, z] (y down, z toward viewer).")
    val bandArgs: Map<String, Schema> = linkedMapOf(
        "shadow_steps" to integer("Ramp steps below the base tone for shadows (default 2).", 1, 4),
        "light_steps" to integer("Ramp steps above the base tone for lights (default 1).", 0, 3),
        "reflected_light" to boolean("Bounce light on the shadow-side rim (default true)."),
        "bias" to num("Shift the terminator: positive = more light (-0.8..0.8)."),
    )
    add(ToolDef("art_review", "Critique the pixel art", "The objective critic — call it after every major step and fix what it reports. Measures the light/dark value span " +
        "(the #1 reason art reads muddy), figure/background separation, banding pairs (Pixel Fixer metric), pillow shading, 1-px stripes, noise and silhouette jaggies, " +
        "returns a 0-100 score, issues with the tool that fixes each, and a greyscale value-map PNG with problems marked (red banding, yellow stripes, cyan pillow, magenta jaggies).",
        "art_review", linkedMapOf("frame" to frameArg, "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Review one layer only (default: the whole frame)."),
            "scale" to integer("PNG upscale 1-64 (default auto).", 1, 64)), emptyList(), Access.READ, Output.PNG_IMAGE, "pixelart"))
    tool("shade_form", "Form lighting", Access.WRITE,
        "Lights flat ramp-coloured shapes like a 3D form: surface normals of a sphere, a tapered capsule (limbs, boots, sleeves) or an 'inflated' height field of any silhouette, " +
            "Lambert-quantised into ramp steps from ONE light direction, with reflected light on the shadow side. Replaces pillow shading. Block each material in its ramp's base tone first.",
        linkedMapOf<String, Schema>("form" to string("inflate (default, any shape), sphere or capsule.", listOf("inflate", "sphere", "capsule")),
            "cx" to num("Sphere centre x."), "cy" to num("Sphere centre y."), "rx" to num("Sphere radius x."), "ry" to num("Sphere radius y (default rx)."),
            "points" to pts, "radius" to linkedMapOf<String, Any?>("type" to listOf("number", "array"), "description" to "Capsule radius, or radii per point (taper)."),
            "light" to lightArg, "ramp" to string("Only pixels of this ramp."), "only_color" to color("Only pixels of this colour."),
            "x" to integer("Region left."), "y" to integer("Region top."), "width" to integer("Region width.", 1), "height" to integer("Region height.", 1),
            "depth" to integer("Extra ramp steps for the whole form (-1 = farther/behind).", -3, 3), "layer" to layerArg, "frame" to frameArg) + bandArgs)
    tool("draw_path", "Smooth curve / shape", Access.WRITE,
        "Draws smooth curves the pixel-art way: Catmull-Rom through points (or cubic Bézier, or polyline). 1-px paths are pixel-perfect with monotone run lengths (no jaggies). " +
            "width > 1 or [w0, …, wn] makes a tapered stroke (hair locks, scarf tails, capes, blades); closed+filled makes a curved shape (clothes, hair masses, leaves).",
        linkedMapOf<String, Schema>("points" to pts, "color" to color(), "curve" to string("spline (default), bezier (p0,c1,c2,p1,…) or polyline.", listOf("spline", "bezier", "polyline")),
            "width" to linkedMapOf<String, Any?>("type" to listOf("number", "array"), "description" to "Stroke width, or widths along the path for a taper."),
            "closed" to boolean("Close the path."), "filled" to boolean("Fill the closed shape."), "edge" to boolean("Filled shapes: also draw the clean 1-px edge (default true)."),
            "pixel_perfect" to boolean("Default true."), "smooth" to boolean("Monotone runs + flattened bumps (default true)."), "layer" to layerArg, "frame" to frameArg),
        listOf("points", "color"))
    tool("antialias", "Selective anti-aliasing", Access.WRITE,
        "Softens stair-steps of darker lines against lighter fills INSIDE the sprite with an in-between ramp tone: longer runs get longer AA, 45° steps and transparent edges are never touched.",
        linkedMapOf<String, Schema>("color" to color("Only lines of this colour."), "amount" to num("Fraction of each run that gets AA, 0-1 (default 0.5)."),
            "min_contrast" to num("Minimum lightness difference line/fill (default 18)."), "allow_new_colors" to boolean("Mix a new colour when no ramp tone fits (default false)."),
            "layer" to layerArg, "frame" to frameArg))
    tool("fix_banding", "Fix banding", Access.WRITE,
        "Automatic banding correction (Pixel Fixer): shortens one end of each same-length parallel run pair with the in-between ramp tone (or the neighbour's colour) while the banding error falls.",
        linkedMapOf<String, Schema>("mode" to string("average (default) or copy.", listOf("average", "copy")), "passes" to integer("1-32 (default 6).", 1, 32), "layer" to layerArg, "frame" to frameArg))
    tool("concept_import", "Import AI concept art", Access.WRITE,
        "Turns an image from an AI image model (PNG or baseline JPEG, any size) into true, editable pixel art: finds the pseudo-pixel grid by Fourier analysis " +
            "(also noisy, blurred, JPEG-compressed upscales), takes each cell's dominant colour, keys out the flat background, removes compression speckle, " +
            "reduces the palette, builds material ramps, crops to the sprite and fits the canvas. Returns the detected cell size, confidence and sprite size.",
        linkedMapOf<String, Schema>("data_base64" to string("The image file (PNG or JPEG) as base64."), "cell" to num("Force the source pixel size (default: detect)."),
            "colors" to integer("Palette size after reduction (default 32).", 2, 256), "background" to string("auto (key out the border colour) or keep.", listOf("auto", "keep")),
            "background_tolerance" to num("Perceptual distance keyed as background (default 0.1)."), "clean" to num("Speckle clean-up strength 0-0.3 (default 0.07, 0 = off)."),
            "ramps" to boolean("Build material ramps from the colours (default true)."), "fit_canvas" to boolean("Resize the canvas to the sprite (default true)."),
            "margin" to integer("Transparent margin around the sprite (default 2).", 0, 64), "sample" to num("Fraction of each cell sampled (default 0.5)."),
            "layer" to layerArg, "x" to integer("Left."), "y" to integer("Top."), "frame" to frameArg),
        listOf("data_base64"))
    tool("draw_figure", "Posed body mannequin", Access.WRITE,
        "Draws a proportioned, posed and form-lit body (head, neck, torso, arms, hands, legs, feet) with a skin ramp — the base to dress with draw_path shapes. " +
            "Returns the joints (head, neck, shoulders, elbows, wrists, waist, pelvis, hips, knees, ankles, toes) to place clothes. layers 5 puts far arm, far leg, body, near leg and near arm on separate layers so sleeves and skirts stack correctly.",
        linkedMapOf<String, Schema>("x" to integer("Centre x between the feet."), "y" to integer("Ground y (feet)."), "height" to num("Figure height in px (default 80% of the canvas)."),
            "heads" to num("Heads tall: 2-3 chibi, 5 stylised, 6.5-7.5 realistic (default 7)."), "pose" to string("Pose.", listOf("contrapposto", "walk", "stand")),
            "facing" to string("Facing direction (3/4 view).", listOf("right", "left")), "build" to num("Width factor 0.6-1.6."), "hips" to num("Hip width factor 0.7-1.5."),
            "ramp" to string("Skin ramp name (default skin)."), "base" to integer("Base ramp index (default middle-low)."), "shade" to boolean("Form-light it (default true)."),
            "light" to lightArg, "layers" to linkedMapOf<String, Any?>("type" to "integer", "enum" to listOf(3, 5), "description" to "3 (far/body/near) or 5 (each limb group)."),
            "name" to string("Layer name prefix (default Body)."), "frame" to frameArg) + bandArgs,
        listOf("x", "y"))
    tool("ramp_set", "Material ramp", Access.WRITE,
        "Defines a named material ramp, dark → light (skin, hair, cloth, metal, outline). Ramps are the palette discipline of hand-made pixel art: shade steps along them and auto_shade with palette_lock uses only them.",
        linkedMapOf<String, Schema>("name" to string("Ramp name."), "colors" to colors("2-16 opaque colors, dark → light.")), listOf("name", "colors"))
    tool("ramp_delete", "Delete ramp", Access.WRITE, "Deletes a material ramp.", linkedMapOf<String, Schema>("name" to string("Ramp name.")), listOf("name"))
    tool("shade", "Shade brush", Access.WRITE,
        "Moves pixels along their own material ramp: steps -1 = one tone darker, +1 lighter (clamped at the ramp ends). Never invents colors — this is how pixel artists place shadows and highlights by hand. " +
            "Target a rectangle (x, y, width, height) or explicit pixels; only_color limits it to one color.",
        linkedMapOf<String, Schema>("steps" to integer("-15..15 (default -1).", -15, 15), "x" to integer("Left."), "y" to integer("Top."), "width" to integer("Width (default 1).", 1), "height" to integer("Height (default 1).", 1),
            "pixels" to pts, "only_color" to color("Only pixels of this color."),
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)))
    tool("draw_stroke", "Freehand stroke", Access.WRITE,
        "Draws a freehand polyline through points with pixel-perfect corners (no L-shaped doubles) — hair strands, cloth folds, outlines, sword trails. size 1-4.",
        linkedMapOf<String, Schema>("points" to pts, "color" to color(), "size" to integer("Brush size 1-4 (default 1).", 1, 4), "pixel_perfect" to boolean("Default true."),
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)),
        listOf("points", "color"))
    tool("auto_shade", "Auto cel-shade", Access.WRITE,
        "Shades flat-colored shapes like an anime pixel artist: every same-color region gets hue-shifted 2-4 tone cel shading that follows its outline " +
            "(lit edges toward the light, shadow on the far side), layers ABOVE cast shadows onto it (bangs on the forehead, head on the neck), optional rim light. " +
            "Workflow: block the character in flat colors on separate layers (hair back, body, face, hair front), auto_shade each, outline mode=selout, then hand-paint eyes and highlights. " +
            "Regions smaller than min_region (eyes, buttons) and 'preserve' colors are untouched.",
        linkedMapOf<String, Schema>(
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer (default active)."),
            "frame" to integer("Frame index (default active).", min = 0), "scope" to scope, "light" to light,
            "tones" to integer("2 = base+shadow, 3 = +highlight (default), 4 = +deep shadow.", 2, 4),
            "bevel" to num("How far edge shading reaches into shapes, px (default 3)."),
            "form" to num("Whole-shape light→shadow gradient 0-2 (default 0.45)."),
            "shadow" to num("-1..1: more (positive) or less shadow."),
            "cast_shadow" to integer("Shadow length cast by layers above, px (default 2, 0 = off).", 0, 64),
            "hue_shift" to num("Auto ramp hue shift in degrees (default 22)."), "contrast" to num("Auto ramp contrast 0.05-1 (default 0.5; pastel ≈ 0.35)."),
            "ramps" to linkedMapOf<String, Any?>("type" to "object", "description" to "Explicit ramps: {\"#base\": [shadow, highlight] | [shadow, base, highlight] | [deep, shadow, base, highlight]}."),
            "preserve" to colors("Colors never shaded."), "min_region" to integer("Smallest region shaded, px (default 6).", 1),
            "rim" to color("Rim-light color on edges facing away from the light."), "rim_strength" to num("Rim blend 0-1 (default 0.55)."),
            "palette_lock" to boolean("Shade only with the document's material ramps and leave other colors untouched (default: on when ramps exist)."),
        ))
    tool("gradient_map", "Gradient map (color grade)", Access.WRITE,
        "Color-grades by mapping luminance onto a dark→light gradient (rainy blue night, sunset, sepia flashback, unified palettes), blended by strength.",
        linkedMapOf<String, Schema>("colors" to colors("2-32 colors from dark to light."), "strength" to num("0-1 (default 1)."), "scope" to scope,
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)),
        listOf("colors"))
    tool("draw_sparkle", "Sparkles / stars", Access.WRITE,
        "Stamps 4-point sparkles (star4), crosses, diamonds or dots — the cute twinkles around chibi characters, magic, item shine, starry skies.",
        linkedMapOf<String, Schema>("x" to integer("Center x."), "y" to integer("Center y."), "size" to integer("Arm length 1-8 (default 3).", 1, 8),
            "points" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "Many sparkles: [[x, y] or [x, y, size], …]."),
            "style" to string("Shape.", listOf("star4", "cross", "diamond", "dot")), "color" to color("Arm color (default #FFE9B0)."), "core" to color("Center color (default white)."),
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)))
    tool("pixel_cleanup", "Pixel-perfect lines", Access.WRITE,
        "Removes L-shaped corner doubles from 1-px lines (the 'pixel perfect' pass) so outlines and strands read clean.",
        linkedMapOf<String, Schema>("color" to color("Only lines of this color."), "passes" to integer("1-4.", 1, 4), "scope" to scope,
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)))

    val layerRef: Schema = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer (default active).")
    tool("draw_polygon", "Draw polygon", Access.WRITE,
        "Draws a filled (default) or outlined polygon: roofs, gables, crystals, banners, blades, terrain slopes. Points are pixel coordinates.",
        linkedMapOf<String, Schema>(
            "points" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "[[x, y], …] (3-4096 points)."),
            "color" to color(), "filled" to boolean("Fill the inside (default true)."), "layer" to layerRef, "frame" to integer("Frame.", min = 0),
        ),
        listOf("points", "color"))
    tool("pattern_fill", "Surface pattern", Access.WRITE,
        "Textures a surface for buildings and terrain: brick, shingle (roof tiles), plank (wood), stone (cobbles), tile, grass, wave, checker. " +
            "By default it only touches existing pixels, so draw the shape flat first, then pattern it, then auto_shade. " +
            "color = seams (mortar, grout, joints); color2 = cell fill (optional); accent = some cells vary.",
        linkedMapOf<String, Schema>(
            "pattern" to string("Surface.", listOf("brick", "shingle", "plank", "stone", "tile", "grass", "wave", "checker")),
            "color" to color("Seam color."), "color2" to color("Cell fill (optional)."), "accent" to color("Variation color for some cells."),
            "size" to integer("Pattern scale 2-64 (default 4).", 2, 64), "seed" to integer("Variation seed."),
            "x" to integer("Region left."), "y" to integer("Region top."), "width" to integer("Region width.", 1), "height" to integer("Region height.", 1),
            "only_opaque" to boolean("Only over existing pixels (default true)."), "only_color" to color("Only over pixels of this color."),
            "layer" to layerRef, "frame" to integer("Frame.", min = 0),
        ),
        listOf("pattern", "color"))
    val fxKinds = listOf(
        "fire", "campfire", "slash", "hit", "magic", "smoke", "lightning", "heal", "explosion", "shockwave", "beam", "portal", "ice",
        "poison", "splash", "dash", "shield", "levelup", "pickup", "dust", "holy", "shadow", "teleport",
    )
    val pairOf: (String) -> Schema = { d -> linkedMapOf<String, Any?>("type" to listOf("array", "number"), "description" to d) }
    tool("fx_generate", "Generate animated effect", Access.WRITE,
        "Creates a complete animated pixel-art effect in ONE call (8-16 frames, anticipation → peak → decay, banded hot→cold ramps): " +
            "fire/campfire/shadow/poison/portal/shield/heal/pickup (loops), slash, hit, magic, smoke, lightning, explosion, shockwave, beam, ice, " +
            "splash, dash, levelup, dust, holy, teleport. Timing 'auto' rushes the anticipation, holds the peak frame and eases out (loops stay even). " +
            "glow=true (or a color) adds the 'lit' tier: a dithered additive halo on its own Add-blend layer. Frames are added as needed; other layers are held.",
        linkedMapOf<String, Schema>(
            "kind" to string("Effect.", fxKinds),
            "frames" to integer("Frame count (default per effect, 8-16).", 1, 64), "frame_ms" to integer("Base ms per frame.", 1, 60000),
            "timing" to string("auto (held peak, slow-out) or uniform.", listOf("auto", "uniform")),
            "size" to integer("Effect size in px (default: canvas).", 4, 1024), "x" to integer("Center x."), "y" to integer("Center y."),
            "frame" to integer("First frame index (default 0).", min = 0), "tag" to string("Animation tag to create."),
            "colors" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to "Replacement ramp colors, brightest first."),
            "glow" to linkedMapOf<String, Any?>("type" to listOf("boolean", "string"), "description" to "Additive glow halo: true or a color."),
            "glow_layer" to string("Glow layer name (default '<layer> Glow')."),
            "layer" to layerRef,
        ),
        listOf("kind"))
    tool("fx_particles", "Particle effect", Access.WRITE,
        "General particle emitter for custom effects (rain, snow, embers, sparks, debris, dust, confetti, coins, bubbles): burst or looping stream, " +
            "speed/gravity/drag physics, a color ramp over each particle's life, size over life, shapes dot/square/plus/star/streak. Frames are added as needed.",
        linkedMapOf<String, Schema>(
            "frames" to integer("Frames (default 8).", 1, 64), "frame_ms" to integer("ms per frame (default 60).", 1, 60000),
            "count" to integer("Particles (default 24).", 1, 2000), "origin" to pairOf("[x, y] emitter (default canvas center)."),
            "area" to pairOf("[half-width, half-height] of the emitter area (default point)."), "angle" to num("Direction in degrees (-90 = up, default)."),
            "spread" to num("Cone in degrees (default 360)."), "speed" to pairOf("[min, max] px per frame (default [1, 3])."),
            "gravity" to num("px/frame² (negative rises; default 0.15)."), "drag" to num("0-0.95 velocity loss per frame (default 0.02)."),
            "life" to pairOf("[min, max] frames each particle lives."), "size" to pairOf("[start, end] size over life (default [2, 1])."),
            "shape" to string("Particle shape.", listOf("dot", "square", "plus", "star", "streak")),
            "emit" to string("burst (default) or stream.", listOf("burst", "stream")), "loop" to boolean("Seamless loop (default true for streams)."),
            "colors" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to "Color ramp over life."),
            "seed" to integer("Random seed."), "clear" to boolean("Clear the layer in those frames first (default true)."),
            "frame" to integer("First frame (default 0).", min = 0), "tag" to string("Animation tag to create."), "layer" to layerRef,
        ))
    tool("anim_generate", "Animate a still sprite", Access.WRITE,
        "Turns a still sprite into an 8+ frame loop with secondary motion: breathe (1-px chest/head rise), bob, float, sway (top lags: follow-through for " +
            "trees, hair, capes), squash (area-preserving), shake, pulse (glow), hurt (white hit-flash, then decaying shake). Frame 0 stays the original " +
            "pose, extremes are held longer (slow-in/out) and every layer with content moves together unless 'layer' restricts it.",
        linkedMapOf<String, Schema>(
            "kind" to string("Motion.", listOf("breathe", "bob", "float", "sway", "squash", "shake", "pulse", "hurt")),
            "frames" to integer("Frames (default 8).", 2, 64), "frame_ms" to integer("Base ms per frame.", 1, 60000),
            "amount" to num("Strength: px for breathe/bob/float/sway/shake/hurt, fraction for squash, brightness for pulse."),
            "frame" to integer("Source pose frame (default 0).", min = 0), "tag" to string("Animation tag to create."),
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Only this layer moves (others are held)."),
        ),
        listOf("kind"))
}

// ═══ FunctionSchemas ═════════════════════════════════════════════════════════════════════════════

/**
 * Converts the shared tool catalogue into the function-calling formats of model APIs, so a
 * custom agent can hand them to any model and forward calls to `POST /api/v1/tools/{name}`.
 */
object FunctionSchemas {
    enum class Format { MCP, OPENAI, ANTHROPIC, GEMINI }

    fun parseFormat(s: String?): Format? = when (s?.lowercase() ?: "mcp") {
        "mcp" -> Format.MCP
        "openai", "chatgpt" -> Format.OPENAI
        "anthropic", "claude" -> Format.ANTHROPIC
        "gemini", "google" -> Format.GEMINI
        else -> null
    }

    fun mcpTool(t: ToolDef): Map<String, Any?> = linkedMapOf(
        "name" to t.name,
        "title" to t.title,
        "description" to t.description,
        "inputSchema" to t.inputSchema(),
        "annotations" to t.annotations(),
    )

    fun export(format: Format): Any = when (format) {
        Format.MCP -> mapOf("tools" to ToolCatalog.tools.map(::mcpTool))
        Format.OPENAI -> ToolCatalog.tools.map {
            linkedMapOf("type" to "function", "function" to linkedMapOf("name" to it.name, "description" to it.description, "parameters" to it.inputSchema()))
        }
        Format.ANTHROPIC -> ToolCatalog.tools.map {
            linkedMapOf("name" to it.name, "description" to it.description, "input_schema" to it.inputSchema())
        }
        Format.GEMINI -> mapOf("functionDeclarations" to ToolCatalog.tools.map {
            linkedMapOf("name" to it.name, "description" to it.description, "parameters" to geminiSchema(it.inputSchema()))
        })
    }

    /**
     * Gemini accepts an OpenAPI-style subset: no `additionalProperties`, no `pattern`, and a
     * single `type`. Union types become string-or-number friendly descriptions.
     */
    @Suppress("UNCHECKED_CAST")
    private fun geminiSchema(schema: Map<String, Any?>): Map<String, Any?> {
        val out = LinkedHashMap<String, Any?>()
        var note = ""
        for ((k, v) in schema) {
            when (k) {
                "additionalProperties", "pattern" -> {}
                "type" -> out["type"] = if (v is List<*>) (if ("string" in v) "string" else "object") else v
                "properties" -> out["properties"] = (v as Map<String, Any?>).mapValues { geminiSchema(it.value as Map<String, Any?>) }
                "items" -> out["items"] = geminiSchema(v as Map<String, Any?>)
                "enum" -> if ((v as List<*>).all { it is String }) out["enum"] = v
                    else note += " Allowed: ${v.joinToString()}."
                else -> out[k] = v
            }
        }
        if (schema["type"] is List<*>) note += " (number or name as text)"
        if (note.isNotEmpty()) out["description"] = ((out["description"] as? String ?: "") + note).trim()
        return out
    }
}

// ═══ ToolExecutor ════════════════════════════════════════════════════════════════════════════════

enum class Decision { ALLOW, CONFIRM, DENY }

/** Asks the human whether an agent may perform a destructive action. Must be thread-safe. */
fun interface Confirmer {
    fun confirm(actor: String, tool: String, summary: String): Boolean

    companion object {
        /** Headless default: never approve destructive actions silently. */
        val DENY_ALL = Confirmer { _, _, _ -> false }
        val APPROVE_ALL = Confirmer { _, _, _ -> true }
    }
}

/**
 * Permission model: reads and drawing are allowed, destructive actions need the user's
 * confirmation, and individual tools can be overridden (e.g. DENY project_new).
 * Agents only ever reach Pixel Studio tools — there is no shell or file-system tool.
 */
class PermissionPolicy(
    private val overrides: Map<String, Decision> = emptyMap(),
    @Volatile var confirmer: Confirmer = Confirmer.DENY_ALL,
) {
    fun decide(tool: String, access: Access): Decision = overrides[tool] ?: when (access) {
        Access.READ, Access.WRITE -> Decision.ALLOW
        Access.DESTRUCTIVE -> Decision.CONFIRM
    }

    /** Returns null when allowed, or a human-readable refusal. */
    fun authorize(actor: String, tool: String, access: Access, summary: String): String? = when (decide(tool, access)) {
        Decision.ALLOW -> null
        Decision.DENY -> "'$tool' is disabled by the user's permission settings"
        Decision.CONFIRM -> if (confirmer.confirm(actor, tool, summary)) null else "the user did not approve '$tool'"
    }
}

/**
 * Where agents may write files: only inside the workspace (project files and Exports/).
 * File names are sanitized; paths from agents are never used directly.
 */
interface Workspace {
    fun saveProject(name: String, bytes: ByteArray): String
    fun writeExport(name: String, extension: String, bytes: ByteArray): String
}

class DirectoryWorkspace(private val root: File) : Workspace {
    override fun saveProject(name: String, bytes: ByteArray): String =
        write(File(root, sanitize(name) + ".pxs"), bytes)

    override fun writeExport(name: String, extension: String, bytes: ByteArray): String =
        write(File(File(root, "Exports"), sanitize(name) + "." + extension), bytes)

    private fun write(target: File, bytes: ByteArray): String {
        val canonical = target.canonicalFile
        require(canonical.path.startsWith(root.canonicalFile.path + File.separator)) { "refusing to write outside the workspace" }
        canonical.parentFile.mkdirs()
        val tmp = File(canonical.parentFile, canonical.name + ".tmp")
        tmp.writeBytes(bytes)
        if (!tmp.renameTo(canonical)) {
            canonical.delete()
            check(tmp.renameTo(canonical)) { "could not write ${canonical.name}" }
        }
        return canonical.path
    }

    companion object {
        fun sanitize(name: String): String {
            val cleaned = name.trim().map { if (it.isLetterOrDigit() || it == '-' || it == '_') it else '_' }.joinToString("").trim('_')
            return cleaned.take(64).ifEmpty { "untitled" }
        }
    }
}

class ToolResult(
    val content: List<Map<String, Any?>>,
    val structured: Map<String, Any?>?,
    val isError: Boolean,
) {
    fun toMcp(): LinkedHashMap<String, Any?> = linkedMapOf<String, Any?>("content" to content).apply {
        if (structured != null) put("structuredContent", structured)
        put("isError", isError)
    }

    fun toJson(): Map<String, Any?> = toMcp()
}

class UnknownToolException(name: String) : IllegalArgumentException("unknown tool '$name'")

/** Executes catalogue tools against the engine. Shared by MCP and the REST tool endpoint. */
class ToolExecutor(
    private val engine: PixelEngine,
    val policy: PermissionPolicy,
    private val workspace: Workspace?,
) {
    fun call(name: String, rawArgs: Map<String, Any?>?, actor: String): ToolResult {
        val tool = ToolCatalog[name] ?: throw UnknownToolException(name)
        val args = LinkedHashMap(rawArgs ?: emptyMap())
        args.putAll(tool.fixed)
        val expectedRevision = (args.remove("expectedRevision") as? Number)?.toLong()
        val operationId = args.remove("operationId") as? String

        var access = tool.access
        if (tool.name == "transaction") {
            // A batch is as dangerous as its most dangerous command.
            val ops = (args["commands"] as? List<*>)?.mapNotNull { (it as? Map<*, *>)?.get("op") as? String } ?: emptyList()
            if (ops.any { ToolCatalog.accessForOp(it) == Access.DESTRUCTIVE }) access = Access.DESTRUCTIVE
        }
        policy.authorize(actor, tool.name, access, summarize(tool, args))?.let { return refusal(it) }

        return when (tool.output) {
            Output.JSON -> jsonResult(engine.execute(tool.op, args, actor, operationId, expectedRevision))
            Output.PNG_IMAGE -> imageResult(engine.execute(tool.op, args, actor), "image/png")
            Output.GIF_IMAGE -> imageResult(engine.execute(tool.op, args, actor), "image/gif")
            Output.PROJECT_SAVE -> saveProject(args, actor)
            Output.PROJECT_EXPORT -> export(args, actor)
        }
    }

    private fun summarize(tool: ToolDef, args: Map<String, Any?>): String {
        val text = Json.write(args)
        return "${tool.title}: " + if (text.length > 200) text.take(200) + "…" else text
    }

    private fun refusal(message: String) = ToolResult(
        listOf(text(Json.write(mapOf("ok" to false, "error" to mapOf("code" to "permission_denied", "message" to message))))),
        mapOf("ok" to false, "error" to mapOf("code" to "permission_denied", "message" to message)),
        isError = true,
    )

    private fun text(s: String) = linkedMapOf<String, Any?>("type" to "text", "text" to s)

    private fun image(bytes: ByteArray, mime: String) =
        linkedMapOf<String, Any?>("type" to "image", "data" to Base64.getEncoder().encodeToString(bytes), "mimeType" to mime)

    private fun structured(r: EngineResponse): Map<String, Any?> = linkedMapOf(
        "ok" to r.ok,
        "revision" to r.revision,
        (if (r.ok) "result" else "error") to (if (r.ok) r.result else r.json["error"]),
    )

    private fun jsonResult(r: EngineResponse): ToolResult {
        val s = structured(r)
        val hint = when (r.errorCode) {
            "conflict" -> "\nHint: someone else changed the canvas. Call canvas_inspect (or canvas_snapshot), then retry with the new revision."
            "locked" -> "\nHint: the layer is locked; unlock it with layer_set locked=false or draw on another layer."
            else -> ""
        }
        return ToolResult(listOf(text(Json.write(s) + hint)), s, isError = !r.ok)
    }

    private fun imageResult(r: EngineResponse, mime: String): ToolResult {
        if (!r.ok) return jsonResult(r)
        val s = structured(r)
        // Very large sheets are summarized instead of inlined (MCP clients have message limits).
        if (r.binary.size > 6 * 1024 * 1024) {
            return ToolResult(listOf(text(Json.write(s) + "\nImage is ${r.binary.size} bytes; render fewer directions/frames or a smaller width to view it inline.")), s, isError = false)
        }
        return ToolResult(listOf(image(r.binary, mime), text(Json.write(s))), s, isError = false)
    }

    private fun saveProject(args: Map<String, Any?>, actor: String): ToolResult {
        val ws = workspace ?: return refusal("this Pixel Studio instance has no workspace folder configured")
        val r = engine.execute("project_save", emptyMap(), actor)
        if (!r.ok) return jsonResult(r)
        val name = args.str("name") ?: engine.execute("canvas_inspect").result.str("name") ?: "untitled"
        val path = runCatching { ws.saveProject(name, r.binary) }.getOrElse { return refusal("save failed: ${it.message}") }
        val s = linkedMapOf<String, Any?>("ok" to true, "revision" to r.revision, "result" to mapOf("file" to File(path).name, "bytes" to r.binary.size))
        return ToolResult(listOf(text(Json.write(s))), s, isError = false)
    }

    private fun export(args: Map<String, Any?>, actor: String): ToolResult {
        val format: String = args.str("format") ?: return refusal("missing 'format'")
        if (format !in EXPORT_FORMATS) return refusal("unknown format '$format' (${EXPORT_FORMATS.joinToString(", ")})")
        val op = "export"
        val mime = if (format == "gif") "image/gif" else "image/png"
        val ext = if (format == "gif") "gif" else "png"
        val projectName: String = engine.execute("canvas_inspect").result.str("name") ?: "export"
        val base: String = args.str("name") ?: (projectName + "_" + format)
        val safeBase: String = DirectoryWorkspace.sanitize(base)
        val engineArgs = LinkedHashMap<String, Any?>(args)
        engineArgs.remove("name")
        if (format == "godot") engineArgs.putIfAbsent("texture_path", "res://$safeBase.png")
        if (format == "atlas") engineArgs.putIfAbsent("image", "$safeBase.png")
        if (format == "tiled") engineArgs.putIfAbsent("image", "${safeBase}_tileset.png")
        val r = engine.execute(op, engineArgs, actor)
        if (!r.ok) return jsonResult(r)
        val files = ArrayList<String>()
        val ws = workspace
        if (ws != null) {
            try {
                writeExportFiles(ws, format, base, ext, r, files)
            } catch (e: Exception) {
                return refusal("export failed: ${e.message}")
            }
        }
        val summary = LinkedHashMap<String, Any?>(r.result)
        // Keep the reply small: the full documents are in the written files.
        if (format == "atlas" || format == "godot" || format == "tiled") {
            summary.remove("data")
            summary.remove("tres")
            summary.remove("tmj")
        }
        summary["files"] = files
        val s = linkedMapOf<String, Any?>("ok" to true, "revision" to r.revision, "result" to summary)
        val content = ArrayList<Map<String, Any?>>()
        if (r.binary.size <= 4 * 1024 * 1024) content.add(image(r.binary, mime))
        content.add(text(Json.write(s)))
        return ToolResult(content, s, isError = false)
    }

    private fun writeExportFiles(ws: Workspace, format: String, base: String, ext: String, r: EngineResponse, files: MutableList<String>) {
        val imageName = if (format == "tiled") base + "_tileset" else base
        files.add(File(ws.writeExport(imageName, ext, r.binary)).name)
        val extra: Pair<String, String>? = when (format) {
            "spritesheet" -> Pair("json", Json.write(r.result))
            "atlas" -> Pair("json", Json.write(r.result["data"]))
            "godot" -> Pair("tres", r.result.str("tres") ?: "")
            "tiled" -> Pair("tmj", Json.write(r.result["tmj"]))
            else -> null
        }
        if (extra != null) files.add(File(ws.writeExport(base, extra.first, extra.second.toByteArray())).name)
    }

    companion object {
        private val EXPORT_FORMATS = listOf("png", "gif", "spritesheet", "atlas", "godot", "tiled", "tileset", "normal_map")

        /** Engine operations every catalogue tool relies on (checked against the engine in tests). */
        fun engineOps(): Set<String> = ToolCatalog.tools.map { it.op }.toSet()
    }
}

// ═══ ToolSchemaExport ════════════════════════════════════════════════════════════════════════════

/** Pretty JSON for the committed schema files (stable, diff-friendly). */
fun prettyJson(value: Any?, indent: String = ""): String {
    val next = "$indent  "
    return when (value) {
        is Map<*, *> -> if (value.isEmpty()) "{}" else value.entries.joinToString(",\n", "{\n", "\n$indent}") { (k, v) ->
            next + Json.write(k.toString()) + ": " + prettyJson(v, next)
        }
        is List<*> -> if (value.isEmpty()) "[]" else if (value.all { it !is Map<*, *> && it !is List<*> }) Json.write(value)
        else value.joinToString(",\n", "[\n", "\n$indent]") { next + prettyJson(it, next) }
        else -> Json.write(value)
    }
}

fun schemaFiles(): Map<String, String> = FunctionSchemas.Format.entries.associate { f ->
    "pixel-studio-tools.${f.name.lowercase()}.json" to prettyJson(FunctionSchemas.export(f)) + "\n"
}

/** Writes the committed tool schema files (`--export-schemas <dir>`). */
fun exportSchemas(path: String?) {
    val dir = File(path ?: "Gateway/Schemas")
    dir.mkdirs()
    for ((name, text) in schemaFiles()) File(dir, name).writeText(text)
    println("Wrote ${schemaFiles().size} schema files to ${dir.absolutePath}")
}
