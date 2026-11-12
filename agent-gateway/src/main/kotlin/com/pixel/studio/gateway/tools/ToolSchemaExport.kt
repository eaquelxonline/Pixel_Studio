package com.pixel.studio.gateway.tools

import com.pixel.studio.json.Json
import java.io.File

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

fun main(args: Array<String>) {
    val dir = File(args.firstOrNull() ?: "protocol/tools")
    dir.mkdirs()
    for ((name, text) in schemaFiles()) File(dir, name).writeText(text)
    println("Wrote ${schemaFiles().size} schema files to ${dir.absolutePath}")
}
