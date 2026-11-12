package com.pixel.studio.gateway.rest

import com.pixel.studio.bridge.EngineResponse
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.gateway.http.HttpRequest
import com.pixel.studio.gateway.http.HttpResponse
import com.pixel.studio.gateway.tools.Access
import com.pixel.studio.gateway.tools.FunctionSchemas
import com.pixel.studio.gateway.tools.ToolCatalog
import com.pixel.studio.gateway.tools.ToolExecutor
import com.pixel.studio.gateway.tools.UnknownToolException
import com.pixel.studio.json.Json
import com.pixel.studio.json.long
import com.pixel.studio.json.str
import java.util.Base64

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
                req.method == "GET" && path == "/project" -> engineJson(engine.execute("project_inspect"))
                req.method == "GET" && path == "/canvas" -> engineJson(engine.execute("canvas_inspect"))
                req.method == "GET" && segments.size == 2 && segments[0] == "frame" ->
                    engineJson(engine.execute("frame_get", mapOf("frame" to segments[1].toLongOrNull())))
                req.method == "GET" && segments.size == 3 && segments[0] == "frame" && segments[2] == "pixels" ->
                    engineJson(engine.execute("read_region", queryArgs(req) + ("frame" to segments[1].toLongOrNull())))
                req.method == "GET" && path == "/snapshot" -> binary(engine.execute("export_png", queryArgs(req)), "image/png")
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
        val (op, mime) = when (args.remove("format") ?: "png") {
            "png" -> "export_png" to "image/png"
            "gif" -> "export_gif" to "image/gif"
            "spritesheet" -> "export_spritesheet" to "image/png"
            "pxs" -> "project_save" to "application/octet-stream"
            else -> return badRequest("format must be png, gif, spritesheet or pxs")
        }
        val r = engine.execute(op, args, actor)
        if (!r.ok) return engineJson(r)
        val headers = LinkedHashMap<String, String>()
        headers["X-Pixel-Revision"] = r.revision.toString()
        if (op == "export_spritesheet") headers["X-Pixel-Metadata"] = Base64.getEncoder().encodeToString(Json.write(r.result).toByteArray())
        return HttpResponse(200, r.binary, mime, headers)
    }
}
