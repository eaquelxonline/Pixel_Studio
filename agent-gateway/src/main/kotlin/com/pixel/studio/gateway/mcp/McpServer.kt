package com.pixel.studio.gateway.mcp

import com.pixel.studio.gateway.jsonrpc.JsonRpc
import com.pixel.studio.gateway.jsonrpc.RpcException
import com.pixel.studio.gateway.prompts.Guidance
import com.pixel.studio.gateway.resources.ResourceCatalog
import com.pixel.studio.gateway.tools.FunctionSchemas
import com.pixel.studio.gateway.tools.ToolCatalog
import com.pixel.studio.gateway.tools.ToolExecutor
import com.pixel.studio.gateway.tools.UnknownToolException
import com.pixel.studio.json.obj
import com.pixel.studio.json.str

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
