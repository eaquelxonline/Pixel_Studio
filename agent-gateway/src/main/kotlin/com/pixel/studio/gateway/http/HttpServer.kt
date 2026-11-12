package com.pixel.studio.gateway.http

import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.net.URLDecoder
import java.util.Locale
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import javax.net.ServerSocketFactory

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
