package com.pixel.studio.gateway.tools

import com.pixel.studio.bridge.EngineResponse
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.json.Json
import com.pixel.studio.json.str
import java.io.File
import java.util.Base64

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
            "locked" -> "\nHint: the layer is locked; unlock it with layer_lock or draw on another layer."
            else -> ""
        }
        return ToolResult(listOf(text(Json.write(s) + hint)), s, isError = !r.ok)
    }

    private fun imageResult(r: EngineResponse, mime: String): ToolResult {
        if (!r.ok) return jsonResult(r)
        val s = structured(r)
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
        val format = args.str("format") ?: return refusal("missing 'format'")
        val (op, ext, mime) = when (format) {
            "png" -> Triple("export_png", "png", "image/png")
            "gif" -> Triple("export_gif", "gif", "image/gif")
            "spritesheet" -> Triple("export_spritesheet", "png", "image/png")
            else -> return refusal("unknown format '$format' (png, gif, spritesheet)")
        }
        val engineArgs = LinkedHashMap(args).apply { remove("format"); remove("name") }
        val r = engine.execute(op, engineArgs, actor)
        if (!r.ok) return jsonResult(r)
        val base = args.str("name") ?: (engine.execute("canvas_inspect").result.str("name") ?: "export") + "_" + format
        val files = mutableListOf<String>()
        workspace?.let { ws ->
            runCatching {
                files += File(ws.writeExport(base, ext, r.binary)).name
                if (format == "spritesheet") files += File(ws.writeExport(base, "json", Json.write(r.result).toByteArray())).name
            }.onFailure { return refusal("export failed: ${it.message}") }
        }
        val s = linkedMapOf<String, Any?>("ok" to true, "revision" to r.revision, "result" to LinkedHashMap(r.result).apply { put("files", files) })
        val content = mutableListOf<Map<String, Any?>>()
        if (r.binary.size <= 4 * 1024 * 1024) content += image(r.binary, mime)
        content += text(Json.write(s))
        return ToolResult(content, s, isError = false)
    }

    companion object {
        /** Engine operations every catalogue tool relies on (checked against the engine in tests). */
        fun engineOps(): Set<String> = ToolCatalog.tools.map { it.op }.filter { it != "export" }.toSet() +
            setOf("export_png", "export_gif", "export_spritesheet")
    }
}
