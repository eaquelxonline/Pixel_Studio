// Pixel Studio — STDIO MCP adapter (`pixstudio-mcp`): forwards subprocess MCP traffic to the running app.

package com.pixel.studio.gateway

import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.PrintStream
import java.net.HttpURLConnection
import java.net.URI

// ═══ StdioAdapter ════════════════════════════════════════════════════════════════════════════════

/**
 * `pixstudio-mcp`: a tiny STDIO MCP server for agents that launch MCP servers as
 * subprocesses. It holds no drawing engine; every newline-delimited JSON-RPC message is
 * forwarded to the running Pixel Studio app's HTTP endpoint and the reply is written back.
 *
 * Usage: `pixstudio-mcp --url http://127.0.0.1:38471/mcp --token <token>`
 * (or env PIXEL_STUDIO_URL / PIXEL_STUDIO_TOKEN).
 */
fun main(args: Array<String>) {
    fun opt(name: String) = args.indexOf(name).takeIf { it >= 0 && it + 1 < args.size }?.let { args[it + 1] }
    val url = opt("--url") ?: System.getenv("PIXEL_STUDIO_URL") ?: "http://127.0.0.1:38471/mcp"
    val token = opt("--token") ?: System.getenv("PIXEL_STUDIO_TOKEN")
    if (token.isNullOrBlank()) {
        System.err.println("pixstudio-mcp: missing --token (copy it from Pixel Studio → Connect an agent)")
        kotlin.system.exitProcess(2)
    }
    // stdout carries protocol messages only; diagnostics go to stderr.
    val out = PrintStream(System.out, true, "UTF-8")
    StdioBridge(url, token).run(BufferedReader(InputStreamReader(System.`in`, Charsets.UTF_8)), out)
}

class StdioBridge(private val url: String, private val token: String) {
    private var sessionId: String? = null
    private var legacyVersion: String? = null

    fun run(input: BufferedReader, out: PrintStream) {
        while (true) {
            val line = input.readLine() ?: return
            if (line.isBlank()) continue
            val reply = forward(line)
            if (reply != null) out.println(reply)
        }
    }

    /** Returns the line to write to stdout, or null (notifications). */
    fun forward(line: String): String? {
        val msg = try {
            Json.parse(line)
        } catch (e: Json.ParseException) {
            return Json.write(mapOf("jsonrpc" to "2.0", "id" to null, "error" to mapOf("code" to -32700, "message" to "parse error")))
        }
        @Suppress("UNCHECKED_CAST")
        val obj = msg as? Map<String, Any?>
        val id = obj?.get("id")
        val method = obj?.str("method")
        val c = URI(url).toURL().openConnection() as HttpURLConnection
        return try {
            c.requestMethod = "POST"
            c.connectTimeout = 5_000
            c.readTimeout = 120_000
            c.doOutput = true
            c.setRequestProperty("Content-Type", "application/json")
            c.setRequestProperty("Accept", "application/json, text/event-stream")
            c.setRequestProperty("Authorization", "Bearer $token")
            // Mirror the modern (2026-07-28) routing headers from the message itself.
            val version = obj?.obj("params")?.obj("_meta")?.str("io.modelcontextprotocol/protocolVersion")
            if (version != null && method != null) {
                c.setRequestProperty("MCP-Protocol-Version", version)
                c.setRequestProperty("Mcp-Method", method)
                val params = obj.obj("params")
                when (method) {
                    "tools/call", "prompts/get" -> params?.str("name")?.let { c.setRequestProperty("Mcp-Name", it) }
                    "resources/read" -> params?.str("uri")?.let { c.setRequestProperty("Mcp-Name", it) }
                }
            } else {
                sessionId?.let { c.setRequestProperty("Mcp-Session-Id", it) }
                legacyVersion?.let { c.setRequestProperty("MCP-Protocol-Version", it) }
            }
            c.outputStream.use { it.write(line.toByteArray(Charsets.UTF_8)) }
            val status = c.responseCode
            c.getHeaderField("Mcp-Session-Id")?.let { sessionId = it }
            val body = (if (status >= 400) c.errorStream else c.inputStream)?.use { String(it.readBytes(), Charsets.UTF_8) } ?: ""
            if (method == "initialize" && status == 200) {
                legacyVersion = runCatching { Json.parseObject(body).obj("result")?.str("protocolVersion") }.getOrNull()
            }
            when {
                status == 202 || body.isBlank() -> if (id == null) null else error(id, -32603, "empty response (HTTP $status)")
                body.trimStart().startsWith("{") || body.trimStart().startsWith("[") -> body.replace("\n", "")
                else -> error(id, -32603, "Pixel Studio returned HTTP $status")
            }
        } catch (e: java.io.IOException) {
            if (id == null) null else error(id, -32603, "Pixel Studio is not reachable at $url — is the app open with Agent Mode on? (${e.message})")
        } finally {
            c.disconnect()
        }
    }

    private fun error(id: Any?, code: Int, message: String) =
        Json.write(mapOf("jsonrpc" to "2.0", "id" to id, "error" to mapOf("code" to code, "message" to message)))
}
