package com.pixel.studio.gateway.mcp

import com.pixel.studio.gateway.http.HttpRequest
import com.pixel.studio.gateway.http.HttpResponse
import com.pixel.studio.gateway.jsonrpc.JsonRpc
import com.pixel.studio.json.Json
import com.pixel.studio.json.obj
import com.pixel.studio.json.str
import java.security.SecureRandom
import java.util.Collections

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
