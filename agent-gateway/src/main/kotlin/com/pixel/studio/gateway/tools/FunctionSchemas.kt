package com.pixel.studio.gateway.tools

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
