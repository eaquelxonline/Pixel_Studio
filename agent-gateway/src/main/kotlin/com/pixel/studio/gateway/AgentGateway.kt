package com.pixel.studio.gateway

import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.gateway.auth.AccessControl
import com.pixel.studio.gateway.http.HttpRequest
import com.pixel.studio.gateway.http.HttpResponse
import com.pixel.studio.gateway.http.HttpServer
import com.pixel.studio.gateway.mcp.McpHttpEndpoint
import com.pixel.studio.gateway.mcp.McpServer
import com.pixel.studio.gateway.resources.ResourceCatalog
import com.pixel.studio.gateway.rest.RestApi
import com.pixel.studio.gateway.tools.Confirmer
import com.pixel.studio.gateway.tools.PermissionPolicy
import com.pixel.studio.gateway.tools.ToolExecutor
import com.pixel.studio.gateway.tools.Workspace
import com.pixel.studio.json.Json
import javax.net.ServerSocketFactory

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
